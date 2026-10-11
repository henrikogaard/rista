//! Local page history + trash, stored under `<vault>/.rista/`.
//!
//! - `history/<doc-relative-dir>/<epoch>.md` — a snapshot of the file's
//!   previous on-disk content, taken before each save overwrites it.
//! - `trash/<epoch>--<name>` — files "deleted" from the tree land here;
//!   `trash/index.json` records the original path so they can be restored.
//!
//! `.rista` is dot-prefixed, so the vault tree never shows it.

use serde::{Deserialize, Serialize};
use std::io;
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
    std::fs::create_dir_all(path)?;
    let root = root.canonicalize()?;
    let path = path.canonicalize()?;
    if !path.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata path outside vault",
        ));
    }
    Ok(path)
}

pub fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Directory holding a document's snapshots: mirrors its vault-relative
/// path with the `.md` extension stripped.
fn snapshot_dir(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(root).ok()?;
    let mut dir = rel.to_path_buf();
    dir.set_extension("");
    Some(history_dir(root).join(dir))
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
    let Some(dir) = snapshot_dir(root, path) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::write(dir.join(format!("{}.md", epoch())), existing);
    prune(&dir);
}

fn prune(dir: &Path) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in names.iter().take(names.len().saturating_sub(MAX_SNAPSHOTS)) {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// Snapshots for `path`, newest first: `(epoch, file)`.
pub fn snapshots(root: &Path, path: &Path) -> Vec<(u64, PathBuf)> {
    let Some(dir) = snapshot_dir(root, path) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ts: u64 = name.strip_suffix(".md")?.parse().ok()?;
                    Some((ts, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|(epoch, _)| std::cmp::Reverse(*epoch));
    out
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
    let _ = std::fs::create_dir_all(trash_dir(root));
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(index_path(root), json);
    }
}

pub fn snapshot_before_write_checked(
    root: &Path,
    path: &Path,
    new_bytes: &[u8],
) -> io::Result<Option<PathBuf>> {
    let canonical_root = root.canonicalize()?;
    let canonical_path = path.canonicalize()?;
    let relative = canonical_path
        .strip_prefix(&canonical_root)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path outside vault"))?;
    if std::fs::metadata(&canonical_path)?.len() > crate::note_composer::MAX_NOTE_BYTES as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "note exceeds composer history limit",
        ));
    }
    let existing = std::fs::read(&canonical_path)?;
    if existing == new_bytes {
        return Ok(None);
    }
    let mut dir = relative.to_path_buf();
    dir.set_extension("");
    let dir = checked_meta_dir(&canonical_root, &history_dir(&canonical_root).join(dir))?;
    let mut destination = dir.join(format!("{}.md", epoch()));
    let mut suffix = 0u32;
    while destination.exists() {
        suffix += 1;
        destination = dir.join(format!("{}-{suffix}.md", epoch()));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)?;
    use std::io::Write;
    file.write_all(&existing)?;
    file.sync_all()?;
    prune(&dir);
    Ok(Some(destination))
}

/// Move `path` into `.rista/trash/` and record its original location.
pub fn move_to_trash(root: &Path, path: &Path) -> io::Result<()> {
    let rel = path
        .strip_prefix(root)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    std::fs::create_dir_all(trash_dir(root))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "untitled".into());
    let trashed = format!("{}--{}", epoch(), name);
    let mut trashed = trashed.clone();
    // Extremely unlikely collision (same name, same second) — disambiguate.
    while trash_dir(root).join(&trashed).exists() {
        trashed = format!("{}--{}", epoch() + 1, name);
    }
    std::fs::rename(path, trash_dir(root).join(&trashed))?;
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
    let target = root.join(&entry.original);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(trash_dir(root).join(&entry.trashed), &target)?;
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

    fn temp_vault() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rista-test-{}", epoch()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
        let snapshot = snapshot_before_write_checked(&root, &note, b"after")
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(snapshot).unwrap(), b"before");

        std::fs::create_dir_all(trash_dir(&root)).unwrap();
        std::fs::write(index_path(&root), b"{").unwrap();
        assert_eq!(
            prepare_trash_transaction(&root, &note).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&note).unwrap(), b"before");
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
        let (entry, changes) = prepare_trash_transaction(&root, &note).unwrap();
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
