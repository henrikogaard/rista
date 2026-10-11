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

pub fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Directory holding a document's snapshots: mirrors its vault-relative
/// path with the `.md` extension stripped.
fn snapshot_dir(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let rel = path.strip_prefix(&root).ok()?;
    let mut dir = rel.to_path_buf();
    dir.set_extension("");
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
) -> io::Result<()> {
    if existing == replacement {
        return Ok(());
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
    loop {
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
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                suffix = suffix.saturating_add(1);
            }
            Err(error) => return Err(error),
        }
    }
    prune(&dir);
    Ok(())
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

#[derive(Serialize, Deserialize, Clone)]
pub struct TrashEntry {
    /// File name inside `.rista/trash/` (`<epoch>--<name>`).
    pub trashed: String,
    /// Original vault-relative path, e.g. `notes/beautiful.md`.
    pub original: String,
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
    fn format_epoch_epoch_zero() {
        assert_eq!(format_epoch(0), "1970-01-01 00:00:00");
    }
}
