//! Attachment housekeeping: image files no note references.
//!
//! The selection is resolver-agnostic — [`unused`] takes the candidate files
//! and the referenced set — so it is testable without a live vault. `Vault`
//! supplies both.

use std::collections::HashSet;
use std::path::PathBuf;

/// Candidate images that no note references, largest first.
pub fn unused(
    candidates: impl IntoIterator<Item = (PathBuf, u64)>,
    referenced: &HashSet<PathBuf>,
) -> Vec<(PathBuf, u64)> {
    let mut unused = candidates
        .into_iter()
        .filter(|(path, _)| !referenced.contains(path))
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
    fn human_size_uses_readable_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
