use gpui_kit::{Bounds, Pixels, Point};
use markdown::mdast::Node;
use regex::Regex;
use std::ops::Range;
use std::sync::{Arc, LazyLock};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(target_os = "macos")]
mod macos;

pub(crate) type SpellResults = Vec<Vec<Range<usize>>>;
pub(crate) type SpellCheckReceiver = smol::channel::Receiver<Result<SpellResults, SpellError>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SpellError {
    #[cfg(not(target_os = "macos"))]
    Unavailable,
    InvalidLanguages(Vec<String>),
}

pub(crate) trait SpellChecker {
    fn check(&self, text: String, languages: &[String]) -> SpellCheckReceiver;
    fn suggestions(&self, word: &str, languages: &[String]) -> Vec<String>;
    fn ignore(&self, word: &str);
    fn learn(&self, word: &str);
}

#[cfg(target_os = "macos")]
pub(crate) fn create_checker(cx: &gpui_kit::App) -> std::rc::Rc<dyn SpellChecker> {
    std::rc::Rc::new(macos::MacSpellChecker::new(
        cx.foreground_executor().clone(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn create_checker(_cx: &gpui_kit::App) -> std::rc::Rc<dyn SpellChecker> {
    std::rc::Rc::new(UnavailableSpellChecker)
}

pub(crate) fn available_languages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        macos::available_languages()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

pub(crate) const fn is_supported() -> bool {
    cfg!(target_os = "macos")
}

pub(crate) fn target_matches(
    text: &str,
    current_revision: u64,
    revision: u64,
    start: usize,
    end: usize,
    original: &str,
) -> bool {
    current_revision == revision
        && start <= end
        && end <= text.len()
        && text.is_char_boundary(start)
        && text.is_char_boundary(end)
        && text.get(start..end) == Some(original)
}

#[cfg(not(target_os = "macos"))]
struct UnavailableSpellChecker;

#[cfg(not(target_os = "macos"))]
impl SpellChecker for UnavailableSpellChecker {
    fn check(&self, _text: String, _languages: &[String]) -> SpellCheckReceiver {
        let (sender, receiver) = smol::channel::unbounded();
        let _ = sender.try_send(Err(SpellError::Unavailable));
        receiver
    }

    fn suggestions(&self, _word: &str, _languages: &[String]) -> Vec<String> {
        Vec::new()
    }

    fn ignore(&self, _word: &str) {}

    fn learn(&self, _word: &str) {}
}

#[derive(Clone)]
pub(crate) struct Analysis {
    pub(crate) text: Arc<str>,
    pub(crate) words: Vec<Range<usize>>,
}

impl Analysis {
    pub(crate) fn new(text: String) -> Self {
        let mut constructs = markdown::Constructs::gfm();
        constructs.frontmatter = true;
        constructs.math_flow = true;
        constructs.math_text = true;
        let options = markdown::ParseOptions {
            constructs,
            math_text_single_dollar: true,
            ..markdown::ParseOptions::gfm()
        };
        let mut prose = Vec::new();
        if let Ok(root) = markdown::to_mdast(&text, &options) {
            prose_ranges(&root, &mut prose);
        }
        static EXCLUDED: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
                r"!?\[\[[^\]\r\n]*\]\]|(?:https?://|www\.|mailto:)[^\s<>()]+|(?:^|[\s(])#[\p{L}\p{N}_/-]+|(?s:\\\[.*?\\\]|\\\(.*?\\\))|&(?:#[0-9]+|#x[0-9A-Fa-f]+|[A-Za-z][A-Za-z0-9]+);",
            )
            .expect("spellcheck exclusion pattern")
        });
        let excluded = EXCLUDED
            .find_iter(&text)
            .map(|m| m.range())
            .collect::<Vec<_>>();
        let mut words = Vec::new();
        for range in prose {
            let Some(raw) = text.get(range.clone()) else {
                continue;
            };
            for (offset, word) in raw.unicode_word_indices() {
                let range = range.start + offset..range.start + offset + word.len();
                let first = excluded.partition_point(|item| item.end <= range.start);
                if word.chars().any(char::is_alphabetic)
                    && word.len() <= 512
                    && !excluded
                        .get(first)
                        .is_some_and(|item| item.start < range.end)
                {
                    words.push(range);
                }
            }
        }
        Self {
            text: text.into(),
            words,
        }
    }
}

fn prose_ranges(node: &Node, out: &mut Vec<Range<usize>>) {
    match node {
        Node::Code(_)
        | Node::InlineCode(_)
        | Node::Math(_)
        | Node::InlineMath(_)
        | Node::Link(_)
        | Node::LinkReference(_)
        | Node::Image(_)
        | Node::ImageReference(_)
        | Node::Definition(_)
        | Node::Yaml(_)
        | Node::Toml(_)
        | Node::Html(_) => return,
        Node::Text(_) => {
            if let Some(position) = node.position() {
                out.push(position.start.offset..position.end.offset);
            }
        }
        _ => {}
    }
    if let Some(children) = node.children() {
        for child in children {
            prose_ranges(child, out);
        }
    }
}

/// Clip cached words against actual laid-out caret positions, including wrapped lines.
/// Offscreen/folded offsets can map to the next visible line's start in GPUI.
pub(crate) fn visible_words(
    words: &[Range<usize>],
    viewport: Bounds<Pixels>,
    line_height: Pixels,
    mut locate: impl FnMut(usize) -> Option<Point<Pixels>>,
) -> Vec<Range<usize>> {
    let mut index = words.partition_point(|word| {
        locate(word.end).is_some_and(|end| end.y + line_height <= viewport.top())
    });
    let mut visible = Vec::new();
    while let Some(word) = words.get(index) {
        let (Some(start), Some(end)) = (locate(word.start), locate(word.end)) else {
            break;
        };
        if start.y >= viewport.bottom() {
            break;
        }
        if start == end {
            // Skip collapsed runs in logarithmic time rather than visiting every hidden word.
            index += 1;
            index += words[index..].partition_point(|word| {
                locate(word.end)
                    .is_some_and(|point| point.y < end.y || (point.y == end.y && point.x <= end.x))
            });
            continue;
        }
        if end.y + line_height > viewport.top()
            && (start.y != end.y || (end.x > viewport.left() && start.x < viewport.right()))
        {
            visible.push(word.clone());
        }
        index += 1;
    }
    visible
}

pub(crate) struct Projection {
    pub(crate) text: String,
    entries: Vec<(Range<usize>, Range<usize>)>,
}

impl Projection {
    pub(crate) fn new(analysis: &Analysis, words: &[Range<usize>]) -> Self {
        let mut text = String::new();
        let mut entries = Vec::new();
        for source in words {
            let Some(word) = analysis.text.get(source.clone()) else {
                continue;
            };
            let start = text.len();
            text.push_str(word);
            entries.push((source.clone(), start..text.len()));
            text.push('\n');
        }
        Self { text, entries }
    }

    /// A word is incorrect only when every selected language rejects it.
    pub(crate) fn diagnostics(&self, results: &[Vec<Range<usize>>]) -> Vec<Range<usize>> {
        if results.is_empty() {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|(_, projected)| {
                results.iter().all(|language| {
                    language
                        .iter()
                        .any(|range| range.start < projected.end && range.end > projected.start)
                })
            })
            .map(|(source, _)| source.clone())
            .collect()
    }
}

pub(crate) fn utf16_range_to_utf8(text: &str, range: Range<usize>) -> Option<Range<usize>> {
    if range.start > range.end {
        return None;
    }
    let mut units = 0;
    let mut start = None;
    for (byte, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if units == range.start {
            start = Some(byte)
        }
        if units == range.end {
            return start.map(|start| start..byte);
        }
        if units > range.end {
            return None;
        }
        units += ch.len_utf16();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{point, px, size};

    #[test]
    fn spellcheck_only_checks_prose_not_markdown_targets_or_code() {
        let input = "---\nmykey: badvalue\n---\n# Helo **wrld**\n`codebad` [linkbad](https://hostbad) [[wikibad|aliasbad]] #tagbad\n```rs\ncodebad\n```\n$$\nmathbad\n$$\n$inlinebad$ \\(latexwrong\\) https://urlbad www.hostbad\nHei Øgård café isn't naïve. Entities &amp; &#123; &#x1F600; remain source text.\n";
        let analysis = Analysis::new(input.into());
        let words = analysis
            .words
            .iter()
            .map(|range| &analysis.text[range.clone()])
            .collect::<Vec<_>>();
        assert_eq!(
            words,
            [
                "Helo", "wrld", "Hei", "Øgård", "café", "isn't", "naïve", "Entities", "remain",
                "source", "text"
            ]
        );
    }

    #[test]
    fn spellcheck_utf16_ranges_preserve_unicode_boundaries() {
        let text = "😀 Hælo café";
        assert_eq!(utf16_range_to_utf8(text, 3..7), Some(5..10));
        assert_eq!(utf16_range_to_utf8(text, 1..2), None);
        assert_eq!(utf16_range_to_utf8(text, 8..12), Some(11..16));
        assert_eq!(utf16_range_to_utf8(text, 99..100), None);
    }

    #[test]
    fn spellcheck_actions_require_current_revision_and_unchanged_utf8_target() {
        let text = "café typo";
        assert!(target_matches(text, 4, 4, 0, 5, "café"));
        assert!(!target_matches(text, 5, 4, 0, 5, "café"));
        assert!(!target_matches("cafe typo", 4, 4, 0, 5, "café"));
        assert!(!target_matches(text, 4, 4, 4, 5, "é"));
        assert!(!target_matches(text, 4, 4, 9, 5, "typo"));
    }

    #[test]
    fn spellcheck_projection_accepts_words_valid_in_any_selected_language() {
        let analysis = Analysis::new("hello hei wrng".into());
        let projection = Projection::new(&analysis, &analysis.words);
        assert_eq!(projection.text, "hello\nhei\nwrng\n");
        assert_eq!(
            projection.diagnostics(&[vec![6..9, 10..14], vec![0..5, 10..14]]),
            [10..14]
        );
        assert!(projection.diagnostics(&[Vec::new()]).is_empty());
        assert!(projection.diagnostics(&[]).is_empty());
    }

    #[test]
    fn spellcheck_visible_words_clip_wrapped_rows_and_horizontal_scroll() {
        let words = (0..20).map(|i| i * 5..i * 5 + 4).collect::<Vec<_>>();
        let viewport = Bounds::new(point(px(0.), px(20.)), size(px(50.), px(20.)));
        let actual = visible_words(&words, viewport, px(10.), |offset| {
            Some(point(
                px((offset % 20) as f32 * 3.),
                px((offset / 20) as f32 * 10.),
            ))
        });
        assert_eq!(actual, words[8..16]);
        let viewport = Bounds::new(point(px(20.), px(0.)), size(px(10.), px(10.)));
        let actual = visible_words(&words, viewport, px(10.), |offset| {
            Some(point(px(offset as f32), px(0.)))
        });
        assert_eq!(actual, words[4..6]);
    }

    #[test]
    fn spellcheck_visible_words_skip_large_folded_runs_without_linear_work() {
        let words = (0..10_001).map(|i| i * 5..i * 5 + 4).collect::<Vec<_>>();
        let mut calls = 0;
        let actual = visible_words(
            &words,
            Bounds::new(point(px(0.), px(0.)), size(px(100.), px(100.))),
            px(10.),
            |offset| {
                calls += 1;
                Some(point(px(offset.saturating_sub(50_000) as f32), px(0.)))
            },
        );
        assert_eq!(actual, [50_000..50_004]);
        assert!(
            calls < 100,
            "folded range must not be scanned word by word: {calls}"
        );
    }
}
