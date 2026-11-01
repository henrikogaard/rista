//! Link health: broken wikilinks and orphan notes across the vault.
//!
//! The aggregation is resolver-agnostic — [`analyze`] takes the same closure
//! shape `Vault::outgoing_from` provides — so the classification stays
//! testable without a live vault.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A link whose target resolves to no file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenLink {
    pub source: PathBuf,
    pub target: String,
}

/// The result of a vault-wide link scan.
#[derive(Clone, Debug, Default)]
pub struct LinkHealth {
    pub broken: Vec<BrokenLink>,
    pub orphans: Vec<PathBuf>,
}

impl LinkHealth {
    pub fn is_empty(&self) -> bool {
        self.broken.is_empty() && self.orphans.is_empty()
    }
}

/// Markdown notes are the only files this scan reads; canvases and bases
/// carry links in their own formats.
fn is_note(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("md") | Some("markdown")
    )
}

/// Scan `notes`, resolving each one's links with `resolve`, and report broken
/// links plus orphan notes (no resolved outgoing links and no incoming ones).
pub fn analyze(
    notes: &[PathBuf],
    resolve: impl Fn(&str, &Path) -> (Vec<PathBuf>, Vec<String>),
) -> LinkHealth {
    let mut broken = Vec::new();
    let mut incoming: HashMap<PathBuf, usize> = HashMap::new();
    let mut outgoing: HashMap<PathBuf, usize> = HashMap::new();
    for path in notes.iter().filter(|path| is_note(path)) {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let dir = path.parent().unwrap_or_else(|| Path::new("/"));
        let (resolved, unresolved) = resolve(&text, dir);
        outgoing.insert(path.clone(), resolved.len());
        for target in resolved {
            *incoming.entry(target).or_default() += 1;
        }
        for target in unresolved {
            broken.push(BrokenLink {
                source: path.clone(),
                target,
            });
        }
    }
    broken.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.target.cmp(&right.target))
    });
    let mut orphans = notes
        .iter()
        .filter(|path| is_note(path))
        .filter(|path| {
            outgoing.get(*path).copied().unwrap_or(0) == 0
                && incoming.get(*path).copied().unwrap_or(0) == 0
        })
        .cloned()
        .collect::<Vec<_>>();
    orphans.sort();
    LinkHealth { broken, orphans }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_dir() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rista-link-health-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, name: &str, body: &str) -> PathBuf {
        let path = root.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Resolves the literal target `b` to `b.md`, everything else unresolved.
    fn fake_resolve(text: &str, dir: &Path) -> (Vec<PathBuf>, Vec<String>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for after in text.split("[[").skip(1) {
            let target = after.split("]]").next().unwrap_or_default();
            if target == "b" {
                resolved.push(dir.join("b.md"));
            } else {
                unresolved.push(target.to_string());
            }
        }
        (resolved, unresolved)
    }

    #[test]
    fn reports_broken_links_and_orphans() {
        let root = temp_dir();
        let a = write(&root, "a.md", "links [[b]] and [[missing]]");
        let b = write(&root, "b.md", "plain text");
        let lonely = write(&root, "lonely.md", "");
        let health = analyze(&[a.clone(), b.clone(), lonely.clone()], fake_resolve);
        assert_eq!(health.broken.len(), 1);
        assert_eq!(health.broken[0].source, a);
        assert_eq!(health.broken[0].target, "missing");
        // `a` links out, `b` is linked to, `lonely` is neither.
        assert_eq!(health.orphans, vec![lonely]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_markdown_files_are_ignored() {
        let root = temp_dir();
        let note = write(&root, "n.md", "");
        let canvas = root.join("board.canvas");
        std::fs::write(&canvas, "{\"nodes\":[],\"edges\":[]}").unwrap();
        let health = analyze(&[note.clone(), canvas.clone()], |_, _| {
            (Vec::new(), Vec::new())
        });
        assert_eq!(health.orphans, vec![note]);
        std::fs::remove_dir_all(root).unwrap();
    }
}
