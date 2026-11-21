//! "Go to date…": turn a typed phrase into a calendar date.
//!
//! [`parse`] is pure: today is an argument, so every phrase is testable
//! without a clock. English and Norwegian phrases are accepted side by side.
//!
//! Weekday rules: a bare weekday (`monday`) is the first such day on or after
//! today; `next monday` is the first one strictly after today; `last monday`
//! is the last one strictly before today.

use chrono::{Datelike, Duration, Months, NaiveDate, Weekday};

/// Largest `N` in "N days ago" — keeps date arithmetic far from overflow.
const MAX_OFFSET: u32 = 100_000;

const WEEKDAYS: [(&str, Weekday); 15] = [
    ("monday", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sunday", Weekday::Sun),
    ("mandag", Weekday::Mon),
    ("tirsdag", Weekday::Tue),
    ("onsdag", Weekday::Wed),
    ("torsdag", Weekday::Thu),
    ("fredag", Weekday::Fri),
    ("lørdag", Weekday::Sat),
    ("søndag", Weekday::Sun),
    ("sondag", Weekday::Sun),
];

#[derive(Clone, Copy)]
enum Unit {
    Day,
    Week,
    Month,
    Year,
}

const UNITS: [(&str, Unit); 15] = [
    ("day", Unit::Day),
    ("days", Unit::Day),
    ("week", Unit::Week),
    ("weeks", Unit::Week),
    ("month", Unit::Month),
    ("months", Unit::Month),
    ("year", Unit::Year),
    ("years", Unit::Year),
    ("dag", Unit::Day),
    ("dager", Unit::Day),
    ("uke", Unit::Week),
    ("uker", Unit::Week),
    ("måned", Unit::Month),
    ("måneder", Unit::Month),
    ("år", Unit::Year),
];

/// The date `input` names, relative to `today`, or `None` when it isn't a
/// phrase we know.
pub fn parse(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    // Four-digit years only: a daily note for year 102026 is a typo.
    resolve(input, today).filter(|date| (1000..=9999).contains(&date.year()))
}

fn resolve(input: &str, today: NaiveDate) -> Option<NaiveDate> {
    let words = input
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    let words = words.iter().map(String::as_str).collect::<Vec<_>>();
    match words.as_slice() {
        [] => None,
        ["today" | "idag"] | ["i", "dag"] => Some(today),
        ["yesterday" | "igår"] | ["i", "går"] => today.checked_sub_signed(Duration::days(1)),
        ["tomorrow" | "imorgen"] | ["i", "morgen"] => today.checked_add_signed(Duration::days(1)),
        // A week names no single day. Like the Schedule dialog's "This week"
        // and "Next week" rows, it stands for today and today + 7 days; the
        // Schedule dialog turns these phrases into its weekly-note choices.
        [..] if week_offset_words(&words).is_some() => {
            let weeks = week_offset_words(&words)?;
            today.checked_add_signed(Duration::weeks(weeks))
        }
        [iso] if iso.contains('-') => iso_date(iso),
        [day] => weekday(day).and_then(|day| nearest(today, day, Direction::OnOrAfter)),
        ["next" | "neste", day] => {
            weekday(day).and_then(|day| nearest(today, day, Direction::After))
        }
        ["last" | "forrige", day] => {
            weekday(day).and_then(|day| nearest(today, day, Direction::Before))
        }
        ["in" | "om", n, unit] => shift(today, n, unit, 1),
        [n, unit, "ago"] => shift(today, n, unit, -1),
        ["for", n, unit, "siden"] => shift(today, n, unit, -1),
        _ => None,
    }
}

/// Weeks from the current one for `this week` / `next week` (and the
/// Norwegian forms): 0 or 1. `None` for any other text.
pub fn week_offset(input: &str) -> Option<i64> {
    let words = input
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>();
    week_offset_words(&words.iter().map(String::as_str).collect::<Vec<_>>())
}

fn week_offset_words(words: &[&str]) -> Option<i64> {
    match words {
        ["this", "week"] | ["denne", "uka" | "uken"] => Some(0),
        ["next", "week"] | ["neste", "uke"] => Some(1),
        _ => None,
    }
}

/// `2026-11-18` — four-digit year, so `18-11` never half-parses.
fn iso_date(text: &str) -> Option<NaiveDate> {
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || year.len() != 4
        || !(1..=2).contains(&month.len())
        || !(1..=2).contains(&day.len())
        || ![year, month, day]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)
}

fn weekday(word: &str) -> Option<Weekday> {
    WEEKDAYS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, day)| *day)
}

#[derive(Clone, Copy)]
enum Direction {
    OnOrAfter,
    After,
    Before,
}

fn nearest(today: NaiveDate, target: Weekday, direction: Direction) -> Option<NaiveDate> {
    let today_index = today.weekday().num_days_from_monday() as i64;
    let target_index = target.num_days_from_monday() as i64;
    let forward = (target_index - today_index).rem_euclid(7);
    let days = match direction {
        Direction::OnOrAfter => forward,
        Direction::After if forward == 0 => 7,
        Direction::After => forward,
        Direction::Before if forward == 0 => -7,
        Direction::Before => forward - 7,
    };
    today.checked_add_signed(Duration::days(days))
}

/// `today` moved `sign * n` units. Months and years clamp to the end of a
/// shorter month, like [`crate::settings::Period::shift`].
fn shift(today: NaiveDate, n: &str, unit: &str, sign: i64) -> Option<NaiveDate> {
    let n = n.parse::<u32>().ok().filter(|n| *n <= MAX_OFFSET)?;
    let unit = UNITS.iter().find(|(name, _)| *name == unit)?.1;
    let months = |count: u32| {
        if sign > 0 {
            today.checked_add_months(Months::new(count))
        } else {
            today.checked_sub_months(Months::new(count))
        }
    };
    match unit {
        Unit::Day => today.checked_add_signed(Duration::days(sign * n as i64)),
        Unit::Week => today.checked_add_signed(Duration::weeks(sign * n as i64)),
        Unit::Month => months(n),
        Unit::Year => months(n.checked_mul(12)?),
    }
}

/// `Wednesday 18 November 2026` / `onsdag 18. november 2026` — the preview
/// line under the input.
pub fn label(date: NaiveDate, norwegian: bool) -> String {
    const EN_DAYS: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const NB_DAYS: [&str; 7] = [
        "mandag", "tirsdag", "onsdag", "torsdag", "fredag", "lørdag", "søndag",
    ];
    const EN_MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const NB_MONTHS: [&str; 12] = [
        "januar",
        "februar",
        "mars",
        "april",
        "mai",
        "juni",
        "juli",
        "august",
        "september",
        "oktober",
        "november",
        "desember",
    ];
    let weekday = date.weekday().num_days_from_monday() as usize;
    let month = date.month0() as usize;
    if norwegian {
        format!(
            "{} {}. {} {}",
            NB_DAYS[weekday],
            date.day(),
            NB_MONTHS[month],
            date.year()
        )
    } else {
        format!(
            "{} {} {} {}",
            EN_DAYS[weekday],
            date.day(),
            EN_MONTHS[month],
            date.year()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Friday 20 November 2026.
    fn today() -> NaiveDate {
        d(2026, 11, 20)
    }

    fn p(text: &str) -> Option<NaiveDate> {
        parse(text, today())
    }

    #[test]
    fn fixture_is_a_friday() {
        assert_eq!(today().weekday(), Weekday::Fri);
    }

    #[test]
    fn iso_dates() {
        assert_eq!(p("2026-11-18"), Some(d(2026, 11, 18)));
        assert_eq!(p(" 2026-01-05 "), Some(d(2026, 1, 5)));
        assert_eq!(p("2026-1-5"), Some(d(2026, 1, 5)));
        assert_eq!(p("2024-02-29"), Some(d(2024, 2, 29)));
    }

    #[test]
    fn invalid_iso_dates() {
        assert_eq!(p("2026-02-30"), None);
        assert_eq!(p("2026-13-01"), None);
        assert_eq!(p("2026-11"), None);
        assert_eq!(p("26-11-18"), None);
        assert_eq!(p("2026-11-18-01"), None);
        assert_eq!(p("2026-aa-18"), None);
        assert_eq!(p("2026-+1-18"), None);
    }

    #[test]
    fn relative_days_in_english() {
        assert_eq!(p("today"), Some(today()));
        assert_eq!(p("yesterday"), Some(d(2026, 11, 19)));
        assert_eq!(p("tomorrow"), Some(d(2026, 11, 21)));
        assert_eq!(p("  TODAY "), Some(today()));
        assert_eq!(p("Tomorrow"), Some(d(2026, 11, 21)));
    }

    #[test]
    fn relative_days_in_norwegian() {
        assert_eq!(p("i dag"), Some(today()));
        assert_eq!(p("idag"), Some(today()));
        assert_eq!(p("i går"), Some(d(2026, 11, 19)));
        assert_eq!(p("igår"), Some(d(2026, 11, 19)));
        assert_eq!(p("i morgen"), Some(d(2026, 11, 21)));
        assert_eq!(p("imorgen"), Some(d(2026, 11, 21)));
        assert_eq!(p("I   Morgen"), Some(d(2026, 11, 21)));
    }

    #[test]
    fn week_phrases_match_the_schedule_rows() {
        assert_eq!(p("this week"), Some(today()));
        assert_eq!(p("denne uka"), Some(today()));
        assert_eq!(p("denne uken"), Some(today()));
        assert_eq!(p("next week"), Some(d(2026, 11, 27)));
        assert_eq!(p("Neste  Uke"), Some(d(2026, 11, 27)));
        assert_eq!(week_offset("this week"), Some(0));
        assert_eq!(week_offset("denne uka"), Some(0));
        assert_eq!(week_offset("next week"), Some(1));
        assert_eq!(week_offset("neste uke"), Some(1));
        assert_eq!(week_offset("next friday"), None);
        assert_eq!(p("next weeks"), None);
    }

    #[test]
    fn bare_weekday_is_on_or_after_today() {
        assert_eq!(p("monday"), Some(d(2026, 11, 23)));
        assert_eq!(p("friday"), Some(today()));
        assert_eq!(p("saturday"), Some(d(2026, 11, 21)));
        assert_eq!(p("thursday"), Some(d(2026, 11, 26)));
        assert_eq!(p("mandag"), Some(d(2026, 11, 23)));
        assert_eq!(p("søndag"), Some(d(2026, 11, 22)));
        assert_eq!(p("sondag"), Some(d(2026, 11, 22)));
        assert_eq!(p("Fredag"), Some(today()));
    }

    #[test]
    fn next_weekday_is_strictly_after_today() {
        assert_eq!(p("next friday"), Some(d(2026, 11, 27)));
        assert_eq!(p("next monday"), Some(d(2026, 11, 23)));
        assert_eq!(p("next thursday"), Some(d(2026, 11, 26)));
        assert_eq!(p("neste fredag"), Some(d(2026, 11, 27)));
        assert_eq!(p("neste mandag"), Some(d(2026, 11, 23)));
    }

    #[test]
    fn last_weekday_is_strictly_before_today() {
        assert_eq!(p("last monday"), Some(d(2026, 11, 16)));
        assert_eq!(p("last friday"), Some(d(2026, 11, 13)));
        assert_eq!(p("last saturday"), Some(d(2026, 11, 14)));
        assert_eq!(p("last thursday"), Some(d(2026, 11, 19)));
        assert_eq!(p("forrige mandag"), Some(d(2026, 11, 16)));
        assert_eq!(p("forrige fredag"), Some(d(2026, 11, 13)));
    }

    #[test]
    fn weekdays_cross_a_year_boundary() {
        let new_year = d(2027, 1, 1); // a Friday
        assert_eq!(parse("monday", new_year), Some(d(2027, 1, 4)));
        assert_eq!(parse("last monday", new_year), Some(d(2026, 12, 28)));
        assert_eq!(parse("next friday", new_year), Some(d(2027, 1, 8)));
    }

    #[test]
    fn offsets_in_english() {
        assert_eq!(p("3 days ago"), Some(d(2026, 11, 17)));
        assert_eq!(p("1 day ago"), Some(d(2026, 11, 19)));
        assert_eq!(p("2 weeks ago"), Some(d(2026, 11, 6)));
        assert_eq!(p("in 2 weeks"), Some(d(2026, 12, 4)));
        assert_eq!(p("in 1 day"), Some(d(2026, 11, 21)));
        assert_eq!(p("in 0 days"), Some(today()));
        assert_eq!(p("in 2 months"), Some(d(2027, 1, 20)));
        assert_eq!(p("1 year ago"), Some(d(2025, 11, 20)));
        assert_eq!(p("IN 10 DAYS"), Some(d(2026, 11, 30)));
    }

    #[test]
    fn offsets_in_norwegian() {
        assert_eq!(p("om 1 år"), Some(d(2027, 11, 20)));
        assert_eq!(p("for 1 år siden"), Some(d(2025, 11, 20)));
        assert_eq!(p("for 3 dager siden"), Some(d(2026, 11, 17)));
        assert_eq!(p("for 1 dag siden"), Some(d(2026, 11, 19)));
        assert_eq!(p("for 2 uker siden"), Some(d(2026, 11, 6)));
        assert_eq!(p("om 2 uker"), Some(d(2026, 12, 4)));
        assert_eq!(p("om 1 uke"), Some(d(2026, 11, 27)));
        assert_eq!(p("om 3 måneder"), Some(d(2027, 2, 20)));
        assert_eq!(p("for 1 måned siden"), Some(d(2026, 10, 20)));
    }

    #[test]
    fn month_offsets_clamp_to_the_end_of_a_short_month() {
        let end = d(2026, 1, 31);
        assert_eq!(parse("in 1 month", end), Some(d(2026, 2, 28)));
        assert_eq!(parse("1 month ago", d(2026, 3, 31)), Some(d(2026, 2, 28)));
        assert_eq!(parse("in 1 year", d(2024, 2, 29)), Some(d(2025, 2, 28)));
    }

    #[test]
    fn offsets_cross_year_and_leap_boundaries() {
        assert_eq!(parse("in 2 days", d(2026, 12, 30)), Some(d(2027, 1, 1)));
        assert_eq!(parse("3 days ago", d(2027, 1, 1)), Some(d(2026, 12, 29)));
        assert_eq!(parse("in 1 day", d(2024, 2, 28)), Some(d(2024, 2, 29)));
        assert_eq!(parse("in 1 day", d(2025, 2, 28)), Some(d(2025, 3, 1)));
    }

    #[test]
    fn huge_or_odd_offsets_are_rejected_not_panics() {
        assert_eq!(p("in 4000000000 days"), None);
        assert_eq!(p("in 100001 days"), None);
        assert_eq!(p("in 99999999999999999999 days"), None);
        assert_eq!(p("in -3 days"), None);
        assert_eq!(p("in 2.5 weeks"), None);
        assert_eq!(p("in 100001 years"), None);
        assert_eq!(p("in 100000 years"), None);
        assert_eq!(p("100000 years ago"), None);
        assert_eq!(p("in 8000 years"), None);
        assert_eq!(p("0999-01-01"), None);
    }

    #[test]
    fn unknown_input_is_none() {
        for text in [
            "",
            "   ",
            "soon",
            "next",
            "last",
            "next lunch",
            "in days",
            "in 3",
            "3 days",
            "ago 3 days",
            "for 3 dager",
            "om 2",
            "om uker",
            "i",
            "i dag i morgen",
            "monday friday",
            "2 weeks",
            "yesterday 1",
        ] {
            assert_eq!(p(text), None, "{text:?}");
        }
    }

    #[test]
    fn mixed_language_words_are_accepted() {
        assert_eq!(p("next mandag"), Some(d(2026, 11, 23)));
        assert_eq!(p("in 2 uker"), Some(d(2026, 12, 4)));
        assert_eq!(p("for 3 days siden"), Some(d(2026, 11, 17)));
    }

    #[test]
    fn labels_in_both_languages() {
        assert_eq!(label(d(2026, 11, 18), false), "Wednesday 18 November 2026");
        assert_eq!(label(d(2026, 11, 18), true), "onsdag 18. november 2026");
        assert_eq!(label(d(2027, 1, 3), true), "søndag 3. januar 2027");
    }
}
