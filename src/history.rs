//! Local page history + trash, stored under `<vault>/.rista/`.
//!
//! - `history/<doc-relative-dir>/<epoch>.md` — a snapshot of the file's
//!   previous on-disk content, taken before each save overwrites it.
//! - `trash/<epoch>--<name>` — files "deleted" from the tree land here;
//!   `trash/index.json` records the original path so they can be restored.
//!
//! `.rista` is dot-prefixed, so the vault tree never shows it.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAX_SNAPSHOTS: usize = 30;
const INDEX_NAME: &str = "index.json";

fn meta_dir(root: &Path) -> PathBuf {
    root.join(".rista")
}

fn history_dir(root: &Path) -> PathBuf {
    meta_dir(root).join("history")
}

fn trash_dir(root: &Path) -> PathBuf {
    meta_dir(root).join("trash")
}

fn checked_meta_dir(root: &Path, path: &Path) -> io::Result<PathBuf> {
    crate::note_composer::checked_vault_dir(root, path)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.message(false)))
}

pub fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Directory holding a document's snapshots: mirrors its vault-relative
/// path with ordinary document extensions stripped.
fn snapshot_dir(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = path.strip_prefix(&root).ok()?;
    let mut dir = rel.to_path_buf();
    if !crate::canvas::is_canvas(&path) {
        dir.set_extension("");
    }
    Some(history_dir(&root).join(dir))
}

/// Copy the file's current on-disk content into history before `new_text`
/// replaces it. No-op when the file is new or unchanged.
pub fn snapshot_before_write(root: &Path, path: &Path, new_text: &str) {
    let Ok(existing) = std::fs::read_to_string(path) else {
        return;
    };
    if existing == new_text {
        return;
    }
    let _ = snapshot_before_write_checked(root, path, existing.as_bytes(), new_text.as_bytes());
}

pub(crate) fn snapshot_before_write_checked(
    root: &Path,
    path: &Path,
    existing: &[u8],
    replacement: &[u8],
) -> io::Result<Option<PathBuf>> {
    if existing == replacement {
        return Ok(None);
    }
    let root = root.canonicalize()?;
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot source is not a regular file",
        ));
    }
    let path = path.canonicalize()?;
    if !path.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot source is outside vault",
        ));
    }
    let dir = snapshot_dir(&root, &path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path is outside vault"))?;
    let relative = dir
        .strip_prefix(&root)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let mut current = root.clone();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe history path",
            ));
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "history path is not a safe directory",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(&current)?;
            }
            Err(error) => return Err(error),
        }
        if !current.canonicalize()?.starts_with(&root) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "history path escapes vault",
            ));
        }
    }

    let timestamp = epoch();
    let mut suffix = 0u64;
    let snapshot = loop {
        let name = if suffix == 0 {
            format!("{timestamp}.md")
        } else {
            format!("{timestamp}-{suffix}.md")
        };
        let candidate = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                let result = file.write_all(existing).and_then(|()| file.sync_all());
                drop(file);
                if let Err(error) = result {
                    let _ = std::fs::remove_file(&candidate);
                    return Err(error);
                }
                break candidate;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                suffix = suffix.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    };
    prune(&dir);
    Ok(Some(snapshot))
}

pub(crate) fn snapshot_composer_before_write_checked(
    root: &Path,
    path: &Path,
    new_bytes: &[u8],
) -> io::Result<Option<PathBuf>> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    if !path.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path outside vault",
        ));
    }
    if std::fs::metadata(&path)?.len() > crate::note_composer::MAX_NOTE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "note exceeds composer history limit",
        ));
    }
    let existing = std::fs::read(&path)?;
    snapshot_before_write_checked(&root, &path, &existing, new_bytes)
}

fn prune(dir: &Path) {
    let mut paths: Vec<(u64, u64, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|entry| {
                    entry
                        .file_type()
                        .map(|file_type| file_type.is_file())
                        .unwrap_or(false)
                })
                .filter_map(|entry| {
                    let (timestamp, suffix) = snapshot_stamp(&entry.file_name())?;
                    Some((timestamp, suffix, entry.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    paths.sort_by_key(|(timestamp, suffix, path)| (*timestamp, *suffix, path.clone()));
    let excess = paths.len().saturating_sub(MAX_SNAPSHOTS);
    for (_, _, path) in paths.into_iter().take(excess) {
        if std::fs::symlink_metadata(&path)
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn snapshot_stamp(name: &OsStr) -> Option<(u64, u64)> {
    let stem = name.to_str()?.strip_suffix(".md")?;
    let (timestamp, suffix) = stem.split_once('-').unwrap_or((stem, "0"));
    Some((timestamp.parse().ok()?, suffix.parse().ok()?))
}

/// Snapshots for `path`, newest first: `(epoch, file)`.
pub fn snapshots(root: &Path, path: &Path) -> Vec<(u64, PathBuf)> {
    let Some(dir) = snapshot_dir(root, path) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, u64, PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let (timestamp, suffix) = snapshot_stamp(&e.file_name())?;
                    Some((timestamp, suffix, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(timestamp, suffix, path)| {
        (
            std::cmp::Reverse(*timestamp),
            std::cmp::Reverse(*suffix),
            path.clone(),
        )
    });
    out.into_iter()
        .map(|(timestamp, _, path)| (timestamp, path))
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TrashEntry {
    /// File name inside `.rista/trash/` (`<epoch>--<name>`).
    pub trashed: String,
    /// Original vault-relative path, e.g. `notes/beautiful.md`.
    pub original: String,
}

pub(crate) fn prepare_trash_transaction(
    root: &Path,
    path: &Path,
    expected_source: &[u8],
) -> io::Result<(
    TrashEntry,
    Vec<crate::note_composer::ComposerTransactionChange>,
)> {
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path outside vault"))?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > crate::note_composer::MAX_NOTE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "note exceeds composer trash limit",
        ));
    }
    let source = std::fs::read(&path)?;
    if source != expected_source {
        return Err(io::Error::other(
            "source changed after the composer snapshot",
        ));
    }
    let trash = checked_meta_dir(&root, &trash_dir(&root))?;
    let index_path = trash.join(INDEX_NAME);
    let index_before = match std::fs::symlink_metadata(&index_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "trash index is a symbolic link",
            ));
        }
        Ok(metadata) if metadata.len() > crate::note_composer::MAX_NOTE_BYTES as u64 => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "trash index is too large",
            ));
        }
        Ok(_) => Some(std::fs::read(&index_path)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let mut entries = match &index_before {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        None => Vec::new(),
    };
    if entries.iter().any(|entry: &TrashEntry| {
        let trashed = Path::new(&entry.trashed);
        let original = Path::new(&entry.original);
        trashed.is_absolute()
            || trashed.components().count() != 1
            || !matches!(
                trashed.components().next(),
                Some(std::path::Component::Normal(_))
            )
            || original.is_absolute()
            || original.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            })
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trash index contains an unsafe path",
        ));
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".into());
    let mut suffix = 0u32;
    let unique = loop {
        let candidate = format!(
            "{}-{}-{}-{suffix}--{}",
            epoch(),
            std::process::id(),
            entries.len(),
            name
        );
        if !trash.join(&candidate).exists() {
            break candidate;
        }
        suffix = suffix.saturating_add(1);
    };
    let entry = TrashEntry {
        trashed: unique,
        original: relative.to_string_lossy().to_string(),
    };
    entries.push(entry.clone());
    let index_after = serde_json::to_vec_pretty(&entries)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let changes = vec![
        (trash.join(&entry.trashed), None, Some(source.clone())),
        (index_path, index_before, Some(index_after)),
        (path, Some(source), None),
    ];
    if changes.iter().any(|(_, before, after)| {
        before
            .as_ref()
            .is_some_and(|bytes| bytes.len() > crate::note_composer::MAX_NOTE_BYTES)
            || after
                .as_ref()
                .is_some_and(|bytes| bytes.len() > crate::note_composer::MAX_NOTE_BYTES)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "trash transaction exceeds composer limits",
        ));
    }
    Ok((entry, changes))
}

fn index_path(root: &Path) -> PathBuf {
    trash_dir(root).join(INDEX_NAME)
}

fn read_index(root: &Path) -> Vec<TrashEntry> {
    std::fs::read_to_string(index_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_index(root: &Path, entries: &[TrashEntry]) {
    let Ok(dir) = checked_meta_dir(root, &trash_dir(root)) else {
        return;
    };
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(dir.join(INDEX_NAME), json);
    }
}

pub(crate) fn prepare_composer_snapshots(
    root: &Path,
    changes: &[crate::note_composer::ComposerTransactionChange],
) -> io::Result<Vec<crate::note_composer::ComposerTransactionChange>> {
    let root = root.canonicalize()?;
    let mut total = 0usize;
    let mut snapshots = Vec::new();
    for (path, before, after) in changes {
        let bytes = match (before, after) {
            (Some(before), Some(after)) if before != after => before,
            (Some(before), None) => before,
            (None, Some(after)) => after,
            _ => continue,
        };
        if bytes.len() > crate::note_composer::MAX_NOTE_BYTES
            || total > crate::note_composer::MAX_TOTAL_BYTES.saturating_sub(bytes.len())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "composer snapshots exceed supported bounds",
            ));
        }
        total += bytes.len();
        let canonical =
            if path.exists() {
                path.canonicalize()?
            } else {
                path.parent()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid path"))?
                    .canonicalize()?
                    .join(path.file_name().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid path")
                    })?)
            };
        let relative = canonical
            .strip_prefix(&root)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut dir = relative.to_path_buf();
        dir.set_extension("");
        let dir = checked_meta_dir(&root, &history_dir(&root).join(dir))?;
        let timestamp = epoch();
        let mut suffix = 0u64;
        let snapshot = loop {
            let name = if suffix == 0 {
                format!("{timestamp}.md")
            } else {
                format!("{timestamp}-{suffix}.md")
            };
            let candidate = dir.join(name);
            if !candidate.exists() {
                break candidate;
            }
            suffix = suffix.saturating_add(1);
        };
        snapshots.push((snapshot, None, Some(bytes.clone())));
    }
    Ok(snapshots)
}

/// Move `path` into `.rista/trash/` and record its original location.
pub fn move_to_trash(root: &Path, path: &Path) -> io::Result<()> {
    let rel = path
        .strip_prefix(root)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let trash = checked_meta_dir(root, &trash_dir(root))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".into());
    let trashed = format!("{}--{}", epoch(), name);
    let mut trashed = trashed.clone();
    // Extremely unlikely collision (same name, same second) — disambiguate.
    while trash.join(&trashed).exists() {
        trashed = format!("{}--{}", epoch() + 1, name);
    }
    std::fs::rename(path, trash.join(&trashed))?;
    let mut index = read_index(root);
    index.push(TrashEntry {
        trashed,
        original: rel.to_string_lossy().to_string(),
    });
    write_index(root, &index);
    Ok(())
}

/// Trash contents in deletion order, oldest first.
pub fn trash_entries(root: &Path) -> Vec<TrashEntry> {
    read_index(root)
        .into_iter()
        .filter(|e| trash_dir(root).join(&e.trashed).exists())
        .collect()
}

/// Restore a trashed entry to its original vault-relative path.
pub fn restore(root: &Path, trashed_name: &str) -> io::Result<()> {
    let mut index = read_index(root);
    let Some(ix) = index.iter().position(|e| e.trashed == trashed_name) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "not in trash index",
        ));
    };
    let entry = index.remove(ix);
    let relative = Path::new(&entry.original);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "trash index contains an unsafe path",
        ));
    }
    let target = root.join(&entry.original);
    if let Some(parent) = target.parent() {
        checked_meta_dir(root, parent)?;
    }
    let trash = checked_meta_dir(root, &trash_dir(root))?;
    let source = trash.join(&entry.trashed);
    if source.parent() != Some(trash.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "trash index contains an unsafe path",
        ));
    }
    std::fs::rename(source, &target)?;
    write_index(root, &index);
    Ok(())
}

/// `1728123456` → `"2026-10-04 17:30"`.
pub fn format_epoch(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let (h, m, s) = (
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60,
    );
    // civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_vault() -> PathBuf {
        loop {
            let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("rista-history-{}-{id}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return dir,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create test vault: {error}"),
            }
        }
    }

    #[test]
    fn snapshot_then_trash_restore_round_trip() {
        let root = temp_vault();
        let note = root.join("note.md");
        std::fs::write(&note, "v1").unwrap();

        snapshot_before_write(&root, &note, "v2");
        std::fs::write(&note, "v2").unwrap();
        let snaps = snapshots(&root, &note);
        assert_eq!(snaps.len(), 1);
        assert_eq!(std::fs::read_to_string(&snaps[0].1).unwrap(), "v1");

        // Unchanged content → no snapshot.
        snapshot_before_write(&root, &note, "v2");
        assert_eq!(snapshots(&root, &note).len(), 1);

        move_to_trash(&root, &note).unwrap();
        assert!(!note.exists());
        let entries = trash_entries(&root);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].original, "note.md");

        restore(&root, &entries[0].trashed).unwrap();
        assert!(note.exists());
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "v2");
        assert!(trash_entries(&root).is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn checked_snapshot_refuses_symlinked_history_directory() {
        use std::os::unix::fs::symlink;

        let root = temp_vault();
        let outside = temp_vault();
        let note = root.join("note.md");
        std::fs::write(&note, b"before").unwrap();
        std::fs::create_dir_all(root.join(".rista/history")).unwrap();
        symlink(&outside, root.join(".rista/history/note")).unwrap();

        assert!(snapshot_before_write_checked(&root, &note, b"before", b"after").is_err());
        assert_eq!(std::fs::read(&note).unwrap(), b"before");
        assert!(std::fs::read_dir(&outside).unwrap().next().is_none());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn snapshots_sort_same_second_suffixes_numerically() {
        let root = temp_vault();
        let note = root.join("note.md");
        std::fs::write(&note, "").unwrap();
        let history = root.join(".rista/history/note");
        std::fs::create_dir_all(&history).unwrap();
        for name in ["100.md", "100-2.md", "100-10.md", "99.md"] {
            std::fs::write(history.join(name), "").unwrap();
        }

        let paths = snapshots(&root, &note)
            .into_iter()
            .map(|(_, path)| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["100-10.md", "100-2.md", "100.md", "99.md"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn checked_composer_history_preserves_snapshot_and_rejects_bad_trash_index() {
        let root = std::env::temp_dir().join(format!(
            "rista-checked-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let note = root.join("note.md");
        std::fs::write(&note, "before").unwrap();
        let snapshot = snapshot_composer_before_write_checked(&root, &note, b"after")
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(snapshot).unwrap(), b"before");

        std::fs::create_dir_all(trash_dir(&root)).unwrap();
        std::fs::write(index_path(&root), b"{").unwrap();
        assert_eq!(
            prepare_trash_transaction(&root, &note, b"before")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&note).unwrap(), b"before");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checked_trash_rejects_a_source_that_differs_from_the_captured_snapshot() {
        let root = temp_vault();
        let note = root.join("note.md");
        std::fs::write(&note, b"external").unwrap();

        assert_eq!(
            prepare_trash_transaction(&root, &note, b"captured")
                .unwrap_err()
                .kind(),
            io::ErrorKind::Other
        );
        assert_eq!(std::fs::read(&note).unwrap(), b"external");
        assert!(trash_entries(&root).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn snapshots_lists_collision_suffixes_in_newest_order() {
        let root = temp_vault();
        let note = root.join("note.md");
        std::fs::write(&note, "current").unwrap();
        let dir = snapshot_dir(&root, &note).unwrap();
        checked_meta_dir(&root, &dir).unwrap();
        for name in ["100.md", "100-1.md", "100-2.md", "99.md"] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        let listed = snapshots(&root, &note);
        assert_eq!(
            listed.iter().map(|(epoch, _)| *epoch).collect::<Vec<_>>(),
            [100, 100, 100, 99]
        );
        assert!(listed[0].1.ends_with("100-2.md"));
        assert!(listed[1].1.ends_with("100-1.md"));
        assert!(listed[2].1.ends_with("100.md"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checked_trash_moves_note_and_index_in_one_transaction() {
        let root = std::env::temp_dir().join(format!(
            "rista-checked-trash-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let note = root.join("note.md");
        std::fs::write(&note, b"before").unwrap();
        let (entry, changes) = prepare_trash_transaction(&root, &note, b"before").unwrap();
        crate::note_composer::merge_transaction_changes(&root, &changes).unwrap();
        assert!(!note.exists());
        assert_eq!(
            std::fs::read(trash_dir(&root).join(&entry.trashed)).unwrap(),
            b"before"
        );
        assert_eq!(trash_entries(&root), vec![entry]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn format_epoch_epoch_zero() {
        assert_eq!(format_epoch(0), "1970-01-01 00:00:00");
    }
}
