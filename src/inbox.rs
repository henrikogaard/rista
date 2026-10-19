//! Quick Capture: one plain Markdown note per capture in `Inbox/` at the
//! vault root. Shared by the in-app dialog and `rista inbox`.

use chrono::NaiveDateTime;
use std::path::{Path, PathBuf};

pub(crate) const INBOX_DIR: &str = "Inbox";
const MAX_SLUG_CHARS: usize = 40;

/// One triage step the Inbox view can take back.
pub(crate) enum InboxUndo {
    /// Moved with link retargeting; undone by moving back the same way.
    Moved { from: PathBuf, to: PathBuf },
    /// Sent to `.rista/trash` under this name; undone by restoring it.
    Trashed { trashed: String },
}

/// The first line, made safe for a file name: characters that are invalid
/// in file names (or break `[[wikilinks]]`) dropped, whitespace collapsed,
/// cut to about 40 characters on a word boundary.
pub(crate) fn slug(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default();
    let cleaned: String = first
        .chars()
        .filter(|c| {
            // Tabs are whitespace, so they collapse into spaces below.
            (c.is_whitespace() || !c.is_control())
                && !matches!(
                    c,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']'
                )
        })
        .collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut slug: String = words.chars().take(MAX_SLUG_CHARS).collect();
    if words.chars().count() > MAX_SLUG_CHARS {
        let cut_mid_word = words
            .chars()
            .nth(MAX_SLUG_CHARS)
            .is_some_and(|c| !c.is_whitespace());
        if let (true, Some(space)) = (cut_mid_word, slug.rfind(' ')) {
            slug.truncate(space);
        }
    }
    // A leading dot hides the file; a trailing dot is invalid on Windows.
    slug.trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string()
}

/// `YYYY-MM-DD HHmm <first words>`, or the time alone when the first line
/// has nothing usable.
pub(crate) fn file_stem(text: &str, now: NaiveDateTime) -> String {
    let time = now.format("%Y-%m-%d %H%M").to_string();
    match slug(text) {
        slug if slug.is_empty() => time,
        slug => format!("{time} {slug}"),
    }
}

/// The first free `<stem>.md`, `<stem> 2.md`, `<stem> 3.md`, … in `dir`.
pub(crate) fn free_path(dir: &Path, text: &str, now: NaiveDateTime) -> PathBuf {
    let stem = file_stem(text, now);
    let mut path = dir.join(format!("{stem}.md"));
    let mut n = 2;
    while std::fs::symlink_metadata(&path).is_ok() {
        path = dir.join(format!("{stem} {n}.md"));
        n += 1;
    }
    path
}

/// The captured text as written, under a `created` frontmatter stamp.
pub(crate) fn content(text: &str, now: NaiveDateTime) -> String {
    format!(
        "---\ncreated: {}\n---\n{text}",
        now.format("%Y-%m-%dT%H:%M:%S")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 19)
            .unwrap()
            .and_hms_opt(9, 5, 7)
            .unwrap()
    }

    #[test]
    fn slug_uses_first_line_and_drops_invalid_characters() {
        assert_eq!(
            slug("Call Río about: the *plan*?\nsecond line"),
            "Call Río about the plan"
        );
        assert_eq!(
            slug("# Heading [[link]] a/b\\c <x> \"q\" | ^id"),
            "Heading link abc x q id"
        );
        assert_eq!(slug("  tabs\tand   spaces  "), "tabs and spaces");
        assert_eq!(slug("...hidden."), "hidden");
        assert_eq!(slug("bell\u{7}char"), "bellchar");
    }

    #[test]
    fn slug_caps_length_on_a_word_boundary() {
        let long = "one two three four five six seven eight nine ten";
        let capped = slug(long);
        assert_eq!(capped, "one two three four five six seven eight");
        assert!(capped.chars().count() <= MAX_SLUG_CHARS);
        let word = "x".repeat(60);
        assert_eq!(slug(&word).chars().count(), MAX_SLUG_CHARS);
        let exact = "a".repeat(39) + " b";
        assert_eq!(slug(&exact), "a".repeat(39));
        let multibyte = "æøå ".repeat(15);
        assert!(slug(&multibyte).chars().count() <= MAX_SLUG_CHARS);
    }

    #[test]
    fn stem_falls_back_to_time_when_the_first_line_is_empty() {
        assert_eq!(file_stem("Buy milk", now()), "2026-10-19 0905 Buy milk");
        assert_eq!(file_stem("\nbody only", now()), "2026-10-19 0905");
        assert_eq!(file_stem("???", now()), "2026-10-19 0905");
        assert_eq!(file_stem("", now()), "2026-10-19 0905");
    }

    #[test]
    fn free_path_adds_numbered_suffixes_on_collision() {
        let dir = std::env::temp_dir().join(format!(
            "rista-inbox-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let first = free_path(&dir, "Idea", now());
        assert_eq!(first, dir.join("2026-10-19 0905 Idea.md"));
        std::fs::write(&first, "").unwrap();
        let second = free_path(&dir, "Idea", now());
        assert_eq!(second, dir.join("2026-10-19 0905 Idea 2.md"));
        std::fs::write(&second, "").unwrap();
        assert_eq!(
            free_path(&dir, "Idea", now()),
            dir.join("2026-10-19 0905 Idea 3.md")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn content_is_created_frontmatter_then_text_as_written() {
        assert_eq!(
            content("Line one\n\n- two\n", now()),
            "---\ncreated: 2026-10-19T09:05:07\n---\nLine one\n\n- two\n"
        );
        assert_eq!(
            content("no newline", now()),
            "---\ncreated: 2026-10-19T09:05:07\n---\nno newline"
        );
    }
}
