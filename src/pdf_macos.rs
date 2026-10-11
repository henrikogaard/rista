use super::*;
use objc2::{
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject},
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::{
    collections::VecDeque,
    ffi::c_void,
    fs::{self, File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    slice,
    sync::OnceLock,
    time::UNIX_EPOCH,
};

#[link(name = "PDFKit", kind = "framework")]
unsafe extern "C" {}

const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PAGE_COUNT: usize = 10_000;
const MAX_IMAGE_SIDE: f64 = 4096.0;
const MAX_IMAGE_AREA: f64 = 8_000_000.0;
const MAX_SEARCH_MATCHES: usize = 1000;
const DOCUMENT_CACHE_CAPACITY: usize = 4;
const WORK_QUEUE_CAPACITY: usize = 16;
const PDF_DISPLAY_BOX_CROP: isize = 1;
const NS_BITMAP_IMAGE_FILE_TYPE_PNG: usize = 4;
const NS_UTF8_STRING_ENCODING: usize = 4;
const NS_CASE_INSENSITIVE_SEARCH: usize = 1;
const NS_LITERAL_SEARCH: usize = 2;

static WORKER: OnceLock<Option<smol::channel::Sender<Command>>> = OnceLock::new();

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    length: u64,
    modified: Option<(u64, u32)>,
    device: u64,
    inode: u64,
}

struct CacheEntry {
    path: PathBuf,
    fingerprint: Fingerprint,
    document: NativeDocument,
}

#[derive(Clone)]
struct NativeDocument {
    object: Retained<AnyObject>,
    info: PdfInfo,
}

#[derive(Clone, Copy)]
struct PageGeometry {
    bounds: NSRect,
    rotation: u16,
}

enum Command {
    Inspect {
        path: PathBuf,
        response: smol::channel::Sender<Result<PdfInfo, PdfError>>,
    },
    Render {
        path: PathBuf,
        page: usize,
        pixel_width: u32,
        response: smol::channel::Sender<Result<PageImage, PdfError>>,
    },
    Select {
        path: PathBuf,
        page: usize,
        start: [f64; 2],
        end: [f64; 2],
        response: smol::channel::Sender<Result<Selection, PdfError>>,
    },
    Search {
        path: PathBuf,
        query: String,
        response: smol::channel::Sender<Result<SearchResults, PdfError>>,
    },
}

impl Command {
    fn is_closed(&self) -> bool {
        match self {
            Self::Inspect { response, .. } => response.is_closed(),
            Self::Render { response, .. } => response.is_closed(),
            Self::Select { response, .. } => response.is_closed(),
            Self::Search { response, .. } => response.is_closed(),
        }
    }

    fn fail_unavailable(self) {
        match self {
            Self::Inspect { response, .. } => {
                let _ = response.try_send(Err(PdfError::Unavailable));
            }
            Self::Render { response, .. } => {
                let _ = response.try_send(Err(PdfError::Unavailable));
            }
            Self::Select { response, .. } => {
                let _ = response.try_send(Err(PdfError::Unavailable));
            }
            Self::Search { response, .. } => {
                let _ = response.try_send(Err(PdfError::Unavailable));
            }
        }
    }

    fn run(self, worker: &mut Worker) {
        match self {
            Self::Inspect { path, response } => {
                if !response.is_closed() {
                    let result = worker.inspect(&path);
                    if !response.is_closed() {
                        let _ = response.try_send(result);
                    }
                }
            }
            Self::Render {
                path,
                page,
                pixel_width,
                response,
            } => {
                if !response.is_closed() {
                    let result = worker.render_page(&path, page, pixel_width);
                    if !response.is_closed() {
                        let _ = response.try_send(result);
                    }
                }
            }
            Self::Select {
                path,
                page,
                start,
                end,
                response,
            } => {
                if !response.is_closed() {
                    let result = worker.select(&path, page, start, end);
                    if !response.is_closed() {
                        let _ = response.try_send(result);
                    }
                }
            }
            Self::Search {
                path,
                query,
                response,
            } => {
                if !response.is_closed() {
                    let result = worker.search(&path, &query, &response);
                    if !response.is_closed() {
                        let _ = response.try_send(result);
                    }
                }
            }
        }
    }
}

struct Worker {
    documents: VecDeque<CacheEntry>,
}

impl Worker {
    fn new() -> Self {
        Self {
            documents: VecDeque::with_capacity(DOCUMENT_CACHE_CAPACITY),
        }
    }

    fn inspect(&mut self, path: &Path) -> Result<PdfInfo, PdfError> {
        Ok(self.document(path)?.info)
    }

    fn render_page(
        &mut self,
        path: &Path,
        page_index: usize,
        pixel_width: u32,
    ) -> Result<PageImage, PdfError> {
        if pixel_width == 0 {
            return Err(PdfError::Invalid);
        }

        let document = self.document(path)?;
        let page = page_at(&document, page_index)?;
        let geometry = page_geometry(&page)?;
        let (display_width, display_height) = display_size(geometry)?;
        let (width, height) =
            bounded_image_size(f64::from(pixel_width), display_width, display_height)?;

        let image: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                &*page,
                thumbnailOfSize: NSSize::new(width, height),
                forBox: PDF_DISPLAY_BOX_CROP
            ]
        };
        let image = image.ok_or(PdfError::Invalid)?;
        let tiff: Option<Retained<AnyObject>> = unsafe { msg_send![&*image, TIFFRepresentation] };
        let tiff = tiff.ok_or(PdfError::Invalid)?;

        let bitmap_class = AnyClass::get(c"NSBitmapImageRep").ok_or(PdfError::Unavailable)?;
        let allocated: *mut AnyObject = unsafe { msg_send![bitmap_class, alloc] };
        if allocated.is_null() {
            return Err(PdfError::Unavailable);
        }
        let initialized: *mut AnyObject = unsafe { msg_send![allocated, initWithData: &*tiff] };
        let bitmap = unsafe { Retained::from_raw(initialized) }.ok_or(PdfError::Invalid)?;

        let pixels_wide: isize = unsafe { msg_send![&*bitmap, pixelsWide] };
        let pixels_high: isize = unsafe { msg_send![&*bitmap, pixelsHigh] };
        if pixels_wide <= 0
            || pixels_high <= 0
            || pixels_wide as f64 > MAX_IMAGE_SIDE
            || pixels_high as f64 > MAX_IMAGE_SIDE
            || (pixels_wide as f64 * pixels_high as f64) > MAX_IMAGE_AREA
        {
            return Err(PdfError::TooLarge);
        }

        let dictionary_class = AnyClass::get(c"NSDictionary").ok_or(PdfError::Unavailable)?;
        let properties: Option<Retained<AnyObject>> =
            unsafe { msg_send![dictionary_class, dictionary] };
        let properties = properties.ok_or(PdfError::Unavailable)?;
        let png: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                &*bitmap,
                representationUsingType: NS_BITMAP_IMAGE_FILE_TYPE_PNG,
                properties: &*properties
            ]
        };
        let png = png.ok_or(PdfError::Invalid)?;

        Ok(PageImage {
            png: native_data(&png, MAX_FILE_BYTES as usize)?,
        })
    }

    fn select(
        &mut self,
        path: &Path,
        page_index: usize,
        start: [f64; 2],
        end: [f64; 2],
    ) -> Result<Selection, PdfError> {
        validate_normalized_point(start)?;
        validate_normalized_point(end)?;

        let document = self.document(path)?;
        if !document.info.can_copy {
            return Err(PdfError::CopyDenied);
        }
        let page = page_at(&document, page_index)?;
        let geometry = page_geometry(&page)?;
        let start = display_to_page(start, geometry)?;
        let end = display_to_page(end, geometry)?;
        let selection: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*page, selectionFromPoint: start, toPoint: end] };
        let Some(selection) = selection else {
            return Ok(Selection::default());
        };
        let text: Option<Retained<AnyObject>> = unsafe { msg_send![&*selection, string] };
        let text = text
            .as_deref()
            .map(native_string)
            .transpose()?
            .unwrap_or_default();
        let rects = selection_rects(&selection, &page, geometry)?;
        Ok(Selection { text, rects })
    }

    fn search(
        &mut self,
        path: &Path,
        query: &str,
        response: &smol::channel::Sender<Result<SearchResults, PdfError>>,
    ) -> Result<SearchResults, PdfError> {
        if query.is_empty() {
            return Err(PdfError::Invalid);
        }

        let document = self.document(path)?;
        if !document.info.can_copy {
            return Err(PdfError::CopyDenied);
        }
        let query = NSString::from_str(query);
        let options = NS_CASE_INSENSITIVE_SEARCH | NS_LITERAL_SEARCH;
        let mut previous: Option<Retained<AnyObject>> = None;
        let mut matches = Vec::new();
        let mut truncated = false;

        loop {
            if response.is_closed() {
                break;
            }
            let found: Option<Retained<AnyObject>> = unsafe {
                msg_send![
                    &*document.object,
                    findString: &*query,
                    fromSelection: previous.as_deref(),
                    withOptions: options
                ]
            };
            let Some(found) = found else {
                break;
            };
            if matches.len() == MAX_SEARCH_MATCHES {
                truncated = true;
                break;
            }

            let page = first_selection_page(&found)?;
            let page_index: usize = unsafe { msg_send![&*document.object, indexForPage: &*page] };
            if page_index >= document.info.pages.len() {
                return Err(PdfError::Invalid);
            }
            let geometry = page_geometry(&page)?;
            matches.push(SearchMatch {
                page: page_index,
                rects: selection_rects(&found, &page, geometry)?,
            });
            previous = Some(found);
        }

        Ok(SearchResults { matches, truncated })
    }

    fn document(&mut self, path: &Path) -> Result<NativeDocument, PdfError> {
        let (canonical, fingerprint) = validate_file(path)?;
        if let Some(index) = self
            .documents
            .iter()
            .position(|entry| entry.path == canonical && entry.fingerprint == fingerprint)
        {
            let entry = self.documents.remove(index).expect("cache index");
            let document = entry.document.clone();
            self.documents.push_front(entry);
            return Ok(document);
        }

        self.documents.retain(|entry| entry.path != canonical);
        let document = load_document(&canonical)?;
        self.documents.push_front(CacheEntry {
            path: canonical,
            fingerprint,
            document: document.clone(),
        });
        while self.documents.len() > DOCUMENT_CACHE_CAPACITY {
            self.documents.pop_back();
        }
        Ok(document)
    }
}

pub fn inspect(path: PathBuf) -> Response<PdfInfo> {
    enqueue(|response| Command::Inspect { path, response })
}

pub fn render_page(path: PathBuf, page: usize, pixel_width: u32) -> Response<PageImage> {
    enqueue(|response| Command::Render {
        path,
        page,
        pixel_width,
        response,
    })
}

pub fn select(path: PathBuf, page: usize, start: [f64; 2], end: [f64; 2]) -> Response<Selection> {
    enqueue(|response| Command::Select {
        path,
        page,
        start,
        end,
        response,
    })
}

pub fn search(path: PathBuf, query: String) -> Response<SearchResults> {
    enqueue(|response| Command::Search {
        path,
        query,
        response,
    })
}

fn enqueue<T: Send + 'static>(
    command: impl FnOnce(smol::channel::Sender<Result<T, PdfError>>) -> Command,
) -> Response<T> {
    let (response, receiver) = smol::channel::bounded(1);
    let Some(worker) = worker() else {
        let _ = response.try_send(Err(PdfError::Unavailable));
        return receiver;
    };
    let worker = worker.clone();
    let command = command(response);
    smol::spawn(async move {
        if command.is_closed() {
            return;
        }
        if let Err(error) = worker.send(command).await {
            error.0.fail_unavailable();
        }
    })
    .detach();
    receiver
}

fn worker() -> Option<&'static smol::channel::Sender<Command>> {
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = smol::channel::bounded(WORK_QUEUE_CAPACITY);
            std::thread::Builder::new()
                .name("rista-pdfkit".into())
                .spawn(move || worker_loop(receiver))
                .ok()
                .map(|_| sender)
        })
        .as_ref()
}

fn worker_loop(receiver: smol::channel::Receiver<Command>) {
    let mut worker = Worker::new();
    while let Ok(command) = receiver.recv_blocking() {
        if command.is_closed() {
            continue;
        }
        autoreleasepool(|_| command.run(&mut worker));
    }
}

fn validate_file(path: &Path) -> Result<(PathBuf, Fingerprint), PdfError> {
    let canonical = fs::canonicalize(path).map_err(|_| PdfError::Unavailable)?;
    let metadata = fs::metadata(&canonical).map_err(|_| PdfError::Unavailable)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(PdfError::Invalid);
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(PdfError::TooLarge);
    }
    File::open(&canonical).map_err(|_| PdfError::Unavailable)?;
    Ok((canonical, fingerprint(&metadata)))
}

fn fingerprint(metadata: &Metadata) -> Fingerprint {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| (duration.as_secs(), duration.subsec_nanos()));
    Fingerprint {
        length: metadata.len(),
        modified,
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn load_document(path: &Path) -> Result<NativeDocument, PdfError> {
    let path = path.to_str().ok_or(PdfError::Invalid)?;
    let path = NSString::from_str(path);
    let url_class = AnyClass::get(c"NSURL").ok_or(PdfError::Unavailable)?;
    let url: Option<Retained<AnyObject>> = unsafe { msg_send![url_class, fileURLWithPath: &*path] };
    let url = url.ok_or(PdfError::Unavailable)?;

    let document_class = AnyClass::get(c"PDFDocument").ok_or(PdfError::Unavailable)?;
    let allocated: *mut AnyObject = unsafe { msg_send![document_class, alloc] };
    if allocated.is_null() {
        return Err(PdfError::Unavailable);
    }
    let initialized: *mut AnyObject = unsafe { msg_send![allocated, initWithURL: &*url] };
    let object = unsafe { Retained::from_raw(initialized) }.ok_or(PdfError::Invalid)?;
    let locked: bool = unsafe { msg_send![&*object, isLocked] };
    if locked {
        return Err(PdfError::Locked);
    }

    let page_count: usize = unsafe { msg_send![&*object, pageCount] };
    if page_count == 0 {
        return Err(PdfError::Invalid);
    }
    if page_count > MAX_PAGE_COUNT {
        return Err(PdfError::TooLarge);
    }
    let can_copy: bool = unsafe { msg_send![&*object, allowsCopying] };
    let mut pages = Vec::with_capacity(page_count);
    for page_index in 0..page_count {
        let page = page_object(&object, page_index).ok_or(PdfError::Invalid)?;
        let geometry = page_geometry(&page)?;
        let (width, height) = display_size(geometry)?;
        pages.push(PageSize { width, height });
    }

    Ok(NativeDocument {
        object,
        info: PdfInfo { pages, can_copy },
    })
}

fn page_at(document: &NativeDocument, page_index: usize) -> Result<Retained<AnyObject>, PdfError> {
    if page_index >= document.info.pages.len() {
        return Err(PdfError::PageMissing);
    }
    page_object(&document.object, page_index).ok_or(PdfError::Invalid)
}

fn page_object(document: &AnyObject, page_index: usize) -> Option<Retained<AnyObject>> {
    unsafe { msg_send![document, pageAtIndex: page_index] }
}

fn page_geometry(page: &AnyObject) -> Result<PageGeometry, PdfError> {
    let bounds: NSRect = unsafe { msg_send![page, boundsForBox: PDF_DISPLAY_BOX_CROP] };
    if !bounds.origin.x.is_finite()
        || !bounds.origin.y.is_finite()
        || !bounds.size.width.is_finite()
        || !bounds.size.height.is_finite()
        || bounds.size.width <= 0.0
        || bounds.size.height <= 0.0
    {
        return Err(PdfError::Invalid);
    }
    let rotation: isize = unsafe { msg_send![page, rotation] };
    let rotation = rotation.rem_euclid(360);
    let rotation = match rotation {
        0 | 90 | 180 | 270 => rotation as u16,
        _ => return Err(PdfError::Invalid),
    };
    Ok(PageGeometry { bounds, rotation })
}

fn display_size(geometry: PageGeometry) -> Result<(f64, f64), PdfError> {
    let size = geometry.bounds.size;
    let (width, height) = match geometry.rotation {
        0 | 180 => (size.width, size.height),
        90 | 270 => (size.height, size.width),
        _ => return Err(PdfError::Invalid),
    };
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(PdfError::Invalid);
    }
    let aspect_ratio = width / height;
    if !aspect_ratio.is_finite() || !(0.001..=1000.0).contains(&aspect_ratio) {
        return Err(PdfError::TooLarge);
    }
    Ok((width, height))
}

fn bounded_image_size(
    requested_width: f64,
    page_width: f64,
    page_height: f64,
) -> Result<(f64, f64), PdfError> {
    if !requested_width.is_finite()
        || requested_width <= 0.0
        || !page_width.is_finite()
        || !page_height.is_finite()
        || page_width <= 0.0
        || page_height <= 0.0
    {
        return Err(PdfError::Invalid);
    }
    let requested_height = requested_width * page_height / page_width;
    if !requested_height.is_finite() || requested_height <= 0.0 {
        return Err(PdfError::Invalid);
    }
    let requested_area = requested_width * requested_height;
    if !requested_area.is_finite() || requested_area <= 0.0 {
        return Err(PdfError::Invalid);
    }
    let side_scale = MAX_IMAGE_SIDE / requested_width.max(requested_height);
    let area_scale = (MAX_IMAGE_AREA / requested_area).sqrt();
    let scale = 1.0_f64.min(side_scale).min(area_scale);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PdfError::Invalid);
    }
    Ok((
        (requested_width * scale).floor().max(1.0),
        (requested_height * scale).floor().max(1.0),
    ))
}

fn validate_normalized_point(point: [f64; 2]) -> Result<(), PdfError> {
    if point
        .into_iter()
        .all(|coordinate| coordinate.is_finite() && (0.0..=1.0).contains(&coordinate))
    {
        Ok(())
    } else {
        Err(PdfError::Invalid)
    }
}

fn display_to_page(point: [f64; 2], geometry: PageGeometry) -> Result<NSPoint, PdfError> {
    let [display_x, display_y] = point;
    let (u, v) = match geometry.rotation {
        0 => (display_x, 1.0 - display_y),
        90 => (display_y, display_x),
        180 => (1.0 - display_x, display_y),
        270 => (1.0 - display_y, 1.0 - display_x),
        _ => return Err(PdfError::Invalid),
    };
    let x = geometry.bounds.origin.x + u * geometry.bounds.size.width;
    let y = geometry.bounds.origin.y + v * geometry.bounds.size.height;
    if x.is_finite() && y.is_finite() {
        Ok(NSPoint::new(x, y))
    } else {
        Err(PdfError::Invalid)
    }
}

fn page_to_display(point: NSPoint, geometry: PageGeometry) -> Result<[f64; 2], PdfError> {
    let u = (point.x - geometry.bounds.origin.x) / geometry.bounds.size.width;
    let v = (point.y - geometry.bounds.origin.y) / geometry.bounds.size.height;
    if !u.is_finite() || !v.is_finite() {
        return Err(PdfError::Invalid);
    }
    Ok(match geometry.rotation {
        0 => [u, 1.0 - v],
        90 => [v, u],
        180 => [1.0 - u, v],
        270 => [1.0 - v, 1.0 - u],
        _ => return Err(PdfError::Invalid),
    })
}

fn normalize_rect(rect: NSRect, geometry: PageGeometry) -> Result<PageRect, PdfError> {
    let x0 = rect.origin.x;
    let x1 = rect.origin.x + rect.size.width;
    let y0 = rect.origin.y;
    let y1 = rect.origin.y + rect.size.height;
    if !x0.is_finite() || !x1.is_finite() || !y0.is_finite() || !y1.is_finite() {
        return Err(PdfError::Invalid);
    }
    let corners = [
        page_to_display(NSPoint::new(x0, y0), geometry)?,
        page_to_display(NSPoint::new(x0, y1), geometry)?,
        page_to_display(NSPoint::new(x1, y0), geometry)?,
        page_to_display(NSPoint::new(x1, y1), geometry)?,
    ];
    let min_x = corners
        .iter()
        .map(|point| point[0])
        .fold(f64::INFINITY, f64::min)
        .clamp(0.0, 1.0);
    let max_x = corners
        .iter()
        .map(|point| point[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .clamp(0.0, 1.0);
    let min_y = corners
        .iter()
        .map(|point| point[1])
        .fold(f64::INFINITY, f64::min)
        .clamp(0.0, 1.0);
    let max_y = corners
        .iter()
        .map(|point| point[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .clamp(0.0, 1.0);
    Ok(PageRect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

fn selection_rects(
    selection: &AnyObject,
    page: &AnyObject,
    geometry: PageGeometry,
) -> Result<Vec<PageRect>, PdfError> {
    let lines: Option<Retained<AnyObject>> = unsafe { msg_send![selection, selectionsByLine] };
    let Some(lines) = lines else {
        return Ok(Vec::new());
    };
    let count: usize = unsafe { msg_send![&*lines, count] };
    let mut rects = Vec::with_capacity(count);
    for index in 0..count {
        let line: Option<Retained<AnyObject>> = unsafe { msg_send![&*lines, objectAtIndex: index] };
        let line = line.ok_or(PdfError::Invalid)?;
        let rect: NSRect = unsafe { msg_send![&*line, boundsForPage: page] };
        let rect = normalize_rect(rect, geometry)?;
        if rect.width > 0.0 && rect.height > 0.0 {
            rects.push(rect);
        }
    }
    Ok(rects)
}

fn first_selection_page(selection: &AnyObject) -> Result<Retained<AnyObject>, PdfError> {
    let pages: Option<Retained<AnyObject>> = unsafe { msg_send![selection, pages] };
    let pages = pages.ok_or(PdfError::Invalid)?;
    let count: usize = unsafe { msg_send![&*pages, count] };
    if count == 0 {
        return Err(PdfError::Invalid);
    }
    let page: Option<Retained<AnyObject>> = unsafe { msg_send![&*pages, objectAtIndex: 0usize] };
    page.ok_or(PdfError::Invalid)
}

fn native_string(string: &AnyObject) -> Result<String, PdfError> {
    let data: Option<Retained<AnyObject>> =
        unsafe { msg_send![string, dataUsingEncoding: NS_UTF8_STRING_ENCODING] };
    let data = data.ok_or(PdfError::Invalid)?;
    let length: usize = unsafe { msg_send![&*data, length] };
    if length == 0 {
        return Ok(String::new());
    }
    String::from_utf8(native_data(&data, MAX_FILE_BYTES as usize)?).map_err(|_| PdfError::Invalid)
}

fn native_data(data: &AnyObject, maximum: usize) -> Result<Vec<u8>, PdfError> {
    let length: usize = unsafe { msg_send![data, length] };
    if length == 0 {
        return Err(PdfError::Invalid);
    }
    if length > maximum {
        return Err(PdfError::TooLarge);
    }
    let bytes: *const c_void = unsafe { msg_send![data, bytes] };
    if bytes.is_null() {
        return Err(PdfError::Invalid);
    }
    Ok(unsafe { slice::from_raw_parts(bytes.cast::<u8>(), length) }.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

    struct TestFile(PathBuf);

    impl TestFile {
        fn new(bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rista-pdf-{}-{}.pdf",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = File::create(&path).expect("create PDF fixture");
            file.write_all(bytes).expect("write PDF fixture");
            Self(path)
        }

        fn sparse(length: u64) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rista-pdf-{}-{}.pdf",
                std::process::id(),
                NEXT_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            File::create(&path)
                .expect("create sparse PDF fixture")
                .set_len(length)
                .expect("size sparse PDF fixture");
            Self(path)
        }
    }

    impl Drop for TestFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn receive<T>(response: Response<T>) -> Result<T, PdfError> {
        smol::block_on(response.recv()).expect("PDF worker response")
    }

    fn minimal_pdf() -> Vec<u8> {
        pdf_with_stream(b"BT /F1 24 Tf 50 300 Td (Hello PDF hello) Tj ET\n")
    }

    fn pdf_with_stream(stream: &[u8]) -> Vec<u8> {
        pdf_with_page(stream, 280, 0)
    }

    fn pdf_with_crop(stream: &[u8], crop_width: usize) -> Vec<u8> {
        pdf_with_page(stream, crop_width, 0)
    }

    fn pdf_with_page(stream: &[u8], crop_width: usize, rotation: u16) -> Vec<u8> {
        let page = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [10 20 {} 420] /CropBox [20 30 {} 400] /Rotate {rotation} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
            crop_width + 30,
            crop_width + 20
        );
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
            page.as_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".as_slice(),
        ];
        let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = vec![0usize];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            writeln!(&mut pdf, "{} 0 obj", index + 1).expect("object header");
            pdf.extend_from_slice(object);
            pdf.extend_from_slice(b"\nendobj\n");
        }
        offsets.push(pdf.len());
        write!(
            &mut pdf,
            "5 0 obj\n<< /Length {} >>\nstream\n",
            stream.len()
        )
        .expect("stream header");
        pdf.extend_from_slice(stream);
        pdf.extend_from_slice(b"endstream\nendobj\n");
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            writeln!(&mut pdf, "{offset:010} 00000 n ").expect("xref entry");
        }
        write!(
            &mut pdf,
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .expect("trailer");
        pdf
    }

    fn geometry(rotation: u16) -> PageGeometry {
        PageGeometry {
            bounds: NSRect::new(NSPoint::new(40.0, 70.0), NSSize::new(240.0, 360.0)),
            rotation,
        }
    }

    #[test]
    fn crop_and_rotation_coordinates_roundtrip() {
        for rotation in [0, 90, 180, 270] {
            let geometry = geometry(rotation);
            let origin = NSPoint::new(geometry.bounds.origin.x, geometry.bounds.origin.y);
            let expected_origin = match rotation {
                0 => [0.0, 1.0],
                90 => [0.0, 0.0],
                180 => [1.0, 0.0],
                270 => [1.0, 1.0],
                _ => unreachable!(),
            };
            assert_eq!(
                page_to_display(origin, geometry).expect("origin"),
                expected_origin
            );
            for point in [[0.0, 0.0], [0.2, 0.8], [0.5, 0.5], [1.0, 1.0]] {
                let page = display_to_page(point, geometry).expect("display point");
                let actual = page_to_display(page, geometry).expect("page point");
                assert!((actual[0] - point[0]).abs() < f64::EPSILON * 8.0);
                assert!((actual[1] - point[1]).abs() < f64::EPSILON * 8.0);
            }
        }
    }

    #[test]
    fn render_dimensions_are_bounded() {
        let (width, height) = bounded_image_size(8000.0, 1.0, 1.0).expect("bounded size");
        assert!(width <= MAX_IMAGE_SIDE);
        assert!(height <= MAX_IMAGE_SIDE);
        assert!(width * height <= MAX_IMAGE_AREA);

        assert!(matches!(
            bounded_image_size(0.0, 100.0, 100.0),
            Err(PdfError::Invalid)
        ));
    }

    #[test]
    fn display_size_rejects_extreme_aspect_ratios() {
        for size in [
            NSSize::new(1001.0, 1.0),
            NSSize::new(f64::MAX, f64::MIN_POSITIVE),
        ] {
            let geometry = PageGeometry {
                bounds: NSRect::new(NSPoint::new(0.0, 0.0), size),
                rotation: 0,
            };
            assert!(matches!(display_size(geometry), Err(PdfError::TooLarge)));
        }
    }

    #[test]
    fn native_pdf_inspect_render_select_and_search() {
        let file = TestFile::new(&minimal_pdf());
        let info = receive(inspect(file.0.clone())).expect("inspect PDF");
        assert_eq!(
            info.pages,
            vec![PageSize {
                width: 280.0,
                height: 370.0
            }]
        );
        assert!(info.can_copy);

        let rotated = TestFile::new(&pdf_with_page(
            b"BT /F1 24 Tf 50 300 Td (Hello) Tj ET\n",
            280,
            90,
        ));
        let rotated_info = receive(inspect(rotated.0.clone())).expect("inspect rotated PDF");
        assert_eq!(
            rotated_info.pages,
            vec![PageSize {
                width: 370.0,
                height: 280.0
            }]
        );

        let image = receive(render_page(file.0.clone(), 0, 320)).expect("render PDF");
        assert!(image.png.starts_with(b"\x89PNG\r\n\x1a\n"));

        let results = receive(search(file.0.clone(), "hello".into())).expect("search PDF");
        assert_eq!(results.matches.len(), 2);
        assert!(!results.truncated);
        assert!(results
            .matches
            .iter()
            .all(|found| found.page == 0 && !found.rects.is_empty()));

        let selection =
            receive(select(file.0.clone(), 0, [0.05, 0.15], [0.95, 0.45])).expect("select PDF");
        assert!(selection.text.contains("Hello PDF hello"));
        assert!(!selection.rects.is_empty());
    }

    #[test]
    fn inspect_requests_over_queue_capacity_all_complete() {
        let file = TestFile::new(&minimal_pdf());
        let responses = (0..WORK_QUEUE_CAPACITY * 2)
            .map(|_| inspect(file.0.clone()))
            .collect::<Vec<_>>();

        assert!(responses.len() > WORK_QUEUE_CAPACITY);
        for response in responses {
            let info = receive(response).expect("inspect queued PDF");
            assert_eq!(info.pages.len(), 1);
        }
    }

    #[test]
    fn search_caps_results_and_reports_truncation() {
        let content = format!(
            "BT /F1 12 Tf 50 300 Td ({}) Tj ET\n",
            vec!["hit"; MAX_SEARCH_MATCHES + 1].join(" ")
        );
        let file = TestFile::new(&pdf_with_crop(content.as_bytes(), 100_000));
        let results = receive(search(file.0.clone(), "hit".into())).expect("search PDF");
        assert_eq!(results.matches.len(), MAX_SEARCH_MATCHES);
        assert!(results.truncated);
    }

    #[test]
    fn invalid_empty_missing_and_page_errors_are_structured() {
        let missing = std::env::temp_dir().join(format!(
            "rista-missing-pdf-{}-{}",
            std::process::id(),
            NEXT_FILE.fetch_add(1, Ordering::Relaxed)
        ));
        assert!(matches!(
            receive(inspect(missing)),
            Err(PdfError::Unavailable)
        ));

        let empty = TestFile::new(&[]);
        assert!(matches!(
            receive(inspect(empty.0.clone())),
            Err(PdfError::Invalid)
        ));

        let malformed = TestFile::new(b"not a PDF");
        assert!(matches!(
            receive(inspect(malformed.0.clone())),
            Err(PdfError::Invalid)
        ));

        let oversized = TestFile::sparse(MAX_FILE_BYTES + 1);
        assert!(matches!(
            receive(inspect(oversized.0.clone())),
            Err(PdfError::TooLarge)
        ));

        let file = TestFile::new(&minimal_pdf());
        assert!(matches!(
            receive(render_page(file.0.clone(), 0, 0)),
            Err(PdfError::Invalid)
        ));
        assert!(matches!(
            receive(render_page(file.0.clone(), 1, 320)),
            Err(PdfError::PageMissing)
        ));
        assert!(matches!(
            receive(select(file.0.clone(), 0, [-0.1, 0.0], [1.0, 1.0])),
            Err(PdfError::Invalid)
        ));
    }
}
