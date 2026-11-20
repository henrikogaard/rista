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
use crate::settings::Language;
use chrono::{Duration, NaiveDate};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme};
use gpui_kit::*;
use std::path::PathBuf;

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

/// `note` with `lines` added at the end of the carried-over section. An
/// existing heading (either language) is reused; otherwise one is appended.
pub fn insert_carried(note: &str, lines: &[String], norwegian: bool) -> String {
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
    let mut fenced = Vec::with_capacity(segments.len());
    let mut fence: Option<&str> = None;
    for (_, segment) in &segments {
        let trimmed = segment.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m));
        match (fence, marker) {
            (None, Some(m)) => {
                fence = Some(m);
                fenced.push(true);
            }
            (Some(open), Some(m)) if open == m => {
                fence = None;
                fenced.push(true);
            }
            _ => fenced.push(fence.is_some()),
        }
    }
    let is_heading = |segment: &str| {
        let line = segment.trim_end();
        line == "## Carried over" || line == "## Overført"
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
        out.push_str(&format!("## {}{nl}{nl}", heading(norwegian)));
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
    use super::{carried_line, heading, insert_carried, lookback_dates, mark_migrated, open_tasks};
    use chrono::NaiveDate;

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

    #[test]
    fn heading_follows_the_language() {
        assert_eq!(heading(false), "Carried over");
        assert_eq!(heading(true), "Overført");
    }
}
