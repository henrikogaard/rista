//! Daily notes: carry unfinished tasks over to today, and "Go to date…".
//!
//! The text rewrites are pure so they test without GPUI. The two small
//! dialog views at the bottom own only their own input and checkbox state;
//! `app.rs` runs the filesystem transaction.
//!
//! Source tasks are marked `- [x] text (moved to [[today]])`, not `- [>]`:
//! the editor and preview only draw `[ ]`/`[x]` boxes, so a `[>]` line would
//! show as raw brackets, while `[x]` keeps the box and leaves the open-task
//! index (which reads only `[ ]`) correct.

use crate::date_phrase;
use crate::note_composer::{
    extract_reference_definitions, parse, rebase_markdown_links, ComposeError,
};
use crate::properties::FenceTracker;
use crate::settings::Language;
use chrono::{Duration, NaiveDate};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::*;
use markdown::mdast::Node;
use std::path::{Component, Path, PathBuf};

/// How many days before today are searched for unfinished tasks.
pub const LOOKBACK_DAYS: i64 = 7;

/// One open task found in an earlier daily note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub date: NaiveDate,
    pub path: PathBuf,
    /// 1-based line in the source note.
    pub line: usize,
    pub text: String,
}

/// The seven days before `today`, oldest first. Today is excluded.
pub fn lookback_dates(today: NaiveDate) -> Vec<NaiveDate> {
    (1..=LOOKBACK_DAYS)
        .rev()
        .map(|back| today - Duration::days(back))
        .collect()
}

/// Open tasks in `note` with something to carry: `(1-based line, text)`.
pub fn open_tasks(note: &str) -> Vec<(usize, String)> {
    crate::properties::open_task_lines(note)
        .into_iter()
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// The heading carried tasks go under.
pub fn heading(norwegian: bool) -> &'static str {
    if norwegian {
        "Overført"
    } else {
        "Carried over"
    }
}

/// `text` split before a trailing block anchor: `("Call Ada", " ^call")`.
/// The anchor part is empty when there is none.
fn split_block_anchor(text: &str) -> (&str, &str) {
    let trimmed = text.trim_end();
    if let Some(at) = trimmed.rfind(" ^") {
        let id = &trimmed[at + 2..];
        if !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return (&trimmed[..at], &trimmed[at..]);
        }
    }
    (text, "")
}

/// `- [ ] text ([[2026-11-18]])` — the task and a link back to its source.
/// A trailing block anchor stays with the source line, so the copy drops it.
pub fn carried_line(text: &str, source_link: &str) -> String {
    let (text, _) = split_block_anchor(text);
    format!("- [ ] {text} ([[{source_link}]])")
}

/// A task ready to copy into today's note.
#[derive(Debug, PartialEq, Eq)]
pub struct Prepared {
    pub text: String,
    /// Reference-style link definitions the text needs in today's note.
    pub definitions: Vec<String>,
}

/// The task on 1-based `line` of `source`, with its local links rebased for
/// `today_path` and the reference definitions it uses brought along. A link
/// that cannot be rebased stays as written; ambiguous definitions fail.
pub fn prepare_task(
    source: &str,
    line: usize,
    source_path: &Path,
    today_path: &Path,
    root: &Path,
) -> Result<Prepared, ComposeError> {
    let mut offset = 0;
    let mut found = None;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        if index + 1 == line {
            let body = segment.trim_end_matches(['\n', '\r']);
            let lead = body.len() - body.trim_start().len();
            found = Some((offset + lead)..(offset + body.len()));
            break;
        }
        offset += segment.len();
    }
    let range = found.ok_or(ComposeError::InvalidMarkdown)?;
    let (with_definitions, _) =
        extract_reference_definitions(source, range.clone(), &source[range])?;
    let same_dir = source_path.parent() == today_path.parent();
    let rebased = if same_dir {
        with_definitions
    } else {
        rebase_markdown_links(&with_definitions, source_path, today_path, root)
            .unwrap_or(with_definitions)
    };
    let (task, definitions) = rebased
        .split_once("\n\n")
        .map_or((rebased.as_str(), ""), |(task, rest)| (task, rest));
    let task = if same_dir {
        task.to_string()
    } else {
        rebase_relative_wikilinks(task, source_path, root)
    };
    let text = task
        .split_once("[ ]")
        .map_or(task.as_str(), |(_, text)| text)
        .trim()
        .to_string();
    Ok(Prepared {
        text,
        definitions: definitions
            .split("\n\n")
            .filter(|definition| !definition.trim().is_empty())
            .map(|definition| definition.trim_end().to_string())
            .collect(),
    })
}

/// `[[./plan]]` and `[[../plan]]` mean "next to this note", so they are
/// rewritten to vault-relative paths before the text moves to another folder.
fn rebase_relative_wikilinks(text: &str, source_path: &Path, root: &Path) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(['`', '[']) {
        let (before, tail) = rest.split_at(start);
        out.push_str(before);
        if tail.starts_with('`') {
            // A code span is copied as written; an unmatched run is literal.
            let run = tail.len() - tail.trim_start_matches('`').len();
            let end = code_span_end(tail, run).unwrap_or(run);
            out.push_str(&tail[..end]);
            rest = &tail[end..];
            continue;
        }
        let Some(end) = tail.find("]]").filter(|_| tail.starts_with("[[")) else {
            out.push('[');
            rest = &tail[1..];
            continue;
        };
        let inner = &tail[2..end];
        let (target, suffix) = inner.split_at(inner.find(['#', '|']).unwrap_or(inner.len()));
        let rebased = (target.starts_with("./") || target.starts_with("../"))
            .then(|| vault_relative(source_path.parent()?, target, root))
            .flatten();
        match rebased {
            Some(path) => out.push_str(&format!("[[{path}{suffix}]]")),
            None => out.push_str(&tail[..end + 2]),
        }
        rest = &tail[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Where the code span opened by the `run` backticks at the start of `tail`
/// ends: after the next backtick run of exactly the same length.
fn code_span_end(tail: &str, run: usize) -> Option<usize> {
    let mut at = run;
    while let Some(offset) = tail[at..].find('`') {
        let start = at + offset;
        let len = tail[start..].len() - tail[start..].trim_start_matches('`').len();
        if len == run {
            return Some(start + len);
        }
        at = start + len;
    }
    None
}

/// `target` resolved from `dir` as a path under `root`, with `/` separators.
fn vault_relative(dir: &Path, target: &str, root: &Path) -> Option<String> {
    let mut parts = dir
        .strip_prefix(root)
        .ok()?
        .components()
        .collect::<Vec<_>>();
    for part in Path::new(target).components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Normal(name) => parts.push(Component::Normal(name)),
            _ => return None,
        }
    }
    let path = parts
        .iter()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    (!path.is_empty()).then_some(path)
}

/// The reference and footnote definitions in `text`, as a normalized label
/// (lowercase, whitespace collapsed) and what it points to: URL and title for
/// a link, the body for a footnote. `None` when `text` does not parse.
fn definitions_in(text: &str) -> Option<Vec<(String, String)>> {
    fn collapse(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
    fn walk(node: &Node, text: &str, found: &mut Vec<(String, String)>) {
        match node {
            Node::Definition(definition) => {
                let label = definition
                    .label
                    .as_deref()
                    .unwrap_or(&definition.identifier);
                found.push((
                    collapse(label).to_lowercase(),
                    format!(
                        "{}\n{}",
                        definition.url,
                        definition.title.as_deref().unwrap_or_default()
                    ),
                ));
            }
            Node::FootnoteDefinition(definition) => {
                let label = definition
                    .label
                    .as_deref()
                    .unwrap_or(&definition.identifier);
                let body = node
                    .position()
                    .and_then(|at| text.get(at.start.offset..at.end.offset))
                    .and_then(|source| source.split_once("]:"))
                    .map(|(_, body)| collapse(body))
                    .unwrap_or_default();
                found.push((format!("footnote:{}", collapse(label).to_lowercase()), body));
            }
            _ => {}
        }
        for child in node.children().into_iter().flatten() {
            walk(child, text, found);
        }
    }
    let root = parse(text).ok()?;
    let mut found = Vec::new();
    walk(&root, text, &mut found);
    Some(found)
}

/// `lines` followed by `definitions` (blank-line separated, identical ones
/// once), ready for `insert_carried`. A definition `today` already has is not
/// added again. `None` when two different definitions share a label, because
/// one would silently take over the other's links.
pub fn with_definitions(
    mut lines: Vec<String>,
    definitions: Vec<String>,
    today: &str,
) -> Option<Vec<String>> {
    let existing = definitions_in(today)?;
    let mut seen: Vec<(String, String)> = Vec::new();
    for definition in definitions {
        let (label, target) = definitions_in(&definition)?.into_iter().next()?;
        if let Some((_, known)) = existing.iter().find(|(known, _)| *known == label) {
            if *known == target {
                continue;
            }
            return None;
        }
        match seen.iter().find(|(known, _)| *known == label) {
            Some((_, text)) if *text == definition => {}
            Some(_) => return None,
            None => seen.push((label, definition)),
        }
    }
    for (_, definition) in seen {
        lines.push(String::new());
        lines.push(definition);
    }
    Some(lines)
}

/// `note` with `lines` added at the end of the carried-over section. An
/// existing heading (either language) is reused; otherwise one is appended.
pub fn insert_carried(note: &str, lines: &[String], norwegian: bool) -> String {
    insert_under(
        note,
        lines,
        &["Carried over", "Overført"],
        heading(norwegian),
    )
}

/// `note` with `lines` added at the end of the level-2 section named by any
/// of `names`; `new_heading` is appended as a new section when none exists.
pub fn insert_under(note: &str, lines: &[String], names: &[&str], new_heading: &str) -> String {
    let nl = if note.contains("\r\n") { "\r\n" } else { "\n" };
    let block = lines
        .iter()
        .map(|line| format!("{line}{nl}"))
        .collect::<String>();
    let mut offset = 0;
    let segments = note
        .split_inclusive('\n')
        .map(|segment| {
            let start = offset;
            offset += segment.len();
            (start, segment)
        })
        .collect::<Vec<_>>();
    // A line inside a fenced code block is never a heading.
    let mut fences = FenceTracker::default();
    let fenced = segments
        .iter()
        .map(|(_, segment)| fences.in_fence(segment))
        .collect::<Vec<_>>();
    let is_heading = |segment: &str| {
        let line = segment.trim_end();
        line.strip_prefix("## ")
            .is_some_and(|name| names.contains(&name))
    };
    let Some(found) = segments
        .iter()
        .enumerate()
        .position(|(i, (_, s))| !fenced[i] && is_heading(s))
    else {
        let mut out = note.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str(nl);
        }
        if !out.is_empty() && !out.ends_with("\n\n") && !out.ends_with("\r\n\r\n") {
            out.push_str(nl);
        }
        out.push_str(&format!("## {new_heading}{nl}{nl}"));
        out.push_str(&block);
        return out;
    };
    // The section ends at the next level 1–2 heading; add after its last
    // non-blank line so trailing blank lines stay below the new tasks.
    let mut last = found;
    for (index, (_, segment)) in segments.iter().enumerate().skip(found + 1) {
        if !fenced[index] && (segment.starts_with("# ") || segment.starts_with("## ")) {
            break;
        }
        if !segment.trim().is_empty() {
            last = index;
        }
    }
    let (start, segment) = segments[last];
    let at = start + segment.len();
    let mut out = note[..at].to_string();
    if !segment.ends_with('\n') {
        out.push_str(nl);
    }
    if last == found {
        out.push_str(nl);
    }
    out.push_str(&block);
    out.push_str(&note[at..]);
    out
}

/// `note` with each task in `tasks` marked moved to `moved_link`. `None`
/// when a task is no longer an open task with that text on that line, so a
/// note edited since the scan is never rewritten blindly.
pub fn mark_migrated(note: &str, tasks: &[(usize, String)], moved_link: &str) -> Option<String> {
    let current = crate::properties::open_task_lines(note);
    if !tasks.iter().all(|task| current.contains(task)) {
        return None;
    }
    let mut out = String::with_capacity(note.len() + tasks.len() * 32);
    for (index, segment) in note.split_inclusive('\n').enumerate() {
        if !tasks.iter().any(|(line, _)| *line == index + 1) {
            out.push_str(segment);
            continue;
        }
        let body = segment.trim_end_matches(['\n', '\r']);
        let eol = &segment[body.len()..];
        // The notice goes before a trailing `^id`, which only resolves at the end.
        let marked = body.trim_end().replacen("[ ]", "[x]", 1);
        let (head, anchor) = split_block_anchor(&marked);
        out.push_str(&format!("{head} (moved to [[{moved_link}]]){anchor}{eol}"));
    }
    Some(out)
}

struct Row {
    candidate: Candidate,
    selected: bool,
}

/// The carry-over dialog body: tasks grouped by source date, all selected.
pub struct CarryoverView {
    rows: Vec<Row>,
    language: Language,
}

impl CarryoverView {
    pub fn new(candidates: Vec<Candidate>, language: Language) -> Self {
        Self {
            rows: candidates
                .into_iter()
                .map(|candidate| Row {
                    candidate,
                    selected: true,
                })
                .collect(),
            language,
        }
    }

    pub fn selected(&self) -> Vec<Candidate> {
        self.rows
            .iter()
            .filter(|row| row.selected)
            .map(|row| row.candidate.clone())
            .collect()
    }
}

impl Render for CarryoverView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let norwegian = self.language == Language::Norwegian;
        let mut list = v_flex().w_full().py_1().gap_0p5();
        let mut group = None;
        for (index, row) in self.rows.iter().enumerate() {
            if group != Some(row.candidate.date) {
                group = Some(row.candidate.date);
                list = list.child(
                    div()
                        .px_3()
                        .pt_2()
                        .pb_0p5()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(date_phrase::label(row.candidate.date, norwegian)),
                );
            }
            list = list.child(
                div().px_3().py_0p5().child(
                    Checkbox::new(("carry-task", index))
                        .checked(row.selected)
                        .label(row.candidate.text.clone())
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            this.rows[index].selected = *checked;
                            cx.notify();
                        })),
                ),
            );
        }
        v_flex()
            .w_full()
            .child(ScrollableElement::overflow_y_scrollbar(
                list.max_h(px(400.)),
            ))
    }
}

/// The "Go to date…" dialog body: an input with the resolved date, or a
/// muted note when the text isn't a date, under it.
pub struct GoToDateView {
    input: Entity<InputState>,
    today: NaiveDate,
    language: Language,
    _change: Subscription,
}

impl GoToDateView {
    pub fn new(
        today: NaiveDate,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(language.text(
                "2026-11-18, next friday, 3 days ago…",
                "2026-11-18, neste fredag, for 3 dager siden…",
            ))
        });
        let change = cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            input,
            today,
            language,
            _change: change,
        }
    }

    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let input = this.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// The date the input names right now.
    pub fn resolve(&self, cx: &App) -> Option<NaiveDate> {
        date_phrase::parse(self.input.read(cx).value().as_ref(), self.today)
    }
}

impl Render for GoToDateView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let typed = !self.input.read(cx).value().trim().is_empty();
        let note = match (self.resolve(cx), typed) {
            (Some(date), _) => div().text_color(theme.foreground).child(date_phrase::label(
                date,
                self.language == Language::Norwegian,
            )),
            (None, true) => div().text_color(theme.muted_foreground).child(
                self.language
                    .text("Not a date I can read", "Kan ikke lese dette som en dato"),
            ),
            (None, false) => div(),
        };
        v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .child(Input::new(&self.input).appearance(true)),
            )
            .child(h_flex().px_1().h_4().text_xs().child(note))
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the GPUI glob import there shadows `#[test]`.
    use super::{
        carried_line, heading, insert_carried, lookback_dates, mark_migrated, open_tasks,
        prepare_task, rebase_relative_wikilinks, with_definitions,
    };
    use chrono::NaiveDate;
    use std::path::PathBuf;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn lookback_is_the_seven_days_before_today() {
        let dates = lookback_dates(d(2026, 11, 20));
        assert_eq!(dates.len(), 7);
        assert_eq!(dates[0], d(2026, 11, 13));
        assert_eq!(dates[6], d(2026, 11, 19));
        assert!(!dates.contains(&d(2026, 11, 20)));
        assert_eq!(lookback_dates(d(2027, 1, 3))[0], d(2026, 12, 27));
    }

    #[test]
    fn open_tasks_skip_done_empty_and_fenced_tasks() {
        let note = "# Day\n- [ ] one\n- [x] done\n- [ ]\n```\n- [ ] code\n```\n  - [ ] nested\n";
        assert_eq!(
            open_tasks(note),
            vec![(2, "one".to_string()), (8, "nested".to_string())]
        );
    }

    #[test]
    fn carried_line_links_back() {
        assert_eq!(
            carried_line("Call Ada", "2026-11-18"),
            "- [ ] Call Ada ([[2026-11-18]])"
        );
    }

    #[test]
    fn insert_appends_a_new_heading_after_a_blank_line() {
        let lines = vec![carried_line("a", "2026-11-18")];
        assert_eq!(
            insert_carried("# Today\n\nNotes\n", &lines, false),
            "# Today\n\nNotes\n\n## Carried over\n\n- [ ] a ([[2026-11-18]])\n"
        );
        assert_eq!(
            insert_carried("# Today", &lines, true),
            "# Today\n\n## Overført\n\n- [ ] a ([[2026-11-18]])\n"
        );
        assert_eq!(
            insert_carried("", &lines, false),
            "## Carried over\n\n- [ ] a ([[2026-11-18]])\n"
        );
    }

    #[test]
    fn insert_reuses_an_existing_heading_in_either_language() {
        let lines = vec!["- [ ] b".to_string()];
        let note = "# Today\n\n## Carried over\n\n- [ ] a\n\n## Notes\ntext\n";
        assert_eq!(
            insert_carried(note, &lines, true),
            "# Today\n\n## Carried over\n\n- [ ] a\n- [ ] b\n\n## Notes\ntext\n"
        );
        let nb = "## Overført\n- [ ] a";
        assert_eq!(
            insert_carried(nb, &lines, false),
            "## Overført\n- [ ] a\n- [ ] b\n"
        );
    }

    #[test]
    fn insert_into_an_empty_existing_section_leaves_a_blank_line() {
        let lines = vec!["- [ ] b".to_string()];
        assert_eq!(
            insert_carried("## Carried over\n## Next\n", &lines, false),
            "## Carried over\n\n- [ ] b\n## Next\n"
        );
    }

    #[test]
    fn insert_ignores_headings_inside_fences() {
        let lines = vec!["- [ ] b".to_string()];
        let note = "## Carried over\n\n- [ ] a\n```markdown\n# Example\n```\n\n## Next\n";
        assert_eq!(
            insert_carried(note, &lines, false),
            "## Carried over\n\n- [ ] a\n```markdown\n# Example\n```\n- [ ] b\n\n## Next\n"
        );
        let fenced_heading = "```\n## Carried over\n```\n";
        let out = insert_carried(fenced_heading, &lines, false);
        assert!(out.starts_with(fenced_heading));
        assert!(out.ends_with("## Carried over\n\n- [ ] b\n"));
    }

    #[test]
    fn insert_tracks_fence_length_and_info_strings() {
        let lines = vec!["- [ ] a".to_string()];
        let note = "````\n```\n## Carried over\n````\n\n## Carried over\n\n- [ ] b\n";
        assert_eq!(
            insert_carried(note, &lines, false),
            "````\n```\n## Carried over\n````\n\n## Carried over\n\n- [ ] b\n- [ ] a\n"
        );
        // A line with an info string never closes the fence.
        let open = "```rust\n```js\n## Carried over\n```\n";
        assert!(insert_carried(open, &lines, false).ends_with("\n## Carried over\n\n- [ ] a\n"));
        assert!(insert_carried(open, &lines, false).starts_with(open));
    }

    #[test]
    fn insert_keeps_crlf_endings() {
        let lines = vec!["- [ ] a".to_string()];
        assert_eq!(
            insert_carried("# T\r\n", &lines, false),
            "# T\r\n\r\n## Carried over\r\n\r\n- [ ] a\r\n"
        );
    }

    #[test]
    fn migrate_marks_only_the_chosen_lines() {
        let note = "- [ ] one\n- [ ] two\n- [x] three\n";
        let marked = mark_migrated(note, &[(2, "two".to_string())], "2026-11-20").unwrap();
        assert_eq!(
            marked,
            "- [ ] one\n- [x] two (moved to [[2026-11-20]])\n- [x] three\n"
        );
        assert!(open_tasks(&marked).iter().all(|(_, text)| text != "two"));
    }

    #[test]
    fn block_anchors_stay_last_on_the_source_and_leave_the_copy() {
        let tasks = [(1, "Call Ada ^call".to_string())];
        let marked = mark_migrated("- [ ] Call Ada ^call\n", &tasks, "t").unwrap();
        assert_eq!(marked, "- [x] Call Ada (moved to [[t]]) ^call\n");
        assert!(crate::preview::block_lines(&marked.lines().collect::<Vec<_>>(), "call").is_some());
        assert_eq!(
            carried_line("Call Ada ^call", "s"),
            "- [ ] Call Ada ([[s]])"
        );
        assert_eq!(carried_line("a^b c", "s"), "- [ ] a^b c ([[s]])");
    }

    #[test]
    fn migrate_handles_indent_crlf_and_a_missing_final_newline() {
        let note = "  - [ ] a\r\n- [ ] b";
        let tasks = [(1, "a".to_string()), (2, "b".to_string())];
        assert_eq!(
            mark_migrated(note, &tasks, "t").unwrap(),
            "  - [x] a (moved to [[t]])\r\n- [x] b (moved to [[t]])"
        );
    }

    #[test]
    fn migrate_refuses_a_note_that_changed() {
        let note = "- [ ] one\n";
        assert_eq!(mark_migrated(note, &[(1, "other".to_string())], "t"), None);
        assert_eq!(mark_migrated(note, &[(2, "one".to_string())], "t"), None);
        assert_eq!(
            mark_migrated("- [x] one\n", &[(1, "one".to_string())], "t"),
            None
        );
    }

    fn vault(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("rista-carry-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("2026/10")).unwrap();
        std::fs::create_dir_all(root.join("2026/11")).unwrap();
        std::fs::write(root.join("2026/10/plan.md"), "# Plan\n").unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn prepare_rebases_links_across_folders() {
        let root = vault("rebase");
        let from = root.join("2026/10/31.md");
        let to = root.join("2026/11/01.md");
        let source = "- [ ] See [plan](./plan.md) and [[./plan]] [[../x|y]] `[[./plan]]`\n";
        let prepared = prepare_task(source, 1, &from, &to, &root).unwrap();
        assert_eq!(
            prepared.text,
            "See [plan](../10/plan.md) and [[2026/10/plan]] [[2026/x|y]] `[[./plan]]`"
        );
        assert!(prepared.definitions.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rebase_keeps_text_before_an_unclosed_wikilink_once() {
        let root = PathBuf::from("/v");
        let from = root.join("2026/10/31.md");
        let text = "see [[./plan]] then [[./plan";
        assert_eq!(
            rebase_relative_wikilinks(text, &from, &root),
            "see [[2026/10/plan]] then [[./plan"
        );
    }

    #[test]
    fn rebase_skips_code_spans_of_any_backtick_length() {
        let root = PathBuf::from("/v");
        let from = root.join("2026/10/31.md");
        let text = "``a ` [[./plan]] b`` [[./plan]] `x [[./plan]]";
        assert_eq!(
            rebase_relative_wikilinks(text, &from, &root),
            "``a ` [[./plan]] b`` [[2026/10/plan]] `x [[2026/10/plan]]"
        );
    }

    #[test]
    fn prepare_leaves_same_folder_links_and_brings_definitions() {
        let root = vault("defs");
        let from = root.join("2026/10/30.md");
        let to = root.join("2026/10/31.md");
        let source = "- [ ] Read [plan][p] [[./plan]]\n\n[p]: ./plan.md\n";
        let prepared = prepare_task(source, 1, &from, &to, &root).unwrap();
        assert_eq!(prepared.text, "Read [plan][p] [[./plan]]");
        assert_eq!(prepared.definitions, vec!["[p]: ./plan.md".to_string()]);
        let other = root.join("2026/11/01.md");
        let moved = prepare_task(source, 1, &from, &other, &root).unwrap();
        assert_eq!(moved.definitions, vec!["[p]: ../10/plan.md".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn definitions_dedupe_and_refuse_conflicts() {
        let lines = vec!["- [ ] a".to_string()];
        let same = vec!["[p]: /a".to_string(), "[P]: /a".to_string()];
        assert_eq!(
            with_definitions(lines.clone(), vec!["[p]: /a".into(), "[p]: /a".into()], ""),
            Some(vec!["- [ ] a".into(), String::new(), "[p]: /a".into()])
        );
        assert_eq!(with_definitions(lines.clone(), same, ""), None);
        assert_eq!(
            with_definitions(lines, vec!["[p]: /a".into(), "[p]: /b".into()], ""),
            None
        );
    }

    #[test]
    fn definitions_check_the_destination_note() {
        let lines = vec!["- [ ] a".to_string()];
        let same = Some(lines.clone());
        let carried = vec!["[p]: /a".to_string()];
        assert_eq!(
            with_definitions(lines.clone(), carried.clone(), "[P]:  /a\n"),
            same
        );
        assert_eq!(
            with_definitions(lines.clone(), carried.clone(), "[p]: /b\n"),
            None
        );
        // A definition inside a fence is not one.
        let kept = Some(vec!["- [ ] a".into(), String::new(), "[p]: /a".into()]);
        let fenced = "```\n[p]: /b\n```\n";
        assert_eq!(
            with_definitions(lines.clone(), carried.clone(), fenced),
            kept
        );
        // Neither is an indented code block.
        assert_eq!(
            with_definitions(lines.clone(), carried, "    [p]: /b\n"),
            kept
        );
        // A title on the next line is part of the same definition.
        let titled = vec!["[p]: /a\n  \"Title\"".to_string()];
        assert_eq!(
            with_definitions(lines.clone(), titled.clone(), "[p]: /a\n  \"Title\"\n"),
            same
        );
        assert_eq!(
            with_definitions(lines, titled, "[p]: /a\n  \"Other\"\n"),
            None
        );
    }

    #[test]
    fn heading_follows_the_language() {
        assert_eq!(heading(false), "Carried over");
        assert_eq!(heading(true), "Overført");
    }
}
