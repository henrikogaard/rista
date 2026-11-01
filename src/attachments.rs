//! Attachment housekeeping: image files no note references.
//!
//! The selection is resolver-agnostic — [`unused`] takes the candidate files
//! and the referenced set — so it is testable without a live vault. `Vault`
//! supplies both.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Lexically normalize a path — drop `.` and apply `..` against the previous
/// component. Candidate image paths and resolved references both derive from
/// the vault root, so this makes `notes/../assets/x.png` comparable with the
/// indexed `assets/x.png` without touching the filesystem.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Candidate images that no note references, largest first. Paths are
/// normalized before comparison.
pub fn unused(
    candidates: impl IntoIterator<Item = (PathBuf, u64)>,
    referenced: &HashSet<PathBuf>,
) -> Vec<(PathBuf, u64)> {
    let referenced: HashSet<PathBuf> = referenced.iter().map(|path| normalize(path)).collect();
    let mut unused = candidates
        .into_iter()
        .filter(|(path, _)| !referenced.contains(&normalize(path)))
        .collect::<Vec<_>>();
    unused.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    unused
}

/// A compact byte size for the status line, e.g. `4.2 MB`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unused_keeps_referenced_out_and_sorts_by_size() {
        let referenced = HashSet::from([PathBuf::from("/v/used.png")]);
        let candidates = vec![
            (PathBuf::from("/v/used.png"), 10),
            (PathBuf::from("/v/small.png"), 5),
            (PathBuf::from("/v/big.png"), 900),
        ];
        let unused = unused(candidates, &referenced);
        assert_eq!(
            unused,
            vec![
                (PathBuf::from("/v/big.png"), 900),
                (PathBuf::from("/v/small.png"), 5)
            ]
        );
    }

    #[test]
    fn unused_normalizes_parent_relative_references() {
        // A note in `notes/` embedding `../assets/photo.png` produces a
        // `..`-containing path; it must still match the indexed image.
        let referenced = HashSet::from([PathBuf::from("/v/notes/../assets/photo.png")]);
        let candidates = vec![(PathBuf::from("/v/assets/photo.png"), 10)];
        assert!(unused(candidates, &referenced).is_empty());
    }

    #[test]
    fn human_size_uses_readable_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
