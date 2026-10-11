use objc2::{
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject},
    sel,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::{
    fs::{self, File},
    io::Read,
    marker::PhantomData,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PDF_BYTES: u64 = 128 * 1024 * 1024;
static EXPORT_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct ActiveExport;

impl ActiveExport {
    fn acquire() -> Result<Self, super::ExportError> {
        EXPORT_ACTIVE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Self)
            .map_err(|_| super::ExportError::Render)
    }
}

impl Drop for ActiveExport {
    fn drop(&mut self) {
        EXPORT_ACTIVE.store(false, Ordering::Release);
    }
}

struct TempPdf {
    directory: PathBuf,
    path: PathBuf,
}

impl TempPdf {
    fn create() -> Result<Self, super::ExportError> {
        use std::os::unix::fs::DirBuilderExt;

        for _ in 0..32 {
            let directory = std::env::temp_dir().join(format!(
                "rista-pdf-export-{}-{}",
                std::process::id(),
                NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&directory) {
                Ok(()) => {
                    return Ok(Self {
                        path: directory.join("render.pdf"),
                        directory,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(super::ExportError::Render),
            }
        }

        Err(super::ExportError::Render)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

pub struct ExportJob {
    web_view: Retained<AnyObject>,
    temporary: TempPdf,
    options: super::Options,
    started: Instant,
    active: Option<ActiveExport>,
    finished: bool,
    _main_thread_only: PhantomData<Rc<()>>,
}

impl ExportJob {
    pub fn start(html: String, options: super::Options) -> Result<Self, super::ExportError> {
        if !on_main_thread() {
            return Err(super::ExportError::Render);
        }
        if !options.valid() {
            return Err(super::ExportError::Invalid);
        }

        let active = ActiveExport::acquire()?;
        let temporary = TempPdf::create()?;
        let started = Instant::now();
        let web_view = autoreleasepool(|_| create_web_view(&html, options))?;

        Ok(Self {
            web_view,
            temporary,
            options,
            started,
            active: Some(active),
            finished: false,
            _main_thread_only: PhantomData,
        })
    }

    pub fn poll(&mut self) -> Option<Result<Vec<u8>, super::ExportError>> {
        if self.finished {
            return None;
        }
        if !on_main_thread() {
            return Some(self.finish(Err(super::ExportError::Render)));
        }
        if self.started.elapsed() >= LOAD_TIMEOUT {
            unsafe {
                let _: () = msg_send![&*self.web_view, stopLoading];
            }
            return Some(self.finish(Err(super::ExportError::Render)));
        }

        let is_loading: bool = unsafe { msg_send![&*self.web_view, isLoading] };
        let progress: f64 = unsafe { msg_send![&*self.web_view, estimatedProgress] };
        if is_loading || !progress.is_finite() || progress < 1.0 {
            return None;
        }

        let result = autoreleasepool(|_| self.print_to_temporary_pdf());
        Some(self.finish(result))
    }

    fn finish(
        &mut self,
        result: Result<Vec<u8>, super::ExportError>,
    ) -> Result<Vec<u8>, super::ExportError> {
        self.finished = true;
        self.active.take();
        result
    }

    fn print_to_temporary_pdf(&self) -> Result<Vec<u8>, super::ExportError> {
        let print_info = print_info(&self.temporary.path, self.options)?;
        let operation: Option<Retained<AnyObject>> = unsafe {
            msg_send![
                &*self.web_view,
                printOperationWithPrintInfo: &*print_info
            ]
        };
        let operation = operation.ok_or(super::ExportError::Render)?;

        unsafe {
            let _: () = msg_send![&*operation, setShowsPrintPanel: false];
            let _: () = msg_send![&*operation, setShowsProgressPanel: false];
            let _: () = msg_send![&*operation, setCanSpawnSeparateThread: false];
        }
        let succeeded: bool = unsafe { msg_send![&*operation, runOperation] };
        if !succeeded || self.started.elapsed() >= LOAD_TIMEOUT {
            return Err(super::ExportError::Render);
        }

        read_pdf(&self.temporary.path)
    }
}

impl Drop for ExportJob {
    fn drop(&mut self) {
        if !self.finished && on_main_thread() {
            unsafe {
                let _: () = msg_send![&*self.web_view, stopLoading];
            }
        }
    }
}

fn on_main_thread() -> bool {
    let Some(thread_class) = AnyClass::get(c"NSThread") else {
        return false;
    };
    unsafe { msg_send![thread_class, isMainThread] }
}

fn create_web_view(
    html: &str,
    options: super::Options,
) -> Result<Retained<AnyObject>, super::ExportError> {
    let configuration_class =
        AnyClass::get(c"WKWebViewConfiguration").ok_or(super::ExportError::Render)?;
    let allocated: *mut AnyObject = unsafe { msg_send![configuration_class, alloc] };
    if allocated.is_null() {
        return Err(super::ExportError::Render);
    }
    let initialized: *mut AnyObject = unsafe { msg_send![allocated, init] };
    let configuration =
        unsafe { Retained::from_raw(initialized) }.ok_or(super::ExportError::Render)?;

    let data_store_class =
        AnyClass::get(c"WKWebsiteDataStore").ok_or(super::ExportError::Render)?;
    let data_store: Option<Retained<AnyObject>> =
        unsafe { msg_send![data_store_class, nonPersistentDataStore] };
    let data_store = data_store.ok_or(super::ExportError::Render)?;
    unsafe {
        let _: () = msg_send![&*configuration, setWebsiteDataStore: &*data_store];
    }

    let webpage_preferences: Option<Retained<AnyObject>> =
        unsafe { msg_send![&*configuration, defaultWebpagePreferences] };
    let webpage_preferences = webpage_preferences.ok_or(super::ExportError::Render)?;
    unsafe {
        let _: () = msg_send![&*webpage_preferences, setAllowsContentJavaScript: false];
        let _: () = msg_send![
            &*configuration,
            setDefaultWebpagePreferences: &*webpage_preferences
        ];
    }

    let preferences: Option<Retained<AnyObject>> =
        unsafe { msg_send![&*configuration, preferences] };
    let preferences = preferences.ok_or(super::ExportError::Render)?;
    unsafe {
        let _: () = msg_send![&*preferences, setJavaScriptCanOpenWindowsAutomatically: false];
        if msg_send![&*preferences, respondsToSelector: sel!(setShouldPrintBackgrounds:)] {
            let _: () = msg_send![&*preferences, setShouldPrintBackgrounds: true];
        }
    }

    let web_view_class = AnyClass::get(c"WKWebView").ok_or(super::ExportError::Render)?;
    let allocated: *mut AnyObject = unsafe { msg_send![web_view_class, alloc] };
    if allocated.is_null() {
        return Err(super::ExportError::Render);
    }
    let (paper_width, paper_height) = options.paper.points();
    let margin = options.margin_mm * 72.0 / 25.4;
    let frame = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(
            (paper_width - margin * 2.0).max(1.0),
            (paper_height - margin * 2.0).max(1.0),
        ),
    );
    let initialized: *mut AnyObject = unsafe {
        msg_send![
            allocated,
            initWithFrame: frame,
            configuration: &*configuration
        ]
    };
    let web_view = unsafe { Retained::from_raw(initialized) }.ok_or(super::ExportError::Render)?;

    let html = NSString::from_str(html);
    let navigation: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            &*web_view,
            loadHTMLString: &*html,
            baseURL: std::ptr::null::<AnyObject>()
        ]
    };
    navigation.ok_or(super::ExportError::Render)?;
    Ok(web_view)
}

fn print_info(
    path: &Path,
    options: super::Options,
) -> Result<Retained<AnyObject>, super::ExportError> {
    let class = AnyClass::get(c"NSPrintInfo").ok_or(super::ExportError::Render)?;
    let shared: Option<Retained<AnyObject>> = unsafe { msg_send![class, sharedPrintInfo] };
    let shared = shared.ok_or(super::ExportError::Render)?;
    let print_info: Option<Retained<AnyObject>> = unsafe { msg_send![&*shared, copy] };
    let print_info = print_info.ok_or(super::ExportError::Render)?;

    let (paper_width, paper_height) = options.paper.points();
    let margin = options.margin_mm * 72.0 / 25.4;
    unsafe {
        let _: () = msg_send![
            &*print_info,
            setPaperSize: NSSize::new(paper_width, paper_height)
        ];
        let _: () = msg_send![&*print_info, setLeftMargin: margin];
        let _: () = msg_send![&*print_info, setRightMargin: margin];
        let _: () = msg_send![&*print_info, setTopMargin: margin];
        let _: () = msg_send![&*print_info, setBottomMargin: margin];
        let _: () = msg_send![&*print_info, setHorizontallyCentered: false];
        let _: () = msg_send![&*print_info, setVerticallyCentered: false];
        let _: () = msg_send![&*print_info, setHorizontalPagination: 1_usize];
        let _: () = msg_send![&*print_info, setVerticalPagination: 0_usize];
    }

    let saving = NSString::from_str("NSPrintSaveJob");
    unsafe {
        let _: () = msg_send![&*print_info, setJobDisposition: &*saving];
    }
    let dictionary: Option<Retained<AnyObject>> = unsafe { msg_send![&*print_info, dictionary] };
    let dictionary = dictionary.ok_or(super::ExportError::Render)?;
    let saving_url = file_url(path)?;
    let saving_key = NSString::from_str("NSPrintJobSavingURL");
    unsafe {
        let _: () = msg_send![&*dictionary, setObject: &*saving_url, forKey: &*saving_key];
    }

    let number_class = AnyClass::get(c"NSNumber").ok_or(super::ExportError::Render)?;
    let no_header_footer: Option<Retained<AnyObject>> =
        unsafe { msg_send![number_class, numberWithBool: false] };
    let no_header_footer = no_header_footer.ok_or(super::ExportError::Render)?;
    let header_footer_key = NSString::from_str("NSPrintHeaderAndFooter");
    unsafe {
        let _: () =
            msg_send![&*dictionary, setObject: &*no_header_footer, forKey: &*header_footer_key];
    }

    Ok(print_info)
}

fn file_url(path: &Path) -> Result<Retained<AnyObject>, super::ExportError> {
    let path = path.to_str().ok_or(super::ExportError::Render)?;
    let path = NSString::from_str(path);
    let class = AnyClass::get(c"NSURL").ok_or(super::ExportError::Render)?;
    let url: Option<Retained<AnyObject>> = unsafe { msg_send![class, fileURLWithPath: &*path] };
    url.ok_or(super::ExportError::Render)
}

fn read_pdf(path: &Path) -> Result<Vec<u8>, super::ExportError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| super::ExportError::Render)?;
    if !metadata.file_type().is_file() {
        return Err(super::ExportError::Render);
    }
    if metadata.len() > MAX_PDF_BYTES {
        return Err(super::ExportError::TooLarge);
    }

    let file = File::open(path).map_err(|_| super::ExportError::Render)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PDF_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| super::ExportError::Render)?;
    if bytes.len() as u64 > MAX_PDF_BYTES {
        return Err(super::ExportError::TooLarge);
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(super::ExportError::Render);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn temporary_pdf_directory_is_private_and_removed_on_drop() {
        let (directory, path) = {
            let temporary = TempPdf::create().unwrap();
            let metadata = fs::metadata(&temporary.directory).unwrap();
            assert_eq!(metadata.mode() & 0o077, 0);
            fs::write(&temporary.path, b"private test output").unwrap();
            (temporary.directory.clone(), temporary.path.clone())
        };

        assert!(!path.exists());
        assert!(!directory.exists());
    }
}
