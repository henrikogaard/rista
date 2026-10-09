//! Template expansion: Obsidian core `{{…}}` variables plus a safe,
//! declarative subset of Templater's `<% tp.… %>` commands. No script
//! runs — `<%* … %>` blocks and unknown commands stay as written.

use chrono::{Datelike, NaiveDate, NaiveDateTime};

/// Looks up a note by name for `tp.file.include`.
pub type Include<'a> = &'a dyn Fn(&str) -> Option<String>;

/// What a template can read about the note it fills.
pub struct TemplateCtx<'a> {
    /// The note's title (file stem).
    pub title: &'a str,
    /// "Now" for `{{date}}` and `tp.date.now()` — a periodic note's own date.
    pub now: NaiveDateTime,
    /// Vault-relative path, e.g. `Projects/Plan.md`.
    pub rel_path: &'a str,
    /// Absolute path.
    pub abs_path: &'a str,
    /// Text whose frontmatter `tp.frontmatter.key` reads (the template
    /// itself when creating a note, the note when inserting).
    pub frontmatter: &'a str,
    pub clipboard: Option<&'a str>,
    /// The editor selection the template replaces (`tp.file.selection()`).
    pub selection: &'a str,
    /// `tp.file.include("[[Note]]")` → that note's text.
    pub include: Option<Include<'a>>,
    /// Answers to `prompts(…)`, in order.
    pub answers: &'a [String],
}

impl<'a> TemplateCtx<'a> {
    pub fn new(title: &'a str, now: NaiveDateTime) -> Self {
        Self {
            title,
            now,
            rel_path: "",
            abs_path: "",
            frontmatter: "",
            clipboard: None,
            selection: "",
            include: None,
            answers: &[],
        }
    }
}

/// An expanded template.
#[derive(Debug, PartialEq)]
pub struct Expanded {
    pub text: String,
    /// Byte offset of the first `{{cursor}}` / `tp.file.cursor()`.
    pub cursor: Option<usize>,
    /// `<% … %>` commands left as written (unsupported or scripts).
    pub unsupported: usize,
}

/// A question a template asks before it expands.
#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
    /// `tp.system.prompt("Question", "default")`.
    Text { question: String, default: String },
    /// `tp.system.suggester(["Label", …], ["value", …], …, "Placeholder")`.
    Choice {
        labels: Vec<String>,
        values: Vec<String>,
        placeholder: String,
    },
}

/// The prompts `text` asks, in the order `expand` consumes answers.
pub fn prompts(text: &str) -> Vec<Prompt> {
    let mut out = Vec::new();
    for tag in tags(text) {
        let Some(call) = parse_call(tag.code) else {
            continue;
        };
        match (call.module.as_str(), call.name.as_str()) {
            ("system", "prompt") => out.push(Prompt::Text {
                question: arg_str(&call.args, 0).unwrap_or_else(|| "Value".into()),
                default: arg_str(&call.args, 1).unwrap_or_default(),
            }),
            ("system", "suggester") => {
                let labels = arg_list(&call.args, 0);
                let values = match call.args.get(1) {
                    Some(Arg::List(v)) => v.clone(),
                    _ => labels.clone(),
                };
                out.push(Prompt::Choice {
                    labels,
                    values,
                    placeholder: arg_str(&call.args, 3).unwrap_or_default(),
                });
            }
            _ => {}
        }
    }
    out
}

/// Fill in a template — see the module docs for what's supported.
pub fn expand(text: &str, ctx: &TemplateCtx) -> Expanded {
    expand_depth(text, ctx, 0)
}

fn expand_depth(text: &str, ctx: &TemplateCtx, depth: usize) -> Expanded {
    let mut out = String::with_capacity(text.len());
    let mut cursor = None;
    let mut unsupported = 0;
    let mut answers = ctx.answers.iter();
    let mut rest = text;
    loop {
        let curly = rest.find("{{");
        let angle = rest.find("<%");
        let at = match (curly, angle) {
            (Some(c), Some(a)) => c.min(a),
            (Some(c), None) => c,
            (None, Some(a)) => a,
            (None, None) => break,
        };
        out.push_str(&rest[..at]);
        let here = &rest[at..];
        if let Some(after) = here.strip_prefix("{{") {
            let Some(close) = after.find("}}") else {
                out.push_str(here);
                rest = "";
                break;
            };
            let inner = after[..close].trim();
            let (name, format) = match inner.split_once(':') {
                Some((name, format)) => (name.trim(), Some(format.trim())),
                None => (inner, None),
            };
            match (name.to_ascii_lowercase().as_str(), format) {
                ("title", None) => out.push_str(ctx.title),
                ("date", f) => out.push_str(&fmt(ctx.now, f.unwrap_or("YYYY-MM-DD"))),
                ("time", f) => out.push_str(&fmt(ctx.now, f.unwrap_or("HH:mm"))),
                ("cursor", None) => {
                    cursor.get_or_insert(out.len());
                }
                _ => out.push_str(&here[..close + 4]),
            }
            rest = &here[close + 4..];
            continue;
        }
        // `<% … %>`
        let Some(tag) = tags(here).into_iter().next().filter(|t| t.start == 0) else {
            out.push_str("<%");
            rest = &here[2..];
            continue;
        };
        if tag.trim_before {
            while out.ends_with([' ', '\t']) {
                out.pop();
            }
            if out.ends_with('\n') {
                out.pop();
            }
        }
        let value = if tag.script {
            None
        } else {
            parse_call(tag.code).and_then(|call| eval(&call, ctx, &mut answers, depth))
        };
        match value {
            Some(Value::Cursor) => {
                cursor.get_or_insert(out.len());
            }
            Some(Value::Text(text)) => out.push_str(&text),
            None => {
                unsupported += 1;
                out.push_str(&here[..tag.end]);
            }
        }
        rest = &here[tag.end..];
        if tag.trim_after {
            rest = rest.trim_start_matches([' ', '\t']);
            rest = rest.strip_prefix('\n').unwrap_or(rest);
        }
    }
    out.push_str(rest);
    Expanded {
        text: out,
        cursor,
        unsupported,
    }
}

fn fmt(dt: NaiveDateTime, format: &str) -> String {
    crate::bases::format_moment(dt, format)
}

// ------------------------------------------------------------------
// Tags and calls
// ------------------------------------------------------------------

struct Tag<'a> {
    start: usize,
    end: usize,
    code: &'a str,
    script: bool,
    trim_before: bool,
    trim_after: bool,
}

/// `<% … %>` tags in `text`, with Templater's `-` whitespace control
/// and `*` script marker.
fn tags(text: &str) -> Vec<Tag<'_>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("<%") {
        let start = from + open;
        let Some(close) = text[start + 2..].find("%>") else {
            break;
        };
        let end = start + 2 + close + 2;
        let mut code = &text[start + 2..end - 2];
        let script = code.starts_with('*');
        let trim_before = code.starts_with(['-', '_']);
        code = code.trim_start_matches(['*', '-', '_']);
        let trim_after = code.ends_with(['-', '_']);
        code = code.trim_end_matches(['-', '_']).trim();
        out.push(Tag {
            start,
            end,
            code,
            script,
            trim_before,
            trim_after,
        });
        from = end;
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Str(String),
    Num(f64),
    Bool(bool),
    List(Vec<String>),
    /// `tp.file.title` and other property reads used as arguments.
    Prop(String, String),
    Null,
}

struct Call {
    module: String,
    name: String,
    /// `None` for a property read (`tp.file.title`).
    args: Vec<Arg>,
    is_call: bool,
}

/// `tp.module.name(args…)`, `tp.module.name`, or `tp.frontmatter["key"]`.
fn parse_call(code: &str) -> Option<Call> {
    let rest = code.trim().strip_prefix("tp.")?;
    let dot = rest.find(['.', '['])?;
    let module = rest[..dot].to_string();
    let rest = rest[dot..].trim_start_matches('.');
    if let Some(key) = rest.strip_prefix('[') {
        let key = key.strip_suffix(']')?;
        let mut p = Parser { s: key, i: 0 };
        let Arg::Str(key) = p.arg()? else {
            return None;
        };
        return (p.done()).then_some(Call {
            module,
            name: key,
            args: Vec::new(),
            is_call: false,
        });
    }
    let name_len = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    let name = rest[..name_len].to_string();
    let tail = rest[name_len..].trim();
    if tail.is_empty() {
        return Some(Call {
            module,
            name,
            args: Vec::new(),
            is_call: false,
        });
    }
    let inner = tail.strip_prefix('(')?.strip_suffix(')')?;
    let mut p = Parser { s: inner, i: 0 };
    let mut args = Vec::new();
    p.ws();
    while !p.done() {
        args.push(p.arg()?);
        p.ws();
        if p.peek() == Some(',') {
            p.i += 1;
            p.ws();
        } else if !p.done() {
            return None;
        }
    }
    Some(Call {
        module,
        name,
        args,
        is_call: true,
    })
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }
    fn done(&mut self) -> bool {
        self.ws();
        self.i >= self.s.len()
    }
    fn string(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.i += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += c.len_utf8();
            match c {
                '\\' => {
                    let next = self.peek()?;
                    self.i += next.len_utf8();
                    out.push(match next {
                        'n' => '\n',
                        't' => '\t',
                        c => c,
                    });
                }
                c if c == quote => return Some(out),
                c => out.push(c),
            }
        }
        None
    }
    fn arg(&mut self) -> Option<Arg> {
        self.ws();
        match self.peek()? {
            '"' | '\'' | '`' => self.string().map(Arg::Str),
            '[' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if self.peek() == Some(']') {
                        self.i += 1;
                        return Some(Arg::List(items));
                    }
                    let Arg::Str(item) = self.arg()? else {
                        return None;
                    };
                    items.push(item);
                    self.ws();
                    if self.peek() == Some(',') {
                        self.i += 1;
                    }
                }
            }
            _ => {
                let end = self.s[self.i..]
                    .find([',', ')', ']'])
                    .map_or(self.s.len(), |e| self.i + e);
                let word = self.s[self.i..end].trim();
                self.i = end;
                Some(match word {
                    "true" => Arg::Bool(true),
                    "false" => Arg::Bool(false),
                    "null" | "undefined" => Arg::Null,
                    w => match w.parse::<f64>() {
                        Ok(n) => Arg::Num(n),
                        Err(_) => {
                            let call = parse_call(w)?;
                            if call.is_call {
                                return None;
                            }
                            Arg::Prop(call.module, call.name)
                        }
                    },
                })
            }
        }
    }
}

fn arg_str(args: &[Arg], ix: usize) -> Option<String> {
    match args.get(ix)? {
        Arg::Str(s) => Some(s.clone()),
        Arg::Num(n) => Some(n.to_string()),
        _ => None,
    }
}

fn arg_list(args: &[Arg], ix: usize) -> Vec<String> {
    match args.get(ix) {
        Some(Arg::List(v)) => v.clone(),
        _ => Vec::new(),
    }
}

// ------------------------------------------------------------------
// Evaluation
// ------------------------------------------------------------------

enum Value {
    Text(String),
    Cursor,
}

fn eval<'a>(
    call: &Call,
    ctx: &TemplateCtx,
    answers: &mut impl Iterator<Item = &'a String>,
    depth: usize,
) -> Option<Value> {
    // Arguments that are property reads (`tp.file.title`) resolve first.
    let args: Vec<Arg> = call
        .args
        .iter()
        .map(|a| match a {
            Arg::Prop(m, n) => prop(m, n, ctx).map_or(Arg::Null, Arg::Str),
            a => a.clone(),
        })
        .collect();
    let text = |s: String| Some(Value::Text(s));
    let format = |ix: usize, default: &str| arg_str(&args, ix).unwrap_or_else(|| default.into());
    let date = |offset_days: i64| {
        let base = reference_date(&args, ctx.now);
        base + chrono::Duration::days(offset_days)
    };
    match (call.module.as_str(), call.name.as_str(), call.is_call) {
        ("date", "now", true) => {
            let base = reference_date(&args, ctx.now);
            let shifted = match args.get(1) {
                Some(Arg::Num(days)) => Some(base + chrono::Duration::days(*days as i64)),
                Some(Arg::Str(iso)) => shift_iso(base, iso),
                _ => Some(base),
            }?;
            text(fmt(shifted, &format(0, "YYYY-MM-DD")))
        }
        ("date", "today", true) => text(fmt(ctx.now, &format(0, "YYYY-MM-DD"))),
        ("date", "tomorrow", true) => text(fmt(date(1), &format(0, "YYYY-MM-DD"))),
        ("date", "yesterday", true) => text(fmt(date(-1), &format(0, "YYYY-MM-DD"))),
        ("date", "weekday", true) => {
            let Some(Arg::Num(day)) = args.get(1) else {
                return None;
            };
            let base = reference_date(&args, ctx.now);
            let monday =
                base - chrono::Duration::days(base.weekday().num_days_from_monday() as i64);
            text(fmt(
                monday + chrono::Duration::days(*day as i64),
                &format(0, "YYYY-MM-DD"),
            ))
        }
        ("file", "cursor", true) => Some(Value::Cursor),
        ("file", kind @ ("creation_date" | "last_modified_date"), true) => {
            // The file's own timestamps; "now" for a note not yet written.
            let stamp = std::fs::metadata(ctx.abs_path)
                .and_then(|m| {
                    if kind == "creation_date" {
                        m.created()
                    } else {
                        m.modified()
                    }
                })
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).naive_local())
                .unwrap_or(ctx.now);
            text(fmt(stamp, &format(0, "YYYY-MM-DD HH:mm")))
        }
        ("file", "folder", true) => {
            let relative = matches!(args.first(), Some(Arg::Bool(true)));
            let folder = ctx.rel_path.rsplit_once('/').map_or("", |(f, _)| f);
            text(if relative {
                folder.to_string()
            } else {
                folder.rsplit('/').next().unwrap_or_default().to_string()
            })
        }
        ("file", "path", true) => {
            let relative = matches!(args.first(), Some(Arg::Bool(true)));
            text(if relative { ctx.rel_path } else { ctx.abs_path }.to_string())
        }
        ("file", "selection", true) => text(ctx.selection.to_string()),
        ("file", "include", true) => {
            if depth > 4 {
                return None;
            }
            let link = arg_str(&args, 0)?;
            let name = link.trim().trim_start_matches("[[").trim_end_matches("]]");
            let included = (ctx.include?)(name)?;
            text(expand_depth(&included, ctx, depth + 1).text)
        }
        ("system", "clipboard", true) => text(ctx.clipboard.unwrap_or_default().to_string()),
        ("system", "prompt", true) => {
            let default = arg_str(&args, 1).unwrap_or_default();
            text(answers.next().cloned().unwrap_or(default))
        }
        ("system", "suggester", true) => text(answers.next().cloned().unwrap_or_default()),
        (module, name, false) => prop(module, name, ctx).map(Value::Text),
        _ => None,
    }
}

/// Property reads: `tp.file.title`, `tp.frontmatter.key`.
fn prop(module: &str, name: &str, ctx: &TemplateCtx) -> Option<String> {
    match (module, name) {
        ("file", "title") => Some(ctx.title.to_string()),
        ("frontmatter", key) => frontmatter_value(ctx.frontmatter, key),
        _ => None,
    }
}

/// `tp.date.*`'s optional `reference, reference_format` arguments
/// (positions 2 and 3): a date string parsed with that Moment format.
fn reference_date(args: &[Arg], now: NaiveDateTime) -> NaiveDateTime {
    let (Some(reference), Some(format)) = (arg_str(args, 2), arg_str(args, 3)) else {
        return now;
    };
    NaiveDate::parse_from_str(&reference, &crate::bases::moment_to_chrono(&format))
        .map(|d| d.and_time(now.time()))
        .unwrap_or(now)
}

/// ISO-8601 durations as `tp.date.now` offsets: `P1D`, `-P1W`, `P-1M`, `P1Y2M`.
fn shift_iso(base: NaiveDateTime, iso: &str) -> Option<NaiveDateTime> {
    let iso = iso.trim();
    let (sign, rest) = match iso.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, iso),
    };
    let mut rest = rest.strip_prefix('P')?;
    let mut out = base;
    while !rest.is_empty() {
        let num_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '-'))
            .unwrap_or(rest.len());
        let n: i64 = rest[..num_len].parse().ok()?;
        let n = n * sign;
        let unit = rest[num_len..].chars().next()?;
        let months = |m: i64| {
            let step = chrono::Months::new(m.unsigned_abs() as u32);
            if m >= 0 {
                out.checked_add_months(step)
            } else {
                out.checked_sub_months(step)
            }
        };
        out = match unit {
            'D' => out + chrono::Duration::days(n),
            'W' => out + chrono::Duration::weeks(n),
            'M' => months(n)?,
            'Y' => months(n * 12)?,
            _ => return None,
        };
        rest = &rest[num_len + 1..];
    }
    Some(out)
}

/// A frontmatter value as text — lists join with `, `.
fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let (yaml, _) = split_frontmatter(text)?;
    let map: serde_yaml::Mapping = serde_yaml::from_str(yaml).ok()?;
    let value = map.get(serde_yaml::Value::String(key.to_string()))?;
    Some(match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Sequence(items) => items
            .iter()
            .filter_map(|v| match v {
                serde_yaml::Value::String(s) => Some(s.clone()),
                serde_yaml::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => return None,
    })
}

// ------------------------------------------------------------------
// Frontmatter merge
// ------------------------------------------------------------------

/// `(yaml, body)` when `text` opens with a `---` frontmatter block.
pub fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&rest[..at], &rest[at + line.len()..]));
        }
        at += line.len();
    }
    None
}

/// Top-level `key:` blocks of a YAML frontmatter body, each with its
/// continuation lines (indented lines and `- item` lists).
fn key_blocks(yaml: &str) -> Vec<(String, &str)> {
    let mut blocks: Vec<(String, usize, usize)> = Vec::new();
    let mut at = 0;
    for line in yaml.split_inclusive('\n') {
        let starts_key = !line.starts_with([' ', '\t', '-', '#']) && line.contains(':');
        if starts_key {
            let key = line[..line.find(':').unwrap_or(0)]
                .trim()
                .trim_matches(['"', '\''])
                .to_string();
            blocks.push((key, at, at + line.len()));
        } else if let Some(last) = blocks.last_mut() {
            last.2 = at + line.len();
        }
        at += line.len();
    }
    blocks
        .into_iter()
        .map(|(key, s, e)| (key, &yaml[s..e]))
        .collect()
}

/// Inserting a template that has frontmatter into `doc`: the note's
/// frontmatter gains the template's keys it lacks (the note's values
/// win), and only the template's body goes in at the caret. Returns
/// `(frontmatter edit, body)` — the edit replaces a byte range of `doc`
/// (or inserts a new block at 0).
pub fn merge_into(doc: &str, template: &str) -> (Option<(std::ops::Range<usize>, String)>, String) {
    let Some((tpl_yaml, tpl_body)) = split_frontmatter(template) else {
        return (None, template.to_string());
    };
    let body = tpl_body.trim_start_matches('\n').to_string();
    match split_frontmatter(doc) {
        Some((doc_yaml, _)) => {
            let have: Vec<String> = key_blocks(doc_yaml).into_iter().map(|(k, _)| k).collect();
            let mut added = String::new();
            for (key, block) in key_blocks(tpl_yaml) {
                if !have.contains(&key) {
                    added.push_str(block);
                    if !block.ends_with('\n') {
                        added.push('\n');
                    }
                }
            }
            if added.is_empty() {
                return (None, body);
            }
            // Right before the closing `---`.
            let yaml_start = doc.find('\n').map_or(0, |i| i + 1);
            let at = yaml_start + doc_yaml.len();
            (Some((at..at, added)), body)
        }
        None => (Some((0..0, format!("---\n{tpl_yaml}---\n\n"))), body),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 9)
            .unwrap()
            .and_hms_opt(8, 3, 0)
            .unwrap()
    }

    fn run(text: &str) -> Expanded {
        let ctx = TemplateCtx {
            rel_path: "Projects/Q4/Plan.md",
            abs_path: "/v/Projects/Q4/Plan.md",
            frontmatter: "---\nstatus: draft\ntags:\n  - a\n  - b\n---\n",
            clipboard: Some("CLIP"),
            selection: "picked",
            ..TemplateCtx::new("Plan", now())
        };
        expand(text, &ctx)
    }

    #[test]
    fn core_variables_formats_and_cursor() {
        let out = run("# {{title}}\n{{date}} {{time}} · {{date:dddd D. MMMM}} · {{ time : HH[h]mm }}\n{{cursor}}x{{cursor}}");
        assert_eq!(
            out.text,
            "# Plan\n2026-10-09 08:03 · Friday 9. October · 08h03\nx"
        );
        assert_eq!(out.cursor, Some(out.text.len() - 1));
    }

    #[test]
    fn templater_dates() {
        assert_eq!(run("<% tp.date.now() %>").text, "2026-10-09");
        assert_eq!(
            run("<% tp.date.now(\"YYYY-MM-DD\", 7) %>").text,
            "2026-10-16"
        );
        assert_eq!(
            run("<% tp.date.now('YYYY-MM-DD', \"P-1M\") %>").text,
            "2026-09-09"
        );
        assert_eq!(run("<% tp.date.now(\"Do\", \"-P1W\") %>").text, "2nd");
        assert_eq!(
            run("<% tp.date.tomorrow(\"DD\") %> <% tp.date.yesterday(\"DD\") %>").text,
            "10 08"
        );
        assert_eq!(run("<% tp.date.weekday(\"ddd D\", 0) %>").text, "Mon 5");
        assert_eq!(
            run("<% tp.date.now(\"YYYY-MM-DD\", 1, \"2026-01-31\", \"YYYY-MM-DD\") %>").text,
            "2026-02-01"
        );
    }

    #[test]
    fn templater_file_frontmatter_and_system() {
        assert_eq!(
            run("<% tp.file.title %>|<% tp.file.folder() %>|<% tp.file.folder(true) %>|<% tp.file.path(true) %>").text,
            "Plan|Q4|Projects/Q4|Projects/Q4/Plan.md"
        );
        assert_eq!(
            run("<% tp.frontmatter.status %> <% tp.frontmatter[\"tags\"] %>").text,
            "draft a, b"
        );
        assert_eq!(run("<% tp.system.clipboard() %>").text, "CLIP");
        assert_eq!(run("> <% tp.file.selection() %>").text, "> picked");
        // A file that doesn't exist yet: "now".
        assert_eq!(
            run("<% tp.file.creation_date(\"YYYY-MM-DD\") %>").text,
            "2026-10-09"
        );
        let out = run("a <% tp.file.cursor(1) %>b");
        assert_eq!((out.text.as_str(), out.cursor), ("a b", Some(2)));
    }

    #[test]
    fn unsupported_and_scripts_stay_as_written() {
        let out = run("<%* tR += 'x' %> <% tp.web.daily_quote() %> <% 1 + 1 %>");
        assert_eq!(
            out.text,
            "<%* tR += 'x' %> <% tp.web.daily_quote() %> <% 1 + 1 %>"
        );
        assert_eq!(out.unsupported, 3);
        assert_eq!(run("{{weather}} {{date").text, "{{weather}} {{date");
    }

    #[test]
    fn whitespace_control_trims_newlines() {
        assert_eq!(run("a\n<%- tp.file.title -%>\nb").text, "aPlanb");
    }

    #[test]
    fn prompts_are_listed_and_answered_in_order() {
        let text = "<% tp.system.prompt(\"Who?\", \"me\") %>-<% tp.system.suggester([\"High\", \"Low\"], [\"h\", \"l\"]) %>";
        assert_eq!(
            prompts(text),
            vec![
                Prompt::Text {
                    question: "Who?".into(),
                    default: "me".into()
                },
                Prompt::Choice {
                    labels: vec!["High".into(), "Low".into()],
                    values: vec!["h".into(), "l".into()],
                    placeholder: String::new(),
                },
            ]
        );
        let answers = vec!["Ann".to_string(), "l".to_string()];
        let ctx = TemplateCtx {
            answers: &answers,
            ..TemplateCtx::new("t", now())
        };
        assert_eq!(expand(text, &ctx).text, "Ann-l");
        assert_eq!(run(text).text, "me-");
    }

    #[test]
    fn includes_expand_recursively() {
        let include = |name: &str| (name == "Footer").then(|| "— {{title}}".to_string());
        let ctx = TemplateCtx {
            include: Some(&include),
            ..TemplateCtx::new("Plan", now())
        };
        assert_eq!(
            expand("Body\n<% tp.file.include(\"[[Footer]]\") %>", &ctx).text,
            "Body\n— Plan"
        );
    }

    #[test]
    fn template_frontmatter_merges_into_the_note() {
        let doc = "---\nstatus: done\n---\n\nText";
        let tpl = "---\nstatus: draft\ntags:\n- meeting\n---\n\n## Notes\n";
        let (edit, body) = merge_into(doc, tpl);
        assert_eq!(edit, Some((17..17, "tags:\n- meeting\n".to_string())));
        assert_eq!(body, "## Notes\n");
        let (edit, _) = merge_into("Plain", tpl);
        assert_eq!(
            edit,
            Some((
                0..0,
                "---\nstatus: draft\ntags:\n- meeting\n---\n\n".to_string()
            ))
        );
        assert_eq!(
            merge_into(doc, "no frontmatter"),
            (None, "no frontmatter".to_string())
        );
    }

    #[test]
    fn file_dates_come_from_the_file() {
        let path = std::env::temp_dir().join(format!("rista-tpl-{}.md", std::process::id()));
        std::fs::write(&path, "x").unwrap();
        let abs = path.to_string_lossy().to_string();
        let ctx = TemplateCtx {
            abs_path: &abs,
            ..TemplateCtx::new("t", now())
        };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert_eq!(
            expand("<% tp.file.last_modified_date(\"YYYY-MM-DD\") %>", &ctx).text,
            today
        );
        let _ = std::fs::remove_file(path);
    }
}
