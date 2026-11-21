//! "Schedule task…": move open tasks from the current note to a daily or
//! weekly note.
//!
//! The text rewrites reuse the carry-over ones (`daily_carryover`), so a
//! scheduled task looks exactly like a carried one: a `- [ ] text ([[source]])`
//! line in the target and `- [x] text (moved to [[target]])` in the source.
//! Everything here is pure and tested without GPUI; the picker view at the
//! bottom owns only its own input state, and `app.rs` runs the transaction.

use crate::daily_carryover::{self, GoToDateView};
use crate::date_phrase;
use crate::settings::{Language, Period};
use chrono::{Duration, NaiveDate};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, WindowExt as _};
use gpui_kit::*;
use std::ops::Range;
use std::rc::Rc;

/// Where a scheduled task goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Today,
    Tomorrow,
    ThisWeek,
    NextWeek,
    Date(NaiveDate),
}

impl Choice {
    /// The picker's quick choices, in display order.
    pub const QUICK: [Choice; 4] = [
        Choice::Today,
        Choice::Tomorrow,
        Choice::ThisWeek,
        Choice::NextWeek,
    ];

    /// The periodic note this choice names and a date inside it.
    pub fn target(self, today: NaiveDate) -> (Period, NaiveDate) {
        match self {
            Choice::Today => (Period::Day, today),
            Choice::Tomorrow => (Period::Day, today + Duration::days(1)),
            Choice::ThisWeek => (Period::Week, today),
            Choice::NextWeek => (Period::Week, today + Duration::weeks(1)),
            Choice::Date(date) => (Period::Day, date),
        }
    }

    fn label(self, norwegian: bool) -> &'static str {
        match (self, norwegian) {
            (Choice::Today, false) => "Today",
            (Choice::Today, true) => "I dag",
            (Choice::Tomorrow, false) => "Tomorrow",
            (Choice::Tomorrow, true) => "I morgen",
            (Choice::ThisWeek, false) => "This week",
            (Choice::ThisWeek, true) => "Denne uken",
            (Choice::NextWeek, false) => "Next week",
            (Choice::NextWeek, true) => "Neste uke",
            (Choice::Date(_), false) => "Pick date…",
            (Choice::Date(_), true) => "Velg dato…",
        }
    }
}

/// The heading scheduled tasks go under.
pub fn heading(norwegian: bool) -> &'static str {
    if norwegian {
        "Planlagt"
    } else {
        "Scheduled"
    }
}

/// `note` with `lines` added at the end of its `## Scheduled` (`## Planlagt`)
/// section; the heading is appended when neither exists.
pub fn insert_scheduled(note: &str, lines: &[String], norwegian: bool) -> String {
    daily_carryover::insert_under(note, lines, &["Scheduled", "Planlagt"], heading(norwegian))
}

/// The open tasks on the lines a selection touches: `(1-based line, text)`.
/// A caret (empty selection) means its own line. A selection that ends at
/// the very start of a line does not include that line.
pub fn tasks_in_selection(text: &str, selection: Range<usize>) -> Vec<(usize, String)> {
    let start = selection.start.min(selection.end).min(text.len());
    let mut end = selection.start.max(selection.end).min(text.len());
    let bytes = text.as_bytes();
    if end > start && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    let line_of = |at: usize| bytes[..at].iter().filter(|b| **b == b'\n').count() + 1;
    let (first, last) = (line_of(start), line_of(end));
    crate::properties::open_task_lines(text)
        .into_iter()
        .filter(|(line, text)| (first..=last).contains(line) && !text.is_empty())
        .collect()
}

type OnPick = Rc<dyn Fn(Choice, &mut Window, &mut App)>;

/// The picker dialog body: the quick choices as rows, then the Go to date
/// input for "Pick date…".
pub struct ScheduleView {
    date: Entity<GoToDateView>,
    today: NaiveDate,
    language: Language,
    on_pick: OnPick,
}

impl ScheduleView {
    pub fn new(
        today: NaiveDate,
        language: Language,
        on_pick: impl Fn(Choice, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            date: cx.new(|cx| GoToDateView::new(today, language, window, cx)),
            today,
            language,
            on_pick: Rc::new(on_pick),
        }
    }

    pub fn focus(this: &Entity<Self>, window: &mut Window, cx: &mut App) {
        let date = this.read(cx).date.clone();
        GoToDateView::focus(&date, window, cx);
    }

    /// The typed date, once it reads as one (Enter in the dialog).
    pub fn typed(&self, cx: &App) -> Option<Choice> {
        self.date.read(cx).resolve(cx).map(Choice::Date)
    }
}

impl Render for ScheduleView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let norwegian = self.language == Language::Norwegian;
        let mut list = v_flex().w_full().gap_0p5();
        for (index, choice) in Choice::QUICK.into_iter().enumerate() {
            let (period, date) = choice.target(self.today);
            let hint = match period {
                Period::Day => date_phrase::label(date, norwegian),
                _ => String::new(),
            };
            let on_pick = self.on_pick.clone();
            list = list.child(
                h_flex()
                    .id(("schedule-choice", index))
                    .w_full()
                    .px_3()
                    .py_1()
                    .gap_3()
                    .justify_between()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.muted))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(choice.label(norwegian)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(hint),
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_pick(choice, window, cx);
                    }),
            );
        }
        v_flex()
            .w_full()
            .gap_2()
            .child(list)
            .child(
                div()
                    .px_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(Choice::Date(self.today).label(norwegian)),
            )
            .child(self.date.clone())
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the GPUI glob import there shadows `#[test]`.
    use super::{heading, insert_scheduled, tasks_in_selection, Choice};
    use crate::daily_carryover::{carried_line, mark_migrated};
    use crate::settings::{Period, Settings};
    use chrono::NaiveDate;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    // 2026-11-20 is a Friday.
    const TODAY: (i32, u32, u32) = (2026, 11, 20);

    fn today() -> NaiveDate {
        d(TODAY.0, TODAY.1, TODAY.2)
    }

    const NOTE: &str = "# Day\n- [ ] one\n- [x] done\n- [ ] two\nplain\n- [ ] three\n";

    #[test]
    fn a_caret_picks_only_its_own_task_line() {
        // Caret inside "one" (line 2) and inside "two" (line 4).
        assert_eq!(tasks_in_selection(NOTE, 9..9), vec![(2, "one".to_string())]);
        let two = NOTE.find("two").unwrap();
        assert_eq!(
            tasks_in_selection(NOTE, two..two),
            vec![(4, "two".to_string())]
        );
        // Not on a task line: a heading, a done task, plain text.
        assert!(tasks_in_selection(NOTE, 0..0).is_empty());
        let done = NOTE.find("done").unwrap();
        assert!(tasks_in_selection(NOTE, done..done).is_empty());
        let plain = NOTE.find("plain").unwrap();
        assert!(tasks_in_selection(NOTE, plain..plain).is_empty());
    }

    #[test]
    fn a_selection_picks_every_open_task_it_touches() {
        let one = NOTE.find("one").unwrap();
        let three = NOTE.find("three").unwrap();
        assert_eq!(
            tasks_in_selection(NOTE, one..three + 2),
            vec![
                (2, "one".to_string()),
                (4, "two".to_string()),
                (6, "three".to_string())
            ]
        );
        // A backwards selection reads the same.
        assert_eq!(
            tasks_in_selection(NOTE, three + 2..one),
            tasks_in_selection(NOTE, one..three + 2)
        );
    }

    #[test]
    fn a_selection_ending_at_a_line_start_leaves_that_line_out() {
        // "- [ ] one\n" fully selected: line 3 must not come along.
        let start = NOTE.find("- [ ] one").unwrap();
        let end = NOTE.find("- [x] done").unwrap();
        assert_eq!(
            tasks_in_selection(NOTE, start..end),
            vec![(2, "one".to_string())]
        );
        let two_line = NOTE.find("- [ ] two").unwrap();
        let plain_line = NOTE.find("plain").unwrap();
        assert_eq!(
            tasks_in_selection(NOTE, two_line..plain_line),
            vec![(4, "two".to_string())]
        );
    }

    #[test]
    fn task_detection_skips_fences_empty_tasks_and_handles_edges() {
        let note = "```\n- [ ] code\n```\n- [ ]\n- [ ] real";
        assert_eq!(
            tasks_in_selection(note, 0..note.len()),
            vec![(5, "real".to_string())]
        );
        assert!(tasks_in_selection("", 0..0).is_empty());
        // A range past the end is clamped, not a panic.
        assert_eq!(
            tasks_in_selection("- [ ] a", 3..99),
            vec![(1, "a".to_string())]
        );
    }

    #[test]
    fn each_choice_resolves_from_a_fixed_today() {
        assert_eq!(
            Choice::Today.target(today()),
            (Period::Day, d(2026, 11, 20))
        );
        assert_eq!(
            Choice::Tomorrow.target(today()),
            (Period::Day, d(2026, 11, 21))
        );
        assert_eq!(
            Choice::ThisWeek.target(today()),
            (Period::Week, d(2026, 11, 20))
        );
        assert_eq!(
            Choice::NextWeek.target(today()),
            (Period::Week, d(2026, 11, 27))
        );
        assert_eq!(
            Choice::Date(d(2027, 1, 5)).target(today()),
            (Period::Day, d(2027, 1, 5))
        );
        // Month and year ends roll over.
        assert_eq!(
            Choice::Tomorrow.target(d(2026, 12, 31)),
            (Period::Day, d(2027, 1, 1))
        );
    }

    #[test]
    fn weekly_targets_are_named_like_existing_weekly_notes() {
        let format = Settings::default().weekly_format;
        let name = |date: NaiveDate| {
            crate::bases::format_moment(date.and_hms_opt(0, 0, 0).unwrap(), &format)
        };
        let (period, this_week) = Choice::ThisWeek.target(today());
        assert_eq!(period, Period::Week);
        assert_eq!(name(this_week), "2026-W47");
        assert_eq!(name(Choice::NextWeek.target(today()).1), "2026-W48");
        // Across the year boundary the ISO week year wins: Mon 28 Dec 2026
        // is week 53 and the next Monday belongs to 2027-W01.
        assert_eq!(name(Choice::ThisWeek.target(d(2026, 12, 28)).1), "2026-W53");
        assert_eq!(name(Choice::NextWeek.target(d(2026, 12, 28)).1), "2027-W01");
        // Same name as the "Open weekly note" path for that day.
        assert_eq!(name(Period::Week.shift(today(), 1)), "2026-W48");
    }

    #[test]
    fn scheduled_text_matches_carry_over() {
        let line = carried_line("Call Ada", "2026-11-20");
        assert_eq!(line, "- [ ] Call Ada ([[2026-11-20]])");
        let target = insert_scheduled("# Week\n", &[line.clone()], false);
        assert_eq!(
            target,
            "# Week\n\n## Scheduled\n\n- [ ] Call Ada ([[2026-11-20]])\n"
        );
        let nb = insert_scheduled("# Uke\n", &[line.clone()], true);
        assert_eq!(
            nb,
            "# Uke\n\n## Planlagt\n\n- [ ] Call Ada ([[2026-11-20]])\n"
        );
        // An existing section in either language is reused.
        let again = insert_scheduled(&nb, &["- [ ] b".to_string()], false);
        assert_eq!(
            again,
            "# Uke\n\n## Planlagt\n\n- [ ] Call Ada ([[2026-11-20]])\n- [ ] b\n"
        );
        let source = mark_migrated(NOTE, &[(2, "one".to_string())], "2026-W47").unwrap();
        assert!(source.contains("- [x] one (moved to [[2026-W47]])\n"));
    }

    #[test]
    fn heading_follows_the_language() {
        assert_eq!(heading(false), "Scheduled");
        assert_eq!(heading(true), "Planlagt");
    }
}
