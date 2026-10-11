use markdown::mdast::Node;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::fs::{self, OpenOptions};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

pub(crate) type ComposerTransactionChange = (PathBuf, Option<Vec<u8>>, Option<Vec<u8>>);

#[path = "note_composer_links.rs"]
mod links;
pub use links::{extract_reference_definitions, rebase_markdown_links, LinkSnapshot};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractBehavior {
    #[default]
    Link,
    Embed,
    Nothing,
}

impl ExtractBehavior {
    pub fn replacement(self, target: &str) -> String {
        match self {
            Self::Link => format!("[[{target}]]\n"),
            Self::Embed => format!("![[{target}]]\n"),
            Self::Nothing => String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MergeOrder {
    #[default]
    Append,
    Prepend,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposeError {
    InvalidMarkdown,
    InvalidFrontmatter,
    PropertyConflict,
    AnchorConflict,
    AmbiguousLink,
    InvalidPath,
    FileTooLarge,
    UnreadableNote,
    DestinationExists,
    ExternalChange,
    TransactionFailed,
}

impl ComposeError {
    pub fn message(&self, norwegian: bool) -> &'static str {
        match (self, norwegian) {
            (Self::InvalidMarkdown, false) => "The note contains invalid Markdown.",
            (Self::InvalidMarkdown, true) => "Notatet inneholder ugyldig Markdown.",
            (Self::InvalidFrontmatter, false) => "The note properties are invalid.",
            (Self::InvalidFrontmatter, true) => "Notatets egenskaper er ugyldige.",
            (Self::PropertyConflict, false) => "The notes contain conflicting properties.",
            (Self::PropertyConflict, true) => "Notatene har motstridende egenskaper.",
            (Self::AnchorConflict, false) => "The notes contain duplicate headings or anchors.",
            (Self::AnchorConflict, true) => "Notatene har dupliserte overskrifter eller ankere.",
            (Self::AmbiguousLink, false) => "A link is ambiguous and cannot be updated safely.",
            (Self::AmbiguousLink, true) => "En lenke er tvetydig og kan ikke oppdateres trygt.",
            (Self::InvalidPath, false) => "The path is outside the vault or unsupported.",
            (Self::InvalidPath, true) => "Stien er utenfor hvelvet eller støttes ikke.",
            (Self::FileTooLarge, false) => "A note is too large to process safely.",
            (Self::FileTooLarge, true) => "Et notat er for stort til trygg behandling.",
            (Self::UnreadableNote, false) => "A linked note could not be read.",
            (Self::UnreadableNote, true) => "Et lenket notat kunne ikke leses.",
            (Self::DestinationExists, false) => "That destination already exists.",
            (Self::DestinationExists, true) => "Målfilen finnes allerede.",
            (Self::ExternalChange, false) => "A file changed during the operation.",
            (Self::ExternalChange, true) => "En fil ble endret under operasjonen.",
            (Self::TransactionFailed, false) => "The note operation could not be completed safely.",
            (Self::TransactionFailed, true) => "Notatoperasjonen kunne ikke fullføres trygt.",
        }
    }
}

pub const MAX_NOTE_BYTES: usize = 2 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const COMPOSER_DIRECTORY: &str = ".rista/composer";

#[derive(Clone)]
struct TransactionChange {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
struct RecoveryManifest {
    committed: bool,
    files: Vec<RecoveryFile>,
}

#[derive(Serialize, Deserialize)]
struct RecoveryFile {
    path: String,
    before: Option<String>,
    after: Option<String>,
}

fn checked_path(root: &Path, path: &Path) -> Result<PathBuf, ComposeError> {
    let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
    let path = if path.exists() {
        path.canonicalize().map_err(|_| ComposeError::InvalidPath)?
    } else {
        let parent = path.parent().ok_or(ComposeError::InvalidPath)?;
        let parent = parent
            .canonicalize()
            .map_err(|_| ComposeError::InvalidPath)?;
        parent.join(path.file_name().ok_or(ComposeError::InvalidPath)?)
    };
    path.starts_with(&root)
        .then_some(path)
        .ok_or(ComposeError::InvalidPath)
}

fn read_bounded(path: &Path, total: &mut usize) -> Result<Vec<u8>, ComposeError> {
    if fs::metadata(path)
        .map_err(|_| ComposeError::UnreadableNote)?
        .len()
        > MAX_NOTE_BYTES as u64
    {
        return Err(ComposeError::FileTooLarge);
    }
    let bytes = fs::read(path).map_err(|_| ComposeError::UnreadableNote)?;
    if bytes.len() > MAX_NOTE_BYTES || *total > MAX_TOTAL_BYTES.saturating_sub(bytes.len()) {
        return Err(ComposeError::FileTooLarge);
    }
    *total += bytes.len();
    Ok(bytes)
}

fn guarded_write(path: &Path, before: Option<&[u8]>, after: &[u8]) -> Result<(), ComposeError> {
    match before {
        Some(before) => {
            if fs::metadata(path).map(|metadata| metadata.len()).ok() != Some(before.len() as u64)
                || fs::read(path).ok().as_deref() != Some(before)
            {
                return Err(ComposeError::ExternalChange);
            }
            let temp = path.with_extension(format!(
                "rista-compose-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ));
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|_| ComposeError::TransactionFailed)?;
            use std::io::Write;
            file.write_all(after)
                .and_then(|_| file.sync_all())
                .map_err(|_| ComposeError::TransactionFailed)?;
            fs::rename(temp, path).map_err(|_| ComposeError::TransactionFailed)?;
        }
        None => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|err| {
                    if err.kind() == io::ErrorKind::AlreadyExists {
                        ComposeError::DestinationExists
                    } else {
                        ComposeError::TransactionFailed
                    }
                })?;
            use std::io::Write;
            file.write_all(after)
                .and_then(|_| file.sync_all())
                .map_err(|_| ComposeError::TransactionFailed)?;
        }
    }
    if let Some(parent) = path.parent() {
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn guarded_change(
    path: &Path,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<(), ComposeError> {
    match before {
        Some(before) => {
            if fs::metadata(path).map(|metadata| metadata.len()).ok() != Some(before.len() as u64)
                || fs::read(path).ok().as_deref() != Some(before)
            {
                return Err(ComposeError::ExternalChange);
            }
        }
        None if path.exists() => return Err(ComposeError::ExternalChange),
        None => {}
    }
    match after {
        Some(after) => guarded_write(path, before, after),
        None => {
            if before.is_none() {
                return Err(ComposeError::InvalidPath);
            }
            fs::remove_file(path).map_err(|_| ComposeError::TransactionFailed)?;
            if let Some(parent) = path.parent() {
                if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
                    let _ = directory.sync_all();
                }
            }
            Ok(())
        }
    }
}

fn read_optional_bounded(path: &Path, total: &mut usize) -> Result<Option<Vec<u8>>, ComposeError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ComposeError::InvalidPath),
        Ok(_) => read_bounded(path, total).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ComposeError::UnreadableNote),
    }
}

fn write_staged(path: &Path, bytes: &[u8]) -> Result<(), ComposeError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| ComposeError::TransactionFailed)?;
    use std::io::Write;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ComposeError::TransactionFailed)
}

fn composer_stage_root(root: &Path) -> Result<PathBuf, ComposeError> {
    let path = root.join(COMPOSER_DIRECTORY);
    fs::create_dir_all(&path).map_err(|_| ComposeError::TransactionFailed)?;
    let path = path
        .canonicalize()
        .map_err(|_| ComposeError::TransactionFailed)?;
    path.starts_with(root)
        .then_some(path)
        .ok_or(ComposeError::InvalidPath)
}

fn create_transaction_stage(
    root: &Path,
    changes: &[TransactionChange],
) -> Result<(PathBuf, RecoveryManifest), ComposeError> {
    let stage_root = composer_stage_root(root)?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    );
    let stage = stage_root.join(id);
    fs::create_dir(&stage).map_err(|_| ComposeError::TransactionFailed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700))
            .map_err(|_| ComposeError::TransactionFailed)?;
    }
    let mut files = Vec::with_capacity(changes.len());
    for (index, change) in changes.iter().enumerate() {
        let relative = change
            .path
            .strip_prefix(root)
            .map_err(|_| ComposeError::InvalidPath)?;
        let before_name = change.before.as_ref().map(|_| format!("before-{index}"));
        let after_name = format!("after-{index}");
        if let (Some(name), Some(bytes)) = (&before_name, &change.before) {
            write_staged(&stage.join(name), bytes)?;
        }
        if let Some(after) = &change.after {
            write_staged(&stage.join(&after_name), after)?;
        }
        files.push(RecoveryFile {
            path: relative.to_string_lossy().to_string(),
            before: before_name,
            after: change.after.as_ref().map(|_| after_name),
        });
    }
    let manifest = RecoveryManifest {
        committed: false,
        files,
    };
    let bytes = serde_json::to_vec(&manifest).map_err(|_| ComposeError::TransactionFailed)?;
    write_staged(&stage.join("manifest.tmp"), &bytes)?;
    fs::rename(stage.join("manifest.tmp"), stage.join("manifest.json"))
        .map_err(|_| ComposeError::TransactionFailed)?;
    if let Ok(directory) = OpenOptions::new().read(true).open(&stage) {
        let _ = directory.sync_all();
    }
    Ok((stage, manifest))
}

fn mark_transaction_committed(
    stage: &Path,
    _manifest: &RecoveryManifest,
) -> Result<(), ComposeError> {
    write_staged(&stage.join("committed"), b"")
}

fn restore_change(change: &TransactionChange) -> Result<(), ComposeError> {
    guarded_change(
        &change.path,
        change.after.as_deref(),
        change.before.as_deref(),
    )
}

fn apply_transaction(root: &Path, changes: &[TransactionChange]) -> Result<(), ComposeError> {
    let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
    let mut total = 0usize;
    let mut canonical_changes = Vec::with_capacity(changes.len());
    for change in changes {
        let path = checked_path(&root, &change.path)?;
        if change
            .after
            .as_ref()
            .is_some_and(|after| after.len() > MAX_NOTE_BYTES)
            || change
                .before
                .as_ref()
                .is_some_and(|before| before.len() > MAX_NOTE_BYTES)
        {
            return Err(ComposeError::FileTooLarge);
        }
        if let Some(after) = &change.after {
            total = total.saturating_add(after.len());
        }
        let actual = read_optional_bounded(&path, &mut total)?;
        if actual != change.before {
            return Err(match (&change.before, &actual) {
                (None, Some(_)) => ComposeError::DestinationExists,
                _ => ComposeError::ExternalChange,
            });
        }
        if total > MAX_TOTAL_BYTES {
            return Err(ComposeError::FileTooLarge);
        }
        canonical_changes.push(TransactionChange {
            path,
            before: change.before.clone(),
            after: change.after.clone(),
        });
    }
    let (stage, manifest) = create_transaction_stage(&root, &canonical_changes)?;
    let mut applied = Vec::new();
    for (index, change) in canonical_changes.iter().enumerate() {
        if let Err(error) = guarded_change(
            &change.path,
            change.before.as_deref(),
            change.after.as_deref(),
        ) {
            let mut rollback_ok = true;
            for index in applied.iter().rev() {
                rollback_ok &= restore_change(&canonical_changes[*index]).is_ok();
            }
            if rollback_ok {
                let _ = fs::remove_dir_all(&stage);
            }
            return Err(error);
        }
        applied.push(index);
    }
    if mark_transaction_committed(&stage, &manifest).is_err() {
        let mut rollback_ok = true;
        for index in applied.iter().rev() {
            rollback_ok &= restore_change(&canonical_changes[*index]).is_ok();
        }
        if rollback_ok {
            let _ = fs::remove_dir_all(&stage);
        }
        return Err(ComposeError::TransactionFailed);
    }
    let _ = fs::remove_dir_all(&stage);
    Ok(())
}

pub fn recover_pending_transactions(root: &Path) -> Result<usize, ComposeError> {
    let root = root.canonicalize().map_err(|_| ComposeError::InvalidPath)?;
    let stage_root = root.join(COMPOSER_DIRECTORY);
    if !stage_root.exists() {
        return Ok(0);
    }
    let stage_root = stage_root
        .canonicalize()
        .map_err(|_| ComposeError::InvalidPath)?;
    if !stage_root.starts_with(&root) {
        return Err(ComposeError::InvalidPath);
    }
    let mut recovered = 0;
    for entry in fs::read_dir(&stage_root).map_err(|_| ComposeError::TransactionFailed)? {
        let entry = entry.map_err(|_| ComposeError::TransactionFailed)?;
        if !entry
            .file_type()
            .map_err(|_| ComposeError::TransactionFailed)?
            .is_dir()
        {
            continue;
        }
        let stage = entry.path();
        let manifest_path = stage.join("manifest.json");
        let metadata = match fs::symlink_metadata(&manifest_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err(ComposeError::TransactionFailed),
        };
        if metadata.file_type().is_symlink() || metadata.len() > MAX_TOTAL_BYTES as u64 {
            return Err(ComposeError::InvalidPath);
        }
        let manifest: RecoveryManifest = serde_json::from_slice(
            &fs::read(&manifest_path).map_err(|_| ComposeError::TransactionFailed)?,
        )
        .map_err(|_| ComposeError::TransactionFailed)?;
        let marker = stage.join("committed");
        let committed = manifest.committed || marker.exists();
        if marker.exists()
            && fs::symlink_metadata(&marker)
                .map_err(|_| ComposeError::TransactionFailed)?
                .file_type()
                .is_symlink()
        {
            return Err(ComposeError::InvalidPath);
        }
        if !committed {
            let mut total = 0usize;
            for file in manifest.files.iter().rev() {
                let safe_stage_name = |name: &str| {
                    let mut components = Path::new(name).components();
                    matches!(components.next(), Some(std::path::Component::Normal(_)))
                        && components.next().is_none()
                };
                if file
                    .after
                    .as_deref()
                    .is_some_and(|name| !safe_stage_name(name))
                    || file
                        .before
                        .as_deref()
                        .is_some_and(|name| !safe_stage_name(name))
                {
                    return Err(ComposeError::InvalidPath);
                }
                let relative = PathBuf::from(&file.path);
                if relative.is_absolute()
                    || relative.components().any(|component| {
                        matches!(
                            component,
                            std::path::Component::ParentDir | std::path::Component::Prefix(_)
                        )
                    })
                {
                    return Err(ComposeError::InvalidPath);
                }
                let path = checked_path(&root, &root.join(relative))?;
                let after = file
                    .after
                    .as_deref()
                    .map(|name| {
                        let after_path = stage.join(name);
                        let after_meta = fs::symlink_metadata(&after_path)
                            .map_err(|_| ComposeError::TransactionFailed)?;
                        if after_meta.file_type().is_symlink()
                            || after_meta.len() > MAX_NOTE_BYTES as u64
                        {
                            return Err(ComposeError::InvalidPath);
                        }
                        read_bounded(&after_path, &mut total)
                    })
                    .transpose()?;
                match &file.before {
                    Some(name) => {
                        let before_path = stage.join(name);
                        let before_meta = fs::symlink_metadata(&before_path)
                            .map_err(|_| ComposeError::TransactionFailed)?;
                        if before_meta.file_type().is_symlink()
                            || before_meta.len() > MAX_NOTE_BYTES as u64
                        {
                            return Err(ComposeError::InvalidPath);
                        }
                        let before = read_bounded(&before_path, &mut total)?;
                        let actual = read_optional_bounded(&path, &mut total)?;
                        if actual == after {
                            guarded_change(&path, after.as_deref(), Some(&before))?;
                        } else if actual.as_deref() != Some(before.as_slice()) {
                            return Err(ComposeError::ExternalChange);
                        }
                    }
                    None => {
                        let actual = read_optional_bounded(&path, &mut total)?;
                        if actual == after {
                            guarded_change(&path, after.as_deref(), None)?;
                        } else if actual.is_some() {
                            return Err(ComposeError::ExternalChange);
                        }
                    }
                }
            }
        }
        fs::remove_dir_all(stage).map_err(|_| ComposeError::TransactionFailed)?;
        recovered += 1;
    }
    Ok(recovered)
}

/// Install an extracted note and the guarded source replacement as one
/// filesystem operation. The caller performs the matching editor replacement
/// only after this function succeeds.
pub fn extract_transaction(
    root: &Path,
    source_path: &Path,
    source_before: &[u8],
    source_after: &[u8],
    destination: &Path,
    payload: &[u8],
) -> Result<(), ComposeError> {
    apply_transaction(
        root,
        &[
            TransactionChange {
                path: destination.to_path_buf(),
                before: None,
                after: Some(payload.to_vec()),
            },
            TransactionChange {
                path: source_path.to_path_buf(),
                before: Some(source_before.to_vec()),
                after: Some(source_after.to_vec()),
            },
        ],
    )
}

pub(crate) fn merge_transaction_changes(
    root: &Path,
    changes: &[ComposerTransactionChange],
) -> Result<(), ComposeError> {
    apply_transaction(
        root,
        &changes
            .iter()
            .map(|(path, before, after)| TransactionChange {
                path: path.clone(),
                before: before.clone(),
                after: after.clone(),
            })
            .collect::<Vec<_>>(),
    )
}

pub fn parse(text: &str) -> Result<Node, ComposeError> {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    markdown::to_mdast(text, &options).map_err(|_| ComposeError::InvalidMarkdown)
}

pub fn heading_section(
    text: &str,
    caret: usize,
) -> Result<Option<(Range<usize>, String)>, ComposeError> {
    let root = parse(text)?;
    let headings: Vec<_> = root
        .children()
        .into_iter()
        .flatten()
        .filter_map(|node| {
            if let Node::Heading(heading) = node {
                Some((
                    node.position()?.start.offset,
                    heading.depth,
                    node.to_string(),
                ))
            } else {
                None
            }
        })
        .collect();
    let Some(index) = headings.iter().rposition(|(start, _, _)| *start <= caret) else {
        return Ok(None);
    };
    let (start, depth, title) = &headings[index];
    let end = headings[index + 1..]
        .iter()
        .find(|(_, next, _)| next <= depth)
        .map(|(start, _, _)| *start)
        .unwrap_or(text.len());
    Ok(Some((*start..end, title.clone())))
}

fn frontmatter(text: &str) -> Result<(Mapping, &str), ComposeError> {
    let Some(span) = crate::properties::frontmatter_span(text) else {
        if text
            .lines()
            .next()
            .is_some_and(|line| line.trim_end() == "---")
        {
            return Err(ComposeError::InvalidFrontmatter);
        }
        return Ok((Mapping::new(), text));
    };
    let block = &text[span.clone()];
    let mut lines: Vec<_> = block.lines().collect();
    lines.remove(0);
    lines.pop();
    let value: Value =
        serde_yaml::from_str(&lines.join("\n")).map_err(|_| ComposeError::InvalidFrontmatter)?;
    let mapping = match value {
        Value::Null => Mapping::new(),
        Value::Mapping(map) => map,
        _ => return Err(ComposeError::InvalidFrontmatter),
    };
    Ok((mapping, &text[span.end..]))
}

fn merge_value(target: &mut Value, source: Value) -> Result<(), ComposeError> {
    if *target == source {
        return Ok(());
    }
    match (target, source) {
        (Value::Mapping(target), Value::Mapping(source)) => merge_mapping(target, source),
        (Value::Sequence(target), Value::Sequence(source)) => {
            for item in source {
                if !target.contains(&item) {
                    target.push(item);
                }
            }
            Ok(())
        }
        _ => Err(ComposeError::PropertyConflict),
    }
}

fn merge_mapping(target: &mut Mapping, source: Mapping) -> Result<(), ComposeError> {
    for (key, value) in source {
        if let Some(existing) = target.get_mut(&key) {
            merge_value(existing, value)?;
        } else {
            target.insert(key, value);
        }
    }
    Ok(())
}

fn heading_keys(text: &str) -> Vec<String> {
    crate::preview::headings(text)
        .into_iter()
        .map(|(_, _, title)| title.nfc().collect::<String>().to_lowercase())
        .collect()
}

fn block_ids(text: &str) -> Result<Vec<String>, ComposeError> {
    let root = parse(text)?;
    let mut ids = Vec::new();
    fn collect(node: &Node, text: &str, ids: &mut Vec<String>) {
        if matches!(
            node,
            Node::Code(_) | Node::Html(_) | Node::Yaml(_) | Node::Toml(_)
        ) {
            return;
        }
        if matches!(
            node,
            Node::Paragraph(_) | Node::Heading(_) | Node::Blockquote(_) | Node::ListItem(_)
        ) {
            if let Some(position) = node.position() {
                let start = position.start.offset.min(text.len());
                let end = position.end.offset.min(text.len());
                if start <= end && text.is_char_boundary(start) && text.is_char_boundary(end) {
                    for line in text[start..end].lines() {
                        if let Some((_, id)) = line.trim().rsplit_once('^') {
                            let id = id.trim();
                            if !id.is_empty()
                                && id
                                    .chars()
                                    .all(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_')
                            {
                                ids.push(id.to_lowercase());
                            }
                        }
                    }
                }
            }
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, ids);
        }
    }
    collect(&root, text, &mut ids);
    Ok(ids)
}

fn reference_definitions(
    text: &str,
) -> Result<std::collections::HashMap<String, Vec<String>>, ComposeError> {
    let root = parse(text)?;
    let mut definitions = std::collections::HashMap::<String, Vec<String>>::new();
    fn collect(node: &Node, text: &str, out: &mut std::collections::HashMap<String, Vec<String>>) {
        match node {
            Node::Definition(definition) => {
                out.entry(definition.identifier.to_lowercase())
                    .or_default()
                    .push(definition.url.clone());
            }
            Node::FootnoteDefinition(definition) => {
                if let Some(position) = &definition.position {
                    out.entry(format!("footnote:{}", definition.identifier.to_lowercase()))
                        .or_default()
                        .push(text[position.start.offset..position.end.offset].to_string());
                }
            }
            _ => {}
        }
        for child in node.children().into_iter().flatten() {
            collect(child, text, out);
        }
    }
    collect(&root, text, &mut definitions);
    Ok(definitions)
}

fn validate_merge_anchors(target: &str, source: &str) -> Result<(), ComposeError> {
    let target_headings = heading_keys(target);
    if heading_keys(source)
        .iter()
        .any(|heading| target_headings.contains(heading))
    {
        return Err(ComposeError::AnchorConflict);
    }
    let target_blocks = block_ids(target)?;
    if block_ids(source)?
        .iter()
        .any(|id| target_blocks.contains(id))
    {
        return Err(ComposeError::AnchorConflict);
    }
    let target_definitions = reference_definitions(target)?;
    let source_definitions = reference_definitions(source)?;
    if target_definitions
        .iter()
        .chain(source_definitions.iter())
        .any(|(_, definitions)| definitions.windows(2).any(|pair| pair[0] != pair[1]))
        || source_definitions.iter().any(|(id, definitions)| {
            target_definitions.get(id).is_some_and(|existing| {
                definitions
                    .iter()
                    .any(|definition| existing.iter().any(|item| item != definition))
            })
        })
    {
        return Err(ComposeError::AnchorConflict);
    }
    Ok(())
}

fn normalize_list_property(mapping: &mut Mapping, name: &str) {
    let key = Value::String(name.to_string());
    match mapping.get(&key).cloned() {
        Some(Value::String(value)) => {
            mapping.insert(key, Value::Sequence(vec![Value::String(value)]));
        }
        Some(Value::Null) => {
            mapping.insert(key, Value::Sequence(Vec::new()));
        }
        _ => {}
    }
}

pub fn merge_text(target: &str, source: &str, order: MergeOrder) -> Result<String, ComposeError> {
    let (mut target_map, target_body) = frontmatter(target)?;
    let (mut source_map, source_body) = frontmatter(source)?;
    validate_merge_anchors(target_body, source_body)?;
    for property in ["tags", "aliases"] {
        normalize_list_property(&mut target_map, property);
        normalize_list_property(&mut source_map, property);
    }
    merge_mapping(&mut target_map, source_map)?;
    let (first, second) = match order {
        MergeOrder::Append => (target_body, source_body),
        MergeOrder::Prepend => (source_body, target_body),
    };
    let body = format!(
        "{}\n\n{}",
        first.trim_end_matches(['\r', '\n']),
        second.trim_start_matches(['\r', '\n'])
    );
    if target_map.is_empty() {
        return Ok(body);
    }
    let yaml = serde_yaml::to_string(&target_map).map_err(|_| ComposeError::InvalidFrontmatter)?;
    Ok(format!(
        "---\n{}---\n\n{}",
        yaml,
        body.trim_start_matches(['\r', '\n'])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_sections_include_children_but_ignore_fences() {
        let text =
            "Intro\n\n# Første\nBody\n\n## Child\n```md\n# Not a heading\n```\n\n# Next\nTail\n";
        assert_eq!(heading_section(text, 0).unwrap(), None);
        let (section, title) = heading_section(text, text.find("Body").unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(title, "Første");
        assert_eq!(
            &text[section],
            "# Første\nBody\n\n## Child\n```md\n# Not a heading\n```\n\n"
        );
        let setext = "Title\n=====\nBody\n\nOther\n-----\nText\n";
        assert_eq!(
            heading_section(setext, 13).unwrap().unwrap().0,
            0..setext.len()
        );
    }

    #[test]
    fn merge_preserves_properties_and_refuses_ambiguous_data() {
        let target = "---\ntags: [a, b]\nproject: {name: one}\n---\n# Target\nBody\n";
        let source = "---\ntags: [b, c]\nproject: {owner: Kari}\n---\n# Source\nMore\n";
        let merged = merge_text(target, source, MergeOrder::Append).unwrap();
        let (map, body) = frontmatter(&merged).unwrap();
        assert_eq!(
            map["tags"],
            serde_yaml::from_str::<Value>("[a, b, c]").unwrap()
        );
        assert_eq!(map["project"]["name"].as_str(), Some("one"));
        assert_eq!(map["project"]["owner"].as_str(), Some("Kari"));
        assert!(body.find("# Target").unwrap() < body.find("# Source").unwrap());
        let prepended = merge_text(target, source, MergeOrder::Prepend).unwrap();
        assert!(prepended.find("# Source").unwrap() < prepended.find("# Target").unwrap());
        assert_eq!(
            merge_text(target, "---\nproject: other\n---\n", MergeOrder::Append),
            Err(ComposeError::PropertyConflict)
        );
        assert_eq!(
            merge_text(target, "# Target\n", MergeOrder::Append),
            Err(ComposeError::AnchorConflict)
        );
        assert_eq!(
            merge_text(target, "---\na: [\n---\n", MergeOrder::Append),
            Err(ComposeError::InvalidFrontmatter)
        );
    }

    #[test]
    fn merge_unions_scalar_aliases_and_rejects_anchor_collisions() {
        let target = "---\naliases: Main\ntags: [a]\n---\n# Target\n\nText ^block-a\n";
        let source = "---\naliases: [Other, Main]\ntags: b\n---\n# Source\n\nText ^block-b\n";
        let merged = merge_text(target, source, MergeOrder::Append).unwrap();
        let (map, _) = frontmatter(&merged).unwrap();
        assert_eq!(
            map["aliases"],
            serde_yaml::from_str::<Value>("[Main, Other]").unwrap()
        );
        assert_eq!(
            map["tags"],
            serde_yaml::from_str::<Value>("[a, b]").unwrap()
        );

        assert_eq!(
            merge_text(target, "# Target\n", MergeOrder::Append),
            Err(ComposeError::AnchorConflict)
        );
        assert_eq!(
            merge_text(target, "Paragraph ^BLOCK-A\n", MergeOrder::Append),
            Err(ComposeError::AnchorConflict)
        );
        let links = "# Source\n\n[a][ref]\n\n[ref]: first.md\n";
        let conflicting_links = "# Target\n\n[b][ref]\n\n[ref]: second.md\n";
        assert_eq!(
            merge_text(conflicting_links, links, MergeOrder::Append),
            Err(ComposeError::AnchorConflict)
        );
    }

    #[test]
    fn extract_transaction_rejects_existing_destination_and_stale_source() {
        let root = std::env::temp_dir().join(format!(
            "rista-compose-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.md");
        let destination = root.join("new.md");
        fs::write(&source, "before").unwrap();
        fs::write(&destination, "existing").unwrap();
        assert_eq!(
            extract_transaction(&root, &source, b"before", b"after", &destination, b"body"),
            Err(ComposeError::DestinationExists)
        );
        fs::remove_file(&destination).unwrap();
        fs::write(&source, "external").unwrap();
        assert_eq!(
            extract_transaction(&root, &source, b"before", b"after", &destination, b"body"),
            Err(ComposeError::ExternalChange)
        );
        assert_eq!(fs::read_to_string(source).unwrap(), "external");
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn extract_transaction_commits_destination_and_source_together() {
        let root = std::env::temp_dir().join(format!(
            "rista-compose-success-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.md");
        let destination = root.join("new.md");
        fs::write(&source, "selected").unwrap();
        extract_transaction(
            &root,
            &source,
            b"selected",
            b"[[new]]",
            &destination,
            b"body",
        )
        .unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"[[new]]");
        assert_eq!(fs::read(&destination).unwrap(), b"body");
        assert_eq!(recover_pending_transactions(&root).unwrap(), 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recovery_rolls_back_uncommitted_create_and_replacement() {
        let root = std::env::temp_dir().join(format!(
            "rista-compose-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.md");
        let destination = root.join("new.md");
        fs::write(&source, b"after").unwrap();
        fs::write(&destination, b"created").unwrap();
        let stage = root.join(COMPOSER_DIRECTORY).join("interrupted");
        fs::create_dir_all(&stage).unwrap();
        write_staged(&stage.join("before-1"), b"before").unwrap();
        write_staged(&stage.join("after-1"), b"after").unwrap();
        write_staged(&stage.join("after-0"), b"created").unwrap();
        let manifest = RecoveryManifest {
            committed: false,
            files: vec![
                RecoveryFile {
                    path: "new.md".into(),
                    before: None,
                    after: Some("after-0".into()),
                },
                RecoveryFile {
                    path: "source.md".into(),
                    before: Some("before-1".into()),
                    after: Some("after-1".into()),
                },
            ],
        };
        write_staged(
            &stage.join("manifest.json"),
            &serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(recover_pending_transactions(&root).unwrap(), 1);
        assert_eq!(fs::read(source).unwrap(), b"before");
        assert!(!destination.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn companion_transaction_round_trips_and_rejects_external_changes() {
        let root = std::env::temp_dir().join(format!(
            "rista-compose-journal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source.md");
        let target = root.join("target.md");
        let backlink = root.join("backlink.md");
        let trashed = root.join(".rista/trash/source.md");
        let index = root.join(".rista/trash/index.json");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::write(&source, b"source-before").unwrap();
        fs::write(&target, b"target-before").unwrap();
        fs::write(&backlink, b"backlink-before").unwrap();
        let changes = vec![
            (
                target.clone(),
                Some(b"target-before".to_vec()),
                Some(b"target-after".to_vec()),
            ),
            (
                backlink.clone(),
                Some(b"backlink-before".to_vec()),
                Some(b"backlink-after".to_vec()),
            ),
            (source.clone(), Some(b"source-before".to_vec()), None),
            (trashed.clone(), None, Some(b"source-before".to_vec())),
            (index.clone(), None, Some(b"trash-index".to_vec())),
        ];
        merge_transaction_changes(&root, &changes).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"target-after");
        assert_eq!(fs::read(&backlink).unwrap(), b"backlink-after");
        assert_eq!(fs::read(&trashed).unwrap(), b"source-before");
        assert_eq!(fs::read(&index).unwrap(), b"trash-index");

        let undo = changes
            .iter()
            .map(|(path, before, after)| (path.clone(), after.clone(), before.clone()))
            .collect::<Vec<_>>();
        merge_transaction_changes(&root, &undo).unwrap();
        assert_eq!(fs::read(&source).unwrap(), b"source-before");
        assert_eq!(fs::read(&target).unwrap(), b"target-before");
        assert_eq!(fs::read(&backlink).unwrap(), b"backlink-before");
        assert!(!trashed.exists());
        assert!(!index.exists());

        merge_transaction_changes(&root, &changes).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&target).unwrap(), b"target-after");

        fs::write(&backlink, b"external").unwrap();
        let conflicting = vec![
            (
                target.clone(),
                Some(b"target-after".to_vec()),
                Some(b"target-redone".to_vec()),
            ),
            (
                backlink.clone(),
                Some(b"backlink-after".to_vec()),
                Some(b"backlink-redone".to_vec()),
            ),
        ];
        assert_eq!(
            merge_transaction_changes(&root, &conflicting),
            Err(ComposeError::ExternalChange)
        );
        assert_eq!(fs::read(&target).unwrap(), b"target-after");
        assert_eq!(fs::read(&backlink).unwrap(), b"external");
        let _ = fs::remove_dir_all(root);
    }
}
