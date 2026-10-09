//! the reference editor Bases — `.base` files are YAML queries over vault
//! frontmatter. We adopt the own spec (filters / formulas /
//! views / properties) so a vault reads identically in both apps.
//!
//! Supported subset:
//! - `filters:` — `and`/`or`/`not` trees of expressions:
//!   `prop == "x"` `!=` `>` `>=` `<` `<=`, arithmetic `+ - * / %`,
//!   `prop.contains("x")`, `startsWith`, `endsWith`, `isEmpty`,
//!   `file.name/path/ext/folder/mtime/ctime/size/tags`, `note.prop`,
//!   `file.hasTag("x")`/`file.inFolder("dir")`/`file.hasLink("x")`, `formula.x`,
//!   functions `contains(a,b)`/`startsWith`/`endsWith`/
//!   `isEmpty`/`now()`/`date("YYYY-MM-DD")`.
//! - `formulas:` — name → expression, referenced as `formula.name`.
//! - `properties:` — `prop: {displayName: …}` column headers.
//! - `views:` — `[{type, name, order, sort, limit, filters, group_by, date}]`;
//!   `type: table|cards|gallery|kanban|board|calendar` all render.
//! - Relations — `[[wikilink]]` properties normalize to resolved paths,
//!   render as link chips, and compare canonically against `link("x")`;
//!   `file.links`/`file.backlinks` expose the vault's link graph.
//! - Rollups — `rollup(prop, "field", "sum|avg|min|max|count|first|list")`
//!   aggregates across a relation; lists also take `.sum()/.avg()/.min()/
//!   .max()/.count()/.unique()/.join(sep)` and `sum(list)` friends.

use crate::app::Workspace;
use crate::document::Document;
use crate::properties;
use crate::vault::Vault;
use chrono::Datelike as _;
use gpui_kit::assets;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Icon, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------
// Literal value space — frontmatter YAML + file metadata + formulas.
// ------------------------------------------------------------------

/// Right-click menu on any row/card — the base row menu:
/// open, open in a new tab, reveal in the tree, copy a wikilink.
fn row_context_menu(menu: PopupMenu, path: PathBuf, workspace: WeakEntity<Workspace>) -> PopupMenu {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    menu.item(
        PopupMenuItem::new("Open")
            .icon(assets::IconName::File)
            .on_click({
                let ws = workspace.clone();
                let path = path.clone();
                move |_, window, cx| {
                    let _ = ws.update(cx, |ws, cx| ws.open_document_pub(path.clone(), window, cx));
                }
            }),
    )
    .item(
        PopupMenuItem::new("Open in new tab")
            .icon(assets::IconName::Plus)
            .on_click({
                let ws = workspace.clone();
                let path = path.clone();
                move |_, window, cx| {
                    let _ = ws.update(cx, |ws, cx| {
                        ws.open_document_new_tab(path.clone(), window, cx)
                    });
                }
            }),
    )
    .item(
        PopupMenuItem::new("Reveal in tree")
            .icon(assets::IconName::FolderOpen)
            .on_click({
                let ws = workspace.clone();
                move |_, _window, cx| {
                    let _ = ws.update(cx, |ws, cx| ws.reveal_file(&path, cx));
                }
            }),
    )
    .item(
        PopupMenuItem::new("Copy wikilink")
            .icon(assets::IconName::Link)
            .on_click(move |_, _window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(format!("[[{stem}]]")));
            }),
    )
}

/// Row/card click → open the note. ⌘+click (platform modifier) opens
/// in a new tab, matching the reference editor.
fn open_path_click(
    workspace: &WeakEntity<Workspace>,
    path: PathBuf,
    ev: &gpui::ClickEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let _ = workspace.update(cx, |ws, cx| {
        if ev.modifiers().platform {
            ws.open_document_new_tab(path, window, cx)
        } else {
            ws.open_document_pub(path, window, cx)
        }
    });
}

#[derive(Clone, Debug, PartialEq)]
enum Lit {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Lit>),
}

impl Lit {
    fn truthy(&self) -> bool {
        match self {
            Lit::Null => false,
            Lit::Bool(b) => *b,
            Lit::Num(n) => *n != 0.0,
            Lit::Str(s) => !s.is_empty(),
            Lit::List(items) => !items.is_empty(),
        }
    }

    fn display(&self) -> String {
        match self {
            Lit::Null => String::new(),
            Lit::Bool(b) => b.to_string(),
            // Whole numbers render without the fraction.
            Lit::Num(n) if n.fract() == 0.0 => format!("{}", *n as i64),
            Lit::Num(n) => format!("{n}"),
            Lit::Str(s) => s.clone(),
            Lit::List(items) => items
                .iter()
                .map(Lit::display)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

fn lit_of(value: &Value) -> Lit {
    match value {
        Value::Null => Lit::Null,
        Value::Bool(b) => Lit::Bool(*b),
        Value::Number(n) => Lit::Num(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Lit::Str(s.clone()),
        Value::Sequence(items) => Lit::List(items.iter().map(lit_of).collect()),
        Value::Mapping(m) => Lit::Str(
            serde_yaml::to_string(m)
                .map(|s| s.trim_end().to_string())
                .unwrap_or_default(),
        ),
        Value::Tagged(t) => lit_of(&t.value),
    }
}

/// Total order for sort: Null < Bool < Num < Str < List.
fn lit_cmp(a: &Lit, b: &Lit) -> std::cmp::Ordering {
    fn rank(l: &Lit) -> u8 {
        match l {
            Lit::Null => 0,
            Lit::Bool(_) => 1,
            Lit::Num(_) => 2,
            Lit::Str(_) => 3,
            Lit::List(_) => 4,
        }
    }
    let (ra, rb) = (rank(a), rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (Lit::Bool(x), Lit::Bool(y)) => x.cmp(y),
        (Lit::Num(x), Lit::Num(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Lit::Str(x), Lit::Str(y)) => x.cmp(y),
        (Lit::List(x), Lit::List(y)) => x.len().cmp(&y.len()),
        _ => std::cmp::Ordering::Equal,
    }
}

fn lit_eq(a: &Lit, b: &Lit) -> bool {
    match (a, b) {
        (Lit::Num(x), Lit::Num(y)) => x == y,
        (Lit::Str(x), Lit::Str(y)) => x == y,
        (Lit::Bool(x), Lit::Bool(y)) => x == y,
        (Lit::Null, Lit::Null) => true,
        _ => false,
    }
}

// ------------------------------------------------------------------
// Expression tokenizer + recursive-descent parser.
// ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(f64),
    Op(&'static str),
    Dot,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let bytes = src.as_bytes();
    let mut ix = 0;
    let mut toks = Vec::new();
    while ix < bytes.len() {
        let b = bytes[ix];
        match b {
            b' ' | b'\t' | b'\n' => ix += 1,
            b'(' => {
                toks.push(Tok::LParen);
                ix += 1;
            }
            b')' => {
                toks.push(Tok::RParen);
                ix += 1;
            }
            b'[' => {
                toks.push(Tok::LBracket);
                ix += 1;
            }
            b']' => {
                toks.push(Tok::RBracket);
                ix += 1;
            }
            b',' => {
                toks.push(Tok::Comma);
                ix += 1;
            }
            b'.' => {
                toks.push(Tok::Dot);
                ix += 1;
            }
            b'"' | b'\'' => {
                let quote = b;
                ix += 1;
                let mut s = String::new();
                while ix < bytes.len() && bytes[ix] != quote {
                    if bytes[ix] == b'\\' && ix + 1 < bytes.len() {
                        ix += 1;
                    }
                    s.push(bytes[ix] as char);
                    ix += 1;
                }
                if ix >= bytes.len() {
                    return Err("unterminated string".into());
                }
                ix += 1;
                toks.push(Tok::Str(s));
            }
            b'0'..=b'9' => {
                let start = ix;
                while ix < bytes.len() && (bytes[ix].is_ascii_digit() || bytes[ix] == b'.') {
                    ix += 1;
                }
                toks.push(Tok::Num(src[start..ix].parse().map_err(|_| "bad number")?));
            }
            b'!' | b'=' | b'<' | b'>' | b'&' | b'|' => {
                let two = if ix + 1 < bytes.len() {
                    &src[ix..ix + 2]
                } else {
                    ""
                };
                let op = match two {
                    "==" => Some("=="),
                    "!=" => Some("!="),
                    ">=" => Some(">="),
                    "<=" => Some("<="),
                    "&&" => Some("&&"),
                    "||" => Some("||"),
                    _ => None,
                };
                match op {
                    Some(op) => {
                        toks.push(Tok::Op(op));
                        ix += 2;
                    }
                    None if b == b'!' => {
                        toks.push(Tok::Op("!"));
                        ix += 1;
                    }
                    None if b == b'>' => {
                        toks.push(Tok::Op(">"));
                        ix += 1;
                    }
                    None if b == b'<' => {
                        toks.push(Tok::Op("<"));
                        ix += 1;
                    }
                    None => return Err(format!("unexpected '{}'", b as char)),
                }
            }
            b'+' | b'-' | b'*' | b'/' | b'%' => {
                toks.push(Tok::Op(match b {
                    b'+' => "+",
                    b'-' => "-",
                    b'*' => "*",
                    b'/' => "/",
                    _ => "%",
                }));
                ix += 1;
            }
            _ if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' => {
                let start = ix;
                while ix < bytes.len()
                    && (bytes[ix].is_ascii_alphanumeric()
                        || bytes[ix] == b'_'
                        || bytes[ix] == b'$'
                        || bytes[ix] == b'-')
                {
                    ix += 1;
                }
                toks.push(Tok::Ident(src[start..ix].to_string()));
            }
            _ => return Err(format!("unexpected '{}'", b as char)),
        }
    }
    Ok(toks)
}

#[derive(Clone, Debug)]
enum Expr {
    Lit(Lit),
    /// `[expr, …]` literal — evaluates each item at eval time.
    List(Vec<Expr>),
    /// `file.name`, `note.x`, `formula.x`, or a bare frontmatter key.
    Ref(Option<String>, String),
    Call(String, Vec<Expr>),
    Method(Box<Expr>, String, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(op)) if ops.contains(op) => {
                let op = *op;
                self.pos += 1;
                Some(op)
            }
            Some(Tok::Ident(word)) => {
                // `and`/`or`/`not` lex as idents.
                let op = match word.as_str() {
                    "and" => Some("&&"),
                    "or" => Some("||"),
                    "not" => Some("!"),
                    _ => None,
                };
                if let Some(op) = op.filter(|op| ops.contains(op)) {
                    self.pos += 1;
                    Some(op)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn expect(&mut self, tok: Tok) -> Result<(), String> {
        if self.next() == Some(tok) {
            Ok(())
        } else {
            Err("expected delimiter".into())
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        self.or()
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.and()?;
        while let Some(op) = self.eat_op(&["||"]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.and()?));
        }
        Ok(lhs)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.eq()?;
        while let Some(op) = self.eat_op(&["&&"]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.eq()?));
        }
        Ok(lhs)
    }

    fn eq(&mut self) -> Result<Expr, String> {
        let mut lhs = self.rel()?;
        while let Some(op) = self.eat_op(&["==", "!="]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.rel()?));
        }
        Ok(lhs)
    }

    fn rel(&mut self) -> Result<Expr, String> {
        let mut lhs = self.add()?;
        while let Some(op) = self.eat_op(&[">", ">=", "<", "<="]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.add()?));
        }
        Ok(lhs)
    }

    fn add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.mul()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.mul()?));
        }
        Ok(lhs)
    }

    fn mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "/", "%"]) {
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(self.unary()?));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if let Some(op) = self.eat_op(&["!", "-"]) {
            return Ok(Expr::Unary(op, Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, String> {
        let mut value = self.primary()?;
        while let Some(Tok::Dot) = self.peek() {
            self.pos += 1;
            let Some(Tok::Ident(name)) = self.next() else {
                return Err("expected member name".into());
            };
            if self.peek() == Some(&Tok::LParen) {
                self.pos += 1;
                let args = self.args()?;
                value = Expr::Method(Box::new(value), name, args);
            } else {
                // `a.b` — namespace member access.
                value = match value {
                    Expr::Ref(None, ns) => Expr::Ref(Some(ns), name),
                    other => Expr::Method(Box::new(other), name, Vec::new()),
                };
            }
        }
        Ok(value)
    }

    fn args(&mut self) -> Result<Vec<Expr>, String> {
        let mut args = Vec::new();
        if self.peek() == Some(&Tok::RParen) {
            self.pos += 1;
            return Ok(args);
        }
        loop {
            args.push(self.expr()?);
            match self.next() {
                Some(Tok::Comma) => continue,
                Some(Tok::RParen) => break,
                _ => return Err("expected ',' or ')'".into()),
            }
        }
        Ok(args)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(Expr::Lit(Lit::Str(s))),
            Some(Tok::Num(n)) => Ok(Expr::Lit(Lit::Num(n))),
            Some(Tok::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Lit(Lit::Bool(true))),
                "false" => Ok(Expr::Lit(Lit::Bool(false))),
                "null" | "None" => Ok(Expr::Lit(Lit::Null)),
                _ if self.peek() == Some(&Tok::LParen) => {
                    self.pos += 1;
                    Ok(Expr::Call(name, self.args()?))
                }
                _ => Ok(Expr::Ref(None, name)),
            },
            Some(Tok::LParen) => {
                let inner = self.expr()?;
                self.expect(Tok::RParen)?;
                Ok(inner)
            }
            // `[a, b, …]` — list literal` and formula args).
            Some(Tok::LBracket) => {
                let mut items = Vec::new();
                if self.peek() == Some(&Tok::RBracket) {
                    self.pos += 1;
                    return Ok(Expr::List(items));
                }
                loop {
                    items.push(self.expr()?);
                    match self.next() {
                        Some(Tok::Comma) => continue,
                        Some(Tok::RBracket) => break,
                        _ => return Err("expected ',' or ']'".into()),
                    }
                }
                Ok(Expr::List(items))
            }
            other => Err(format!("unexpected token {:?}", other)),
        }
    }
}

fn parse_expr(src: &str) -> Result<Expr, String> {
    let mut parser = Parser {
        toks: tokenize(src)?,
        pos: 0,
    };
    let expr = parser.expr()?;
    if parser.pos != parser.toks.len() {
        return Err("trailing tokens".into());
    }
    Ok(expr)
}

// ------------------------------------------------------------------
// Row context + evaluation.
// ------------------------------------------------------------------

/// One vault note flattened for evaluation.
struct RowData {
    path: PathBuf,
    props: BTreeMap<String, Lit>,
    /// Properties whose raw values were `[[wikilinks]]` — normalized to
    /// resolved paths for evaluation, rendered as links for display.
    link_props: std::collections::BTreeSet<String>,
    file_meta: BTreeMap<String, Lit>,
}

/// `[[target]]` occurrences in a note body (embeds included), stripped
/// of `!`, `|alias` and `#anchor` — the outgoing side of the link graph.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find("]]") else {
            break;
        };
        let inner = &rest[..close];
        rest = &rest[close + 2..];
        let target = inner
            .split('|')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
    }
    out
}

/// `![[x]]` embed targets only — `file.embeds`, like `file.links`
/// but embed forms exclusively.
fn embed_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("![[") {
        rest = &rest[open + 3..];
        let Some(close) = rest.find("]]") else {
            break;
        };
        let inner = &rest[..close];
        rest = &rest[close + 2..];
        let target = inner
            .split('|')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        if !target.is_empty() {
            out.push(target.to_string());
        }
    }
    out
}

fn prop_had_links(v: &Value) -> bool {
    match v {
        Value::String(s) => s.contains("[["),
        Value::Sequence(items) => items.iter().any(prop_had_links),
        _ => false,
    }
}

/// `[[x]]`/`![[x]]` strings become the resolved absolute path so
/// `contains`, `==` and `link("x")` all compare canonical values;
/// unresolved targets keep their raw text.
fn normalize_links(lit: Lit, resolve: &dyn Fn(&str) -> Option<PathBuf>) -> Lit {
    fn norm(s: &str, resolve: &dyn Fn(&str) -> Option<PathBuf>) -> Lit {
        let inner = s
            .trim()
            .strip_prefix("![[")
            .or_else(|| s.trim().strip_prefix("[["))
            .and_then(|s| s.strip_suffix("]]"))
            .map(|t| {
                t.split('|')
                    .next()
                    .unwrap_or("")
                    .split('#')
                    .next()
                    .unwrap_or("")
                    .trim()
            });
        match inner {
            Some(target) if !target.is_empty() => resolve(target)
                .map(|p| Lit::Str(p.to_string_lossy().replace('\\', "/")))
                .unwrap_or_else(|| Lit::Str(s.to_string())),
            _ => Lit::Str(s.to_string()),
        }
    }
    match lit {
        Lit::Str(s) => norm(&s, resolve),
        Lit::List(items) => Lit::List(
            items
                .into_iter()
                .map(|i| match i {
                    Lit::Str(s) => norm(&s, resolve),
                    other => other,
                })
                .collect(),
        ),
        other => other,
    }
}

fn row_data(
    root: &Path,
    path: &Path,
    resolve: &dyn Fn(&str) -> Option<PathBuf>,
) -> (RowData, Vec<String>, Vec<String>) {
    let rel = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let folder = Path::new(&rel)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let meta = std::fs::metadata(path).ok();
    let epoch_of = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as f64)
            .unwrap_or(0.0)
    };
    let mut file_meta = BTreeMap::new();
    file_meta.insert("name".into(), Lit::Str(name));
    file_meta.insert("path".into(), Lit::Str(rel.clone()));
    file_meta.insert("ext".into(), Lit::Str(ext));
    file_meta.insert("folder".into(), Lit::Str(folder));
    file_meta.insert(
        "basename".into(),
        Lit::Str(
            path.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
    );
    file_meta.insert(
        "size".into(),
        Lit::Num(meta.as_ref().map(|m| m.len() as f64).unwrap_or(0.0)),
    );
    file_meta.insert(
        "mtime".into(),
        Lit::Num(meta.as_ref().map(|m| epoch_of(m.modified())).unwrap_or(0.0)),
    );
    file_meta.insert(
        "ctime".into(),
        Lit::Num(meta.as_ref().map(|m| epoch_of(m.created())).unwrap_or(0.0)),
    );
    // `file.day` — date parsed from a `YYYY-MM-DD` stem (daily notes),
    //. Absent when the name isn't date-shaped.
    if let Some(day) = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .and_then(|s| parse_date(&s))
        .and_then(|d| d.and_hms_opt(0, 0, 0))
    {
        file_meta.insert("day".into(), Lit::Num(day.and_utc().timestamp() as f64));
    }
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // `file.tags` — the `#name`-shaped tag list (frontmatter +
    // inline `#tag`s, code-span and fence safe).
    file_meta.insert(
        "tags".into(),
        Lit::List(
            properties::note_tags(&text)
                .into_iter()
                .map(|t| Lit::Str(format!("#{t}")))
                .collect(),
        ),
    );
    let targets = link_targets(&text);
    let embeds = embed_targets(&text);
    let mut link_props = std::collections::BTreeSet::new();
    let props = properties::properties(&text)
        .into_iter()
        .map(|(k, v)| {
            if prop_had_links(&v) {
                link_props.insert(k.clone());
            }
            (k, normalize_links(lit_of(&v), resolve))
        })
        .collect();
    (
        RowData {
            path: path.to_path_buf(),
            props,
            link_props,
            file_meta,
        },
        targets,
        embeds,
    )
}

struct Env<'a> {
    row: &'a RowData,
    /// Every note in the vault — rollup + link resolution reach past
    /// the filtered row set.
    rows: &'a [RowData],
    formulas: &'a BTreeMap<String, Expr>,
    /// The `values` list a named summary formula aggregates over —
    /// only bound inside `summaries:` evaluation.
    values: Option<Vec<Lit>>,
    /// In an embedded ```` ```base ```` fence, `this` binds to the note
    /// hosting the embed. None in `.base` files.
    this_row: Option<&'a RowData>,
    resolve: &'a dyn Fn(&str) -> Option<PathBuf>,
    depth: usize,
}

fn aggregate(agg: &str, vals: Vec<Lit>) -> Result<Lit, String> {
    fn nums(vals: &[Lit]) -> Vec<f64> {
        vals.iter()
            .filter_map(|v| match v {
                Lit::Num(n) => Some(*n),
                _ => None,
            })
            .collect()
    }
    match agg {
        "list" => Ok(Lit::List(vals)),
        "count" | "len" => Ok(Lit::Num(vals.len() as f64)),
        "first" => Ok(vals.into_iter().next().unwrap_or(Lit::Null)),
        "sum" => Ok(Lit::Num(nums(&vals).iter().sum())),
        "avg" | "mean" => {
            let ns = nums(&vals);
            Ok(Lit::Num(if ns.is_empty() {
                0.0
            } else {
                ns.iter().sum::<f64>() / ns.len() as f64
            }))
        }
        "min" => Ok(nums(&vals)
            .iter()
            .copied()
            .reduce(f64::min)
            .map(Lit::Num)
            .unwrap_or(Lit::Null)),
        "max" => Ok(nums(&vals)
            .iter()
            .copied()
            .reduce(f64::max)
            .map(Lit::Num)
            .unwrap_or(Lit::Null)),
        _ => Err(format!("unknown aggregate '{agg}'")),
    }
}

/// Built-in `.base` summary names (the set) over a column's
/// values across the filtered row set. Returns `None` for names that
/// aren't built-ins — the caller then tries `summaries:` formulas.
fn summarize_builtin(name: &str, vals: &[Lit]) -> Option<Lit> {
    fn nums(vals: &[Lit]) -> Vec<f64> {
        vals.iter()
            .filter_map(|v| match v {
                Lit::Num(n) => Some(*n),
                _ => None,
            })
            .collect()
    }
    fn is_empty(v: &Lit) -> bool {
        match v {
            Lit::Null => true,
            Lit::Str(s) => s.is_empty(),
            Lit::List(items) => items.is_empty(),
            _ => false,
        }
    }
    match name.to_ascii_lowercase().as_str() {
        "sum" => Some(Lit::Num(nums(vals).iter().sum())),
        "average" | "avg" | "mean" => {
            let ns = nums(vals);
            Some(Lit::Num(if ns.is_empty() {
                0.0
            } else {
                ns.iter().sum::<f64>() / ns.len() as f64
            }))
        }
        "min" | "earliest" => vals
            .iter()
            .filter(|v| !matches!(v, Lit::Null))
            .min_by(|a, b| lit_cmp(a, b))
            .cloned(),
        "max" | "latest" => vals
            .iter()
            .filter(|v| !matches!(v, Lit::Null))
            .max_by(|a, b| lit_cmp(a, b))
            .cloned(),
        "median" => {
            let mut ns = nums(vals);
            ns.sort_by(f64::total_cmp);
            match ns.len() {
                0 => Some(Lit::Null),
                n if n % 2 == 1 => Some(Lit::Num(ns[n / 2])),
                n => Some(Lit::Num((ns[n / 2 - 1] + ns[n / 2]) / 2.0)),
            }
        }
        "range" => {
            let ns = nums(vals);
            match (
                ns.iter().copied().reduce(f64::min),
                ns.iter().copied().reduce(f64::max),
            ) {
                (Some(min), Some(max)) => Some(Lit::Num(max - min)),
                _ => Some(Lit::Null),
            }
        }
        "checked" => Some(Lit::Num(
            vals.iter().filter(|v| matches!(v, Lit::Bool(true))).count() as f64,
        )),
        "unchecked" => Some(Lit::Num(
            vals.iter()
                .filter(|v| matches!(v, Lit::Bool(false)))
                .count() as f64,
        )),
        "empty" => Some(Lit::Num(vals.iter().filter(|v| is_empty(v)).count() as f64)),
        "filled" => Some(Lit::Num(vals.iter().filter(|v| !is_empty(v)).count() as f64)),
        "unique" => {
            let mut sorted = vals.to_vec();
            sorted.sort_by(lit_cmp);
            sorted.dedup_by(|a, b| lit_cmp(a, b) == std::cmp::Ordering::Equal);
            Some(Lit::Num(sorted.len() as f64))
        }
        "count" => Some(Lit::Num(vals.len() as f64)),
        _ => None,
    }
}

/// Row-less env target for `values`-only summary formulas.
static SUMMARY_ROW: RowData = RowData {
    path: PathBuf::new(),
    props: BTreeMap::new(),
    link_props: std::collections::BTreeSet::new(),
    file_meta: BTreeMap::new(),
};

fn eval(expr: &Expr, env: &mut Env) -> Result<Lit, String> {
    if env.depth > 32 {
        return Err("formula recursion".into());
    }
    match expr {
        Expr::Lit(l) => Ok(l.clone()),
        Expr::List(items) => Ok(Lit::List(
            items
                .iter()
                .map(|e| eval(e, env))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Ref(ns, name) => match ns.as_deref() {
            None => Ok(if name == "values" {
                env.values.clone().map(Lit::List).unwrap_or(Lit::Null)
            } else if name == "this" {
                // Bare `this` — the hosting note's rel path, so it
                // compares equal to `file.links`/`file.backlinks` items.
                env.this_row
                    .and_then(|r| r.file_meta.get("path").cloned())
                    .unwrap_or(Lit::Null)
            } else {
                env.row
                    .props
                    .get(name)
                    .cloned()
                    // Bare `file.*`/`note.*`-less refs to file metadata work too.
                    .or_else(|| env.row.file_meta.get(name).cloned())
                    .unwrap_or(Lit::Null)
            }),
            Some("file") => Ok(env.row.file_meta.get(name).cloned().unwrap_or(Lit::Null)),
            // `this` — the file hosting the embed. `this` alone is the
            // vault-relative path (compares equal to link-graph items);
            // `this.<x>` reads its file metadata then its properties,
            // and `this.file` is the same path again so
            // `this.file.name`-style chains resolve below in Method.
            Some("this") => Ok(env
                .this_row
                .map(|r| {
                    if name == "file" {
                        r.file_meta.get("path").cloned().unwrap_or(Lit::Null)
                    } else {
                        r.file_meta
                            .get(name)
                            .cloned()
                            .or_else(|| r.props.get(name).cloned())
                            .unwrap_or(Lit::Null)
                    }
                })
                .unwrap_or(Lit::Null)),
            Some("note") => Ok(env.row.props.get(name).cloned().unwrap_or(Lit::Null)),
            Some("formula") => {
                let expr = env
                    .formulas
                    .get(name)
                    .ok_or_else(|| format!("unknown formula '{name}'"))?
                    .clone();
                env.depth += 1;
                let out = eval(&expr, env);
                env.depth -= 1;
                out
            }
            Some(other) => Err(format!("unknown namespace '{other}'")),
        },
        Expr::Unary("!", inner) => Ok(Lit::Bool(!eval(inner, env)?.truthy())),
        Expr::Unary("-", inner) => match eval(inner, env)? {
            Lit::Num(n) => Ok(Lit::Num(-n)),
            _ => Err("unary - needs a number".into()),
        },
        Expr::Unary(_, _) => unreachable!(),
        Expr::Binary(op, a, b) => {
            let (x, y) = (eval(a, env)?, eval(b, env)?);
            match *op {
                "&&" => Ok(Lit::Bool(x.truthy() && y.truthy())),
                "||" => Ok(Lit::Bool(x.truthy() || y.truthy())),
                "==" => Ok(Lit::Bool(lit_eq(&x, &y))),
                "!=" => Ok(Lit::Bool(!lit_eq(&x, &y))),
                ">" | ">=" | "<" | "<=" => {
                    let ord = lit_cmp(&x, &y);
                    Ok(Lit::Bool(match *op {
                        ">" => ord == std::cmp::Ordering::Greater,
                        ">=" => ord != std::cmp::Ordering::Less,
                        "<" => ord == std::cmp::Ordering::Less,
                        _ => ord != std::cmp::Ordering::Greater,
                    }))
                }
                "+" => match (x, y) {
                    (Lit::Num(a), Lit::Num(b)) => Ok(Lit::Num(a + b)),
                    (Lit::Str(a), Lit::Str(b)) => Ok(Lit::Str(a + &b)),
                    (Lit::Str(a), b) => Ok(Lit::Str(a + &b.display())),
                    (a, Lit::Str(b)) => Ok(Lit::Str(a.display() + &b)),
                    _ => Err("+ needs numbers or strings".into()),
                },
                "-" | "*" | "/" | "%" => match (x, y) {
                    (Lit::Num(a), Lit::Num(b)) => Ok(Lit::Num(match *op {
                        "-" => a - b,
                        "*" => a * b,
                        "/" if b != 0.0 => a / b,
                        "/" => 0.0,
                        _ if b != 0.0 => a % b,
                        _ => 0.0,
                    })),
                    _ => Err(format!("{op} needs numbers")),
                },
                _ => unreachable!(),
            }
        }
        Expr::Method(target, name, args) => {
            // `this.file.name` — the third segment of the the reference editor
            // `this.file.<prop>` chain (parses as a no-arg method on
            // `Ref(Some("this"), "file")`).
            if let Expr::Ref(Some(ns), field) = target.as_ref() {
                if ns == "this" && field == "file" {
                    return Ok(env
                        .this_row
                        .and_then(|r| r.file_meta.get(name).cloned())
                        .unwrap_or(Lit::Null));
                }
            }
            // `file.hasTag("x")` / `file.inFolder("dir")` — the reference editor
            // file-object methods, evaluated against file_meta before
            // the generic method dispatch.
            if matches!(target.as_ref(), Expr::Ref(None, n) if n == "file") {
                match name.as_str() {
                    "hasTag" => {
                        let arg = args
                            .first()
                            .map(|a| eval(a, env))
                            .transpose()?
                            .map(|v| v.display().trim_start_matches('#').to_string())
                            .unwrap_or_default();
                        let tags = match env.row.file_meta.get("tags") {
                            Some(Lit::List(items)) => items.clone(),
                            _ => Vec::new(),
                        };
                        return Ok(Lit::Bool(
                            tags.iter()
                                .any(|t| t.display().trim_start_matches('#') == arg),
                        ));
                    }
                    "inFolder" => {
                        let arg = args
                            .first()
                            .map(|a| eval(a, env))
                            .transpose()?
                            .map(|v| v.display().trim_matches('/').to_string())
                            .unwrap_or_default();
                        let folder = env
                            .row
                            .file_meta
                            .get("folder")
                            .map(|f| f.display())
                            .unwrap_or_default();
                        return Ok(Lit::Bool(
                            folder == arg || folder.starts_with(&format!("{arg}/")),
                        ));
                    }
                    "hasLink" => {
                        // `file.hasLink("note")` — outgoing-link membership,
                        // like `file.links.contains(link("note"))`. Both the
                        // argument and the stored links resolve through the
                        // same index so `link()` values (absolute paths) and
                        // bare names compare equal.
                        let raw = args
                            .first()
                            .map(|a| eval(a, env))
                            .transpose()?
                            .map(|v| v.display())
                            .unwrap_or_default();
                        let target = (env.resolve)(&raw).or_else(|| {
                            let p = PathBuf::from(&raw);
                            p.exists().then_some(p)
                        });
                        let links = match env.row.file_meta.get("links") {
                            Some(Lit::List(items)) => items.clone(),
                            _ => Vec::new(),
                        };
                        return Ok(Lit::Bool(match target {
                            Some(t) => links
                                .iter()
                                .any(|l| (env.resolve)(&l.display()).as_ref() == Some(&t)),
                            None => links.iter().any(|l| l.display() == raw),
                        }));
                    }
                    _ => {}
                }
            }
            let value = eval(target, env)?;
            let arg_values = args
                .iter()
                .map(|a| eval(a, env))
                .collect::<Result<Vec<_>, _>>()?;
            apply_method(&value, name, &arg_values)
        }
        Expr::Call(name, args) => {
            match name.as_str() {
                // `link("x")` — canonical relation target for filters like
                // `related.contains(link("daily-note"))`.
                "link" => {
                    return match args.first().map(|a| eval(a, env)) {
                        Some(Ok(Lit::Str(s))) => Ok((env.resolve)(&s)
                            .map(|p| Lit::Str(p.to_string_lossy().replace('\\', "/")))
                            .unwrap_or(Lit::Null)),
                        _ => Ok(Lit::Null),
                    }
                }
                // `rollup(related, "hours", "sum")` — aggregate a field
                // across a link property.
                "rollup" => {
                    if args.len() != 3 {
                        return Err("rollup(prop, field, agg) wants 3 args".into());
                    }
                    let targets = eval(&args[0], env)?;
                    let field = eval(&args[1], env)?.display();
                    let agg = eval(&args[2], env)?.display();
                    let mut paths = Vec::new();
                    match targets {
                        Lit::Str(s) => paths.push(s),
                        Lit::List(items) => {
                            paths.extend(items.into_iter().filter_map(|i| match i {
                                Lit::Str(s) => Some(s),
                                _ => None,
                            }))
                        }
                        _ => {}
                    }
                    let mut vals = Vec::new();
                    for p in paths {
                        let target = (env.resolve)(&p).unwrap_or_else(|| PathBuf::from(&p));
                        if let Some(row) = env.rows.iter().find(|r| r.path == target) {
                            if let Some(v) =
                                row.props.get(&field).or_else(|| row.file_meta.get(&field))
                            {
                                vals.push(v.clone());
                            }
                        }
                    }
                    return aggregate(&agg, vals);
                }
                _ => {}
            }
            let vals = args
                .iter()
                .map(|a| eval(a, env))
                .collect::<Result<Vec<_>, _>>()?;
            apply_fn(name, &vals)
        }
    }
}

fn apply_method(value: &Lit, name: &str, args: &[Lit]) -> Result<Lit, String> {
    let arg = args.first();
    match name {
        "contains" => match (value, arg) {
            (Lit::Str(s), Some(needle)) => Ok(Lit::Bool(s.contains(&needle.display()))),
            (Lit::List(items), Some(needle)) => {
                Ok(Lit::Bool(items.iter().any(|i| lit_eq(i, needle))))
            }
            _ => Ok(Lit::Bool(false)),
        },
        // the reference editor list predicates — all/any/none membership against a
        // list argument.
        "containsAll" => match (value, arg) {
            (Lit::List(items), Some(Lit::List(needles))) => Ok(Lit::Bool(
                needles.iter().all(|n| items.iter().any(|i| lit_eq(i, n))),
            )),
            _ => Ok(Lit::Bool(false)),
        },
        "containsAny" => match (value, arg) {
            (Lit::List(items), Some(Lit::List(needles))) => Ok(Lit::Bool(
                needles.iter().any(|n| items.iter().any(|i| lit_eq(i, n))),
            )),
            _ => Ok(Lit::Bool(false)),
        },
        "containsNone" => match (value, arg) {
            (Lit::List(items), Some(Lit::List(needles))) => Ok(Lit::Bool(
                !needles.iter().any(|n| items.iter().any(|i| lit_eq(i, n))),
            )),
            _ => Ok(Lit::Bool(true)),
        },
        "startsWith" => {
            Ok(Lit::Bool(arg.is_some_and(|n| {
                value.display().starts_with(&n.display())
            })))
        }
        "endsWith" => Ok(Lit::Bool(
            arg.is_some_and(|n| value.display().ends_with(&n.display())),
        )),
        "isEmpty" => Ok(Lit::Bool(!value.truthy())),
        "lower" => Ok(Lit::Str(value.display().to_lowercase())),
        "upper" => Ok(Lit::Str(value.display().to_uppercase())),
        "trim" => Ok(Lit::Str(value.display().trim().to_string())),
        "title" => Ok(Lit::Str(
            value
                .display()
                .split_whitespace()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" "),
        )),
        "replace" => match args {
            [from, to] => Ok(Lit::Str(
                value.display().replace(&from.display(), &to.display()),
            )),
            _ => Err("replace(from, to) wants 2 args".into()),
        },
        "split" => {
            let sep = arg.map(Lit::display).unwrap_or_else(|| " ".into());
            Ok(Lit::List(
                value
                    .display()
                    .split(&sep)
                    .map(|s| Lit::Str(s.to_string()))
                    .collect(),
            ))
        }
        "slice" => {
            let (start, len) = match args {
                [Lit::Num(s)] => (*s as usize, None),
                [Lit::Num(s), Lit::Num(l)] => (*s as usize, Some(*l as usize)),
                _ => return Err("slice(start[, length]) wants number args".into()),
            };
            match value {
                Lit::List(items) => Ok(Lit::List(
                    items
                        .iter()
                        .skip(start)
                        .take(len.unwrap_or(usize::MAX))
                        .cloned()
                        .collect(),
                )),
                _ => Err("slice needs a list".into()),
            }
        }
        "reverse" => match value {
            Lit::List(items) => Ok(Lit::List(items.iter().rev().cloned().collect())),
            _ => Err("reverse needs a list".into()),
        },
        "sort" => match value {
            Lit::List(items) => {
                let mut sorted = items.clone();
                sorted.sort_by(lit_cmp);
                Ok(Lit::List(sorted))
            }
            _ => Err("sort needs a list".into()),
        },
        "last" => match value {
            Lit::List(items) => Ok(items.last().cloned().unwrap_or(Lit::Null)),
            _ => Err("last needs a list".into()),
        },
        "indexOf" => match (value, arg) {
            (Lit::List(items), Some(needle)) => Ok(Lit::Num(
                items
                    .iter()
                    .position(|i| lit_eq(i, needle))
                    .map(|i| i as f64)
                    .unwrap_or(-1.0),
            )),
            _ => Err("indexOf needs a list + item".into()),
        },
        "unique" => match value {
            Lit::List(items) => {
                let mut seen = std::collections::BTreeSet::new();
                Ok(Lit::List(
                    items
                        .iter()
                        .filter(|i| seen.insert(i.display()))
                        .cloned()
                        .collect(),
                ))
            }
            _ => Ok(value.clone()),
        },
        "join" => match value {
            Lit::List(items) => {
                let sep = arg.map(Lit::display).unwrap_or_else(|| ", ".into());
                Ok(Lit::Str(
                    items
                        .iter()
                        .map(Lit::display)
                        .collect::<Vec<_>>()
                        .join(&sep),
                ))
            }
            _ => Ok(value.clone()),
        },
        "sum" | "avg" | "mean" | "min" | "max" | "count" | "len" | "first" => match value {
            Lit::List(items) => aggregate(name, items.clone()),
            _ => Err(format!("{name} needs a list")),
        },
        // `values.mean().round(3)` — the reference editor summary formulas round
        // to decimal places, not just integers.
        "round" | "floor" | "ceil" | "abs" => match value {
            Lit::Num(n) => {
                let places = args
                    .first()
                    .and_then(|a| match a {
                        Lit::Num(p) => Some(*p as i32),
                        _ => None,
                    })
                    .unwrap_or(0);
                let f = 10f64.powi(places);
                Ok(Lit::Num(match name {
                    "round" => (n * f).round() / f,
                    "floor" => (n * f).floor() / f,
                    "ceil" => (n * f).ceil() / f,
                    _ => n.abs(),
                }))
            }
            _ => Err(format!("{name} needs a number")),
        },
        _ => Err(format!("unknown method '{name}'")),
    }
}

fn apply_fn(name: &str, args: &[Lit]) -> Result<Lit, String> {
    match name {
        // Method-style functions also work in `fn(value, …)` form —
        // `contains(x, "a")`, `replace(s, "a", "b")`, `slice(l, 1, 2)`.
        "contains" | "containsAll" | "containsAny" | "containsNone" | "startsWith" | "endsWith"
        | "isEmpty" | "lower" | "upper" | "trim" | "title" | "replace" | "split" | "slice"
        | "reverse" | "sort" | "last" | "indexOf" | "unique" | "join" => {
            if args.is_empty() {
                return Err(format!("{name} wants at least 1 arg"));
            }
            apply_method(&args[0], name, &args[1..])
        }
        "sum" | "avg" | "mean" | "min" | "max" | "count" | "len" | "first" => match args {
            [Lit::List(items)] => aggregate(name, items.clone()),
            vals => aggregate(name, vals.to_vec()),
        },
        "now" => Ok(Lit::Num(chrono::Local::now().timestamp() as f64)),
        "today" => Ok(Lit::Num(
            chrono::Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .map(|t| t.and_utc().timestamp() as f64)
                .unwrap_or(0.0),
        )),
        "if" => match args {
            [cond, then, otherwise] => Ok(if cond.truthy() {
                then.clone()
            } else {
                otherwise.clone()
            }),
            _ => Err("if wants 3 args".into()),
        },
        "round" | "floor" | "ceil" | "abs" => match args {
            [Lit::Num(n)] => Ok(Lit::Num(match name {
                "round" => n.round(),
                "floor" => n.floor(),
                "ceil" => n.ceil(),
                _ => n.abs(),
            })),
            _ => Err(format!("{name} wants a number")),
        },
        "date" => match args {
            [Lit::Str(s)] => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| {
                    Lit::Num(
                        d.and_hms_opt(0, 0, 0)
                            .map(|t| t.and_utc().timestamp() as f64)
                            .unwrap_or(0.0),
                    )
                })
                .map_err(|_| "date() wants YYYY-MM-DD".into()),
            _ => Err("date() wants a string".into()),
        },
        "format" | "dateAdd" | "dateSubtract" | "duration" | "year" | "month" | "day"
        | "weekday" | "hour" | "minute" | "second"
            if args.iter().any(|a| matches!(a, Lit::Null)) =>
        {
            Ok(Lit::Null)
        }
        "format" => match args {
            [Lit::Num(epoch), Lit::Str(pattern)] => {
                let dt = chrono::DateTime::from_timestamp(*epoch as i64, 0)
                    .ok_or("format() wants a timestamp")?;
                Ok(Lit::Str(dt.format(&moment_to_chrono(pattern)).to_string()))
            }
            _ => Err("format(date, pattern) wants timestamp + string".into()),
        },
        "dateAdd" | "dateSubtract" => match args {
            [Lit::Num(epoch), Lit::Str(dur)] => {
                let d = parse_duration(dur).ok_or("duration like \"1 week\"")?;
                let sign: i64 = if name == "dateAdd" { 1 } else { -1 };
                let dt = chrono::DateTime::from_timestamp(*epoch as i64, 0)
                    .ok_or("dateAdd wants a timestamp")?;
                let out = match d {
                    Dur::Delta(d) => dt.checked_add_signed(d * (sign as i32)),
                    Dur::Months(m) if sign > 0 => dt.checked_add_months(chrono::Months::new(m)),
                    Dur::Months(m) => dt.checked_sub_months(chrono::Months::new(m)),
                };
                out.map(|d| Lit::Num(d.timestamp() as f64))
                    .ok_or_else(|| format!("{name} overflow"))
            }
            _ => Err(format!(
                "{name}(date, \"1 week\") wants timestamp + duration"
            )),
        },
        "duration" => match args {
            [Lit::Str(s)] => match parse_duration(s) {
                Some(Dur::Delta(d)) => Ok(Lit::Num(d.num_seconds() as f64)),
                Some(Dur::Months(m)) => Ok(Lit::Num(m as f64 * 30.0 * 86400.0)),
                None => Err("duration(\"1 week\") unrecognized".into()),
            },
            _ => Err("duration(\"1 week\") wants a string".into()),
        },
        "year" | "month" | "day" | "weekday" | "hour" | "minute" | "second" => match args {
            [Lit::Num(epoch)] => {
                let dt = chrono::DateTime::from_timestamp(*epoch as i64, 0)
                    .ok_or(format!("{name} wants a timestamp"))?;
                use chrono::Datelike as _;
                use chrono::Timelike as _;
                Ok(Lit::Num(match name {
                    "year" => dt.year() as f64,
                    "month" => dt.month() as f64,
                    "day" => dt.day() as f64,
                    "weekday" => (dt.weekday().num_days_from_monday() + 1) as f64,
                    "hour" => dt.hour() as f64,
                    "minute" => dt.minute() as f64,
                    _ => dt.second() as f64,
                }))
            }
            _ => Err(format!("{name}(date) wants a timestamp")),
        },
        "datetime" => match args {
            [Lit::Str(s)] => {
                for fmt in [
                    "%Y-%m-%dT%H:%M:%S",
                    "%Y-%m-%dT%H:%M",
                    "%Y-%m-%d %H:%M:%S",
                    "%Y-%m-%d %H:%M",
                ] {
                    if let Ok(t) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                        return Ok(Lit::Num(t.and_utc().timestamp() as f64));
                    }
                }
                Err("datetime() wants \"YYYY-MM-DD HH:MM\"".into())
            }
            _ => Err("datetime() wants a string".into()),
        },
        "number" => match args {
            [Lit::Num(n)] => Ok(Lit::Num(*n)),
            [Lit::Str(s)] => s
                .trim()
                .parse::<f64>()
                .map(Lit::Num)
                .map_err(|_| "number() wants a numeric string".into()),
            [Lit::Bool(b)] => Ok(Lit::Num(if *b { 1.0 } else { 0.0 })),
            _ => Err("number() wants a string/number/bool".into()),
        },
        "list" => match args {
            [Lit::List(items)] => Ok(Lit::List(items.clone())),
            [v] => Ok(Lit::List(vec![v.clone()])),
            _ => Err("list() wants one arg".into()),
        },
        _ => Err(format!("unknown function '{name}'")),
    }
}

/// Moment.js pattern → chrono strftime. Covers the common tokens:
/// `YYYY MM DD HH mm ss`, ordinals `Do`, names `dddd/ddd/MMMM/MMM`,
/// `A/a` meridian — the reference editor `.base` `format(date, pattern)`.
pub(crate) fn moment_to_chrono(pattern: &str) -> String {
    const TOKENS: &[(&str, &str)] = &[
        // Longest first — `find` returns the first prefix match.
        ("dddd", "%A"),
        ("MMMM", "%B"),
        ("ddd", "%a"),
        ("MMM", "%b"),
        ("YYYY", "%Y"),
        ("Do", "%-d"),
        ("DD", "%d"),
        ("MM", "%m"),
        ("YY", "%y"),
        ("HH", "%H"),
        ("hh", "%I"),
        ("mm", "%M"),
        ("ss", "%S"),
        ("D", "%-d"),
        ("M", "%-m"),
        ("H", "%-H"),
        ("h", "%-I"),
        ("m", "%-M"),
        ("s", "%-S"),
        ("A", "%p"),
        ("a", "%P"),
    ];
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut rest = pattern;
    while !rest.is_empty() {
        if let Some((tok, fmt)) = TOKENS.iter().find(|(tok, _)| rest.starts_with(tok)) {
            out.push_str(fmt);
            rest = &rest[tok.len()..];
        } else {
            out.push(rest.chars().next().unwrap());
            rest = &rest[1..];
        }
    }
    out
}

/// Format `dt` with a Moment pattern — the syntax daily, periodic,
/// and unique note names use. Beyond `moment_to_chrono` it handles
/// `[literal]` text, quarters (`Q`), ISO weeks (`W`/`WW`, `GGGG`), and
/// Moment's default-locale weeks (`w`/`ww`, `gggg`: Sunday-first, week
/// 1 holds January 1st). Each token is formatted on its own, so no
/// pattern can make chrono fail.
pub(crate) fn format_moment(dt: chrono::NaiveDateTime, pattern: &str) -> String {
    use chrono::Datelike;
    // Locale week: the week's Saturday decides its year; its day-of-year
    // decides the number.
    let sat = dt.date() + chrono::Duration::days(6 - dt.weekday().num_days_from_sunday() as i64);
    let locale_week = sat.ordinal0() / 7 + 1;
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if c == '[' {
            if let Some(end) = rest.find(']') {
                out.push_str(&rest[1..end]);
                rest = &rest[end + 1..];
                continue;
            }
        }
        let custom: Option<(usize, String)> = [
            ("GGGG", dt.iso_week().year().to_string()),
            ("gggg", sat.year().to_string()),
            ("WW", format!("{:02}", dt.iso_week().week())),
            ("ww", format!("{locale_week:02}")),
            ("W", dt.iso_week().week().to_string()),
            ("w", locale_week.to_string()),
            ("Q", (dt.month0() / 3 + 1).to_string()),
            ("Do", ordinal(dt.day())),
        ]
        .into_iter()
        .find(|(tok, _)| rest.starts_with(tok))
        .map(|(tok, v)| (tok.len(), v));
        if let Some((len, value)) = custom {
            out.push_str(&value);
            rest = &rest[len..];
            continue;
        }
        // Plain tokens map one-to-one onto a chrono spec.
        const TOKENS: &[&str] = &[
            "dddd", "MMMM", "ddd", "MMM", "YYYY", "Do", "DD", "MM", "YY", "HH", "hh", "mm", "ss",
            "D", "M", "H", "h", "m", "s", "A", "a",
        ];
        match TOKENS.iter().find(|t| rest.starts_with(*t)) {
            Some(tok) => {
                out.push_str(&dt.format(&moment_to_chrono(tok)).to_string());
                rest = &rest[tok.len()..];
            }
            None => {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// `1st`, `2nd`, `3rd`, `11th`, `22nd` — Moment's `Do`.
fn ordinal(day: u32) -> String {
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{day}{suffix}")
}

/// Whether a Moment pattern counts Moment's Sunday-first locale weeks
/// (`w`, `gggg`) — literals in `[...]` don't count.
pub(crate) fn moment_locale_weeks(pattern: &str) -> bool {
    let mut in_literal = false;
    pattern.chars().any(|c| {
        match c {
            '[' => in_literal = true,
            ']' => in_literal = false,
            _ => {}
        }
        !in_literal && matches!(c, 'w' | 'g')
    })
}

enum Dur {
    Delta(chrono::Duration),
    Months(u32),
}

/// `"3 days"`, `"1 week"`, `"2 months"` — calendar-aware for month/year.
fn parse_duration(s: &str) -> Option<Dur> {
    let mut it = s.split_whitespace();
    let n: f64 = it.next()?.parse().ok()?;
    let unit = it.next().unwrap_or("seconds");
    Some(match unit.trim_end_matches('s').to_lowercase().as_str() {
        "second" => Dur::Delta(chrono::Duration::seconds(n as i64)),
        "minute" => Dur::Delta(chrono::Duration::minutes(n as i64)),
        "hour" => Dur::Delta(chrono::Duration::hours(n as i64)),
        "day" => Dur::Delta(chrono::Duration::days(n as i64)),
        "week" => Dur::Delta(chrono::Duration::weeks(n as i64)),
        "month" => Dur::Months(n as u32),
        "year" => Dur::Months((n * 12.0) as u32),
        _ => return None,
    })
}

// ------------------------------------------------------------------
// Spec parsing
// ------------------------------------------------------------------

struct SortKey {
    prop: String,
    desc: bool,
}

struct ViewSpec {
    name: String,
    /// `table` (default) or `cards`/`gallery`/`kanban`/`board`.
    kind: String,
    /// Kanban: column the board groups on (`group_by:`/`group:`/`groupBy:`).
    group_by: Option<String>,
    /// `groupBy: {property, direction: DESC}` — group bands order
    /// descending instead of ascending.
    group_desc: bool,
    /// Calendar: property the month grid buckets on
    /// (`date:`/`dateProperty:`/`date_property:`/`property:`).
    date_prop: Option<String>,
    /// Cards: the property supplying the card image — the `image: note.cover` (bare `cover` also accepted). Empty → the
    /// usual cover/banner/image key list applies.
    image_prop: Option<String>,
    /// Cards: `imageFit:`/`image_fit` — `cover` (default crop) or
    /// `contain` (letterbox the whole image).
    image_fit: Option<String>,
    /// Cards: `imageAspectRatio:`/`image_aspect_ratio` — the cover
    /// area's width/height ratio instead of the fixed strip height.
    image_aspect: Option<f64>,
    /// Tables: `columnSize: {prop: px}` — per-column pixel widths
    /// overriding the grid default.
    col_size: BTreeMap<String, f64>,
    columns: Vec<String>,
    sort: Vec<SortKey>,
    limit: Option<usize>,
    filters: Option<Value>,
    /// `summaries:` — `(column prop, summary name)` in YAML order.
    summaries: Vec<(String, String)>,
}

struct BaseSpec {
    filters: Option<Value>,
    formulas: BTreeMap<String, Expr>,
    /// Raw `formulas:` source strings (unquoted) — the "Edit formula"
    /// dialog's prefill, since `Expr` doesn't round-trip to text.
    formula_srcs: BTreeMap<String, String>,
    /// Top-level `summaries:` — named custom formulas evaluated over
    /// `values` (the column's values across the filtered row set).
    summaries: BTreeMap<String, Expr>,
    properties: BTreeMap<String, String>,
    views: Vec<ViewSpec>,
    error: Option<String>,
}

fn parse_spec(yaml: &str) -> BaseSpec {
    let parsed = serde_yaml::from_str::<Value>(yaml);
    let mut spec = BaseSpec {
        filters: None,
        formulas: BTreeMap::new(),
        formula_srcs: BTreeMap::new(),
        summaries: BTreeMap::new(),
        properties: BTreeMap::new(),
        views: Vec::new(),
        error: None,
    };
    let root = match parsed {
        Ok(Value::Mapping(m)) => m,
        Ok(_) => {
            spec.error = Some("a .base file is a YAML mapping".into());
            return spec;
        }
        Err(e) => {
            spec.error = Some(format!("{e}"));
            return spec;
        }
    };
    let get = |key: &str| root.get(Value::String(key.into()));

    spec.filters = get("filters").cloned();

    if let Some(Value::Mapping(f)) = get("formulas") {
        for (k, v) in f {
            let (Some(name), Some(src)) = (k.as_str(), v.as_str()) else {
                continue;
            };
            spec.formula_srcs.insert(name.to_string(), src.to_string());
            match parse_expr(src) {
                Ok(expr) => {
                    spec.formulas.insert(name.to_string(), expr);
                }
                Err(e) => {
                    spec.error = Some(format!("formula '{name}': {e}"));
                }
            }
        }
    }

    if let Some(Value::Mapping(s)) = get("summaries") {
        for (k, v) in s {
            let (Some(name), Some(src)) = (k.as_str(), v.as_str()) else {
                continue;
            };
            match parse_expr(src) {
                Ok(expr) => {
                    spec.summaries.insert(name.to_string(), expr);
                }
                Err(e) => {
                    spec.error = Some(format!("summary '{name}': {e}"));
                }
            }
        }
    }

    if let Some(Value::Mapping(p)) = get("properties") {
        for (k, v) in p {
            let Some(name) = k.as_str() else { continue };
            let label = v
                .get(Value::String("displayName".into()))
                .and_then(|d| d.as_str())
                .unwrap_or(name);
            spec.properties.insert(name.to_string(), label.to_string());
        }
    }

    if let Some(Value::Sequence(views)) = get("views") {
        for view in views {
            let Some(map) = view.as_mapping() else {
                continue;
            };
            let getv = |key: &str| map.get(Value::String(key.into()));
            let name = getv("name")
                .and_then(|n| n.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("View {}", spec.views.len() + 1));
            let kind = getv("type")
                .and_then(|t| t.as_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "table".into());
            let (group_by, group_desc) = ["group_by", "groupBy", "group"]
                .iter()
                .find_map(|k| match getv(k) {
                    Some(Value::String(s)) => Some((Some(s.clone()), false)),
                    // Spec: `groupBy: {property: note.age, direction: DESC}`.
                    Some(Value::Mapping(m)) => {
                        let prop = m
                            .get("property")
                            .or_else(|| m.get("prop"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string)?;
                        let desc = m
                            .get("direction")
                            .and_then(|d| d.as_str())
                            .map(|d| d.eq_ignore_ascii_case("desc"))
                            .unwrap_or(false);
                        Some((Some(prop), desc))
                    }
                    _ => None,
                })
                .unwrap_or((None, false));
            let date_prop = ["date", "dateProperty", "date_property", "property"]
                .iter()
                .find_map(|k| getv(k).and_then(|g| g.as_str()).map(str::to_string));
            let columns = getv("order")
                .and_then(|o| o.as_sequence())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut sort = Vec::new();
            let push_key = |sort: &mut Vec<SortKey>, prop: &str, desc: bool| {
                sort.push(SortKey {
                    prop: prop.trim_start_matches('-').to_string(),
                    desc: desc || prop.starts_with('-'),
                });
            };
            match getv("sort") {
                Some(Value::String(s)) => push_key(&mut sort, s, false),
                Some(Value::Sequence(items)) => {
                    for item in items {
                        match item {
                            Value::String(s) => push_key(&mut sort, s, false),
                            Value::Mapping(m) => {
                                let prop = m
                                    .get(Value::String("property".into()))
                                    .and_then(|p| p.as_str())
                                    .unwrap_or_default();
                                let desc = m
                                    .get(Value::String("direction".into()))
                                    .and_then(|d| d.as_str())
                                    .map(|d| d.eq_ignore_ascii_case("desc"))
                                    .unwrap_or(false);
                                if !prop.is_empty() {
                                    push_key(&mut sort, prop, desc);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some(Value::Mapping(m)) => {
                    let prop = m
                        .get(Value::String("property".into()))
                        .and_then(|p| p.as_str())
                        .unwrap_or_default();
                    let desc = m
                        .get(Value::String("direction".into()))
                        .and_then(|d| d.as_str())
                        .map(|d| d.eq_ignore_ascii_case("desc"))
                        .unwrap_or(false);
                    if !prop.is_empty() {
                        push_key(&mut sort, prop, desc);
                    }
                }
                _ => {}
            }
            let limit = getv("limit").and_then(|l| l.as_u64()).map(|n| n as usize);
            let summaries = getv("summaries")
                .and_then(|s| s.as_mapping())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| {
                            Some((k.as_str()?.to_string(), v.as_str()?.to_string()))
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            spec.views.push(ViewSpec {
                name,
                kind,
                group_by,
                group_desc,
                date_prop,
                columns,
                sort,
                limit,
                filters: getv("filters").cloned(),
                summaries,
                image_prop: ["image", "imageProperty", "image_property"]
                    .iter()
                    .find_map(|k| getv(k).and_then(|v| v.as_str()).map(str::to_string)),
                image_fit: ["imageFit", "image_fit"]
                    .iter()
                    .find_map(|k| getv(k).and_then(|v| v.as_str()).map(str::to_string)),
                image_aspect: ["imageAspectRatio", "image_aspect_ratio"]
                    .iter()
                    .find_map(|k| getv(k).and_then(|v| v.as_f64())),
                col_size: ["columnSize", "column_size"]
                    .iter()
                    .find_map(|k| getv(k))
                    .and_then(|v| v.as_mapping())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_f64()?)))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
        }
    }

    // A base with no views gets a default table.
    if spec.views.is_empty() {
        spec.views.push(ViewSpec {
            name: "Table".into(),
            kind: "table".into(),
            group_by: None,
            group_desc: false,
            date_prop: None,
            columns: Vec::new(),
            sort: Vec::new(),
            limit: None,
            filters: None,
            image_prop: None,
            image_fit: None,
            image_aspect: None,
            col_size: BTreeMap::new(),
            summaries: Vec::new(),
        });
    }
    spec
}

// ------------------------------------------------------------------
// Evaluation pipeline — shared between views and tests.
// ------------------------------------------------------------------

struct Cell {
    /// Display string.
    text: String,
    /// Single-relation cells carry the resolved target — rendered as a
    /// chip that opens the note instead of the row.
    link: Option<PathBuf>,
    /// Raw value — interactive header sorting compares these, not text.
    lit: Lit,
}

struct Row {
    path: PathBuf,
    /// Display cells aligned with `Computed::headers`.
    cells: Vec<Cell>,
    /// `cover:`/`banner:`/`image:` property resolved to a file path or URL.
    cover: Option<String>,
}

/// ☐/☑ glyph for boolean cells — the reference editor renders booleans as
/// checkboxes rather than "true"/"false" text.
fn bool_icon(checked: bool, muted: Hsla, accent: Hsla) -> AnyElement {
    Icon::new(if checked {
        assets::IconName::SquareCheck
    } else {
        assets::IconName::Square
    })
    .size(px(13.))
    .text_color(if checked { accent } else { muted })
    .into_any_element()
}

/// Table column width — `columnSize: {prop: px}` pins a
/// column; otherwise the first column flexes and the rest hold 140px.
/// `sizes` is `col_size` overlaid with any in-progress drag-resize.
fn sized_cell<T: gpui::Styled>(d: T, sizes: &[Option<f32>], cix: usize) -> T {
    match sizes.get(cix).copied().flatten() {
        Some(w) => d.w(px(w)).flex_none(),
        None if cix == 0 => d.flex_1(),
        None => d.w(px(140.)).flex_none(),
    }
}

/// "name: value" property cell for cards/list views — bools render
/// `name: ☐` with the checkbox glyph.
fn prop_cell(header: &str, cell: &Cell, muted: Hsla, accent: Hsla) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .text_xs()
        .text_color(muted)
        .truncate()
        .child(format!("{header}:"))
        .child(match &cell.lit {
            Lit::Bool(b) => bool_icon(*b, muted, accent),
            _ => cell.text.clone().into_any_element(),
        })
}

/// Date cell text → day for the calendar grid. ISO `YYYY-MM-DD` (with
/// optional time), `YYYY/MM/DD`, and `DD.MM.YYYY` all parse.
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for fmt in ["%Y-%m-%d", "%Y/%m/%d", "%d.%m.%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, fmt) {
            return Some(d);
        }
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(t) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(t.date());
        }
    }
    None
}

/// `file.size` cell text — human-readable bytes
/// ("12.3 KB" instead of raw "12600").
fn human_size(bytes: f64) -> String {
    let units = ["B", "KB", "MB", "GB"];
    let mut v = bytes;
    let mut u = 0usize;
    while v >= 1024.0 && u < units.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} B", v as i64)
    } else {
        format!("{v:.1} {}", units[u])
    }
}

/// Stem of a resolved path, or the string itself when it isn't one.
fn stem_of(s: &str) -> String {
    let p = Path::new(s);
    match (p.file_stem(), p.extension()) {
        (Some(stem), Some(_)) => stem.to_string_lossy().to_string(),
        _ => s.to_string(),
    }
}

/// One whitespace-separated search term against a base row. `prop=v`,
/// `prop!=v`, `prop~v` (contains) and bare `prop=` (empty cell) match a
/// named column by header or expression tail; any other term
/// contains-matches the file stem + every displayed cell.
fn search_term_match(row: &Row, computed: &Computed, term: &str) -> bool {
    let (prop, cmp, val) = if let Some((p, v)) = term.split_once("!=") {
        (p, "!=", v)
    } else if let Some((p, v)) = term.split_once('=') {
        (p, "=", v)
    } else if let Some((p, v)) = term.split_once('~') {
        (p, "~", v)
    } else {
        ("", "?", term)
    };
    if cmp == "?" || prop.is_empty() {
        return row
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase().contains(term))
            .unwrap_or(false)
            || row
                .cells
                .iter()
                .any(|c| c.text.to_lowercase().contains(term));
    }
    let ix = computed
        .headers
        .iter()
        .position(|h| h.eq_ignore_ascii_case(prop))
        .or_else(|| {
            computed
                .columns
                .iter()
                .position(|c| c.eq_ignore_ascii_case(prop) || c.rsplit('.').next() == Some(prop))
        });
    let Some(ix) = ix else {
        return false;
    };
    let cell = &row.cells[ix];
    let eq = |val: &str| {
        cell.text.trim().eq_ignore_ascii_case(val)
            || serde_yaml::from_str::<serde_yaml::Value>(val)
                .map(|v| lit_cmp(&cell.lit, &lit_of(&v)) == std::cmp::Ordering::Equal)
                .unwrap_or(false)
    };
    match cmp {
        "=" => {
            if val.is_empty() {
                cell.text.trim().is_empty()
            } else {
                eq(val)
            }
        }
        "!=" => !eq(val),
        "~" => cell.text.to_lowercase().contains(val),
        _ => false,
    }
}

struct Computed {
    headers: Vec<String>,
    /// Column expressions (`note.status`, `formula.x`, `file.name`) —
    /// `headers` indexes into this; frontmatter columns are editable.
    columns: Vec<String>,
    rows: Vec<Row>,
    view_names: Vec<String>,
    /// Kind of each view in `view_names` order — `table`, `cards`,
    /// `kanban`, `calendar`, `list` — for the tab icons.
    view_kinds: Vec<String>,
    /// `table`, `cards`, or `kanban` — picked per selected view.
    kind: String,
    /// Kanban grouping column index into `headers`/`cells`, when resolved.
    group_ix: Option<usize>,
    /// The view has an explicit `group_by:` that resolved to a column —
    /// kanban/calendar always group, tables only when the user asked.
    grouped: bool,
    /// `groupBy: {direction: DESC}` — group bands order descending.
    group_desc: bool,
    /// Kanban: frontmatter key the board groups on — card drops write to
    /// it, so `formula.`/`file.` columns are excluded.
    group_prop: Option<String>,
    /// Frontmatter pairs a new note needs to satisfy the base + view
    /// filters — `prop == literal` under conjunctions only.
    prefill: Vec<(String, String)>,
    /// `summaries:` results — `(column ix, summary name, display)`
    /// rendered as a footer row under table views.
    summaries: Vec<(usize, String, String)>,
    /// Column-chooser candidates: every property/formula/file.* in the
    /// vault not already on show — offered by the header `+` cell.
    available: Vec<String>,
    /// Group-chooser candidates: every property/formula/file.* in the
    /// vault — the view-tab "Group by…" pick list.
    groupable: Vec<String>,
    /// Selected view's spec `sort:` — first `(prop, desc)` key, for
    /// the view-tab "Sort by…" direction toggle.
    sort_spec: Option<(String, bool)>,
    /// Per-view `filters:` presence — drives the view menu's
    /// "Clear filters" item.
    filters_by_view: Vec<bool>,
    /// Per-view filter expression texts in file order — the
    /// "Remove filter…" list, indexed like
    /// `splice_view_filter_remove`.
    view_filters: Vec<Vec<String>>,
    /// Per-view `order:` columns — the view-tab "Columns" submenu's
    /// check state + the `remove_base_column` fallback set.
    view_columns: Vec<Vec<String>>,
    /// Raw `formulas:` sources — the "Edit formula…" dialog prefill
    /// for `formula.*` headers.
    formula_srcs: BTreeMap<String, String>,
    /// Selected view's `imageFit:` (`cover`/`contain`) for card art.
    image_fit: Option<String>,
    /// Selected view's `imageAspectRatio:` for card art.
    image_aspect: Option<f64>,
    /// Selected view's `columnSize:` resolved per column index —
    /// `Some(px)` pins that column's width.
    col_size: Vec<Option<f32>>,
    error: Option<String>,
}

/// First image-ish property a note declares — `cover`, `banner`, `image`.
/// `![[name]]`/`[[name]]`/`![](url)` wrappers are unwrapped; vault basenames
/// resolve through the image index, relative paths against the vault root.
fn cover_of(
    row: &RowData,
    root: &Path,
    images: &std::collections::HashMap<String, PathBuf>,
    image_prop: Option<String>,
) -> Option<String> {
    // The view's `image:` key names the cover property (the `image: note.cover`); without it the usual cover keys are probed.
    let custom = image_prop
        .as_deref()
        .map(|p| p.trim().trim_start_matches("note.").to_string())
        .filter(|p| !p.is_empty());
    let prop_of = |key: &str| -> Option<String> {
        match row.props.get(key) {
            Some(Lit::Str(s)) => Some(s.clone()),
            Some(Lit::List(items)) => items.iter().find_map(|i| match i {
                Lit::Str(s) => Some(s.clone()),
                _ => None,
            }),
            _ => None,
        }
    };
    let raw = match &custom {
        Some(key) => prop_of(key)?,
        None => ["cover", "banner", "image", "cover_image"]
            .iter()
            .find_map(|k| prop_of(k))?,
    };
    let raw = raw.trim();
    let raw = raw
        .strip_prefix("![[")
        .or_else(|| raw.strip_prefix("[["))
        .and_then(|s| s.strip_suffix("]]"))
        .map(|s| s.split('|').next().unwrap_or(s).trim())
        .map(str::to_string)
        .unwrap_or_else(|| {
            raw.strip_prefix("![](")
                .or_else(|| raw.strip_prefix("[]("))
                .and_then(|s| s.strip_suffix(')'))
                .map(str::to_string)
                .unwrap_or_else(|| raw.to_string())
        });
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") {
        return Some(raw.to_string());
    }
    let name = std::path::Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(raw)
        .to_lowercase();
    if let Some(path) = images.get(&name) {
        return Some(format!("file://{}", path.display()));
    }
    let candidate = root.join(raw);
    candidate
        .exists()
        .then(|| format!("file://{}", candidate.display()))
}

/// The "Filter by…" dialog chain — property picker → operator picker →
/// value input (skipped for `is [not] empty`, which writes immediately).
/// Shared by the view-tab menu and the filter chips' "+ Filter" chip.
fn filter_pick_prop(
    this: Entity<BaseView>,
    view_ix: usize,
    candidates: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let theme = _cx.theme();
        let mut list = v_flex().w_full().py_1();
        for (nix, prop) in candidates.iter().enumerate() {
            let prop = prop.clone();
            let this = this.clone();
            list = list.child(
                div()
                    .id(("filter-pick", nix))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(prop.clone()),
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let this = this.clone();
                        let prop = prop.clone();
                        window.defer(cx, move |window, cx| {
                            filter_pick_op(this, view_ix, prop, window, cx);
                        });
                    }),
            );
        }
        dialog
            .title("Filter by property")
            .w(px(320.))
            .overlay_closable(true)
            .child(
                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                    list.max_h(px(320.)),
                ),
            )
    });
}

fn filter_pick_op(
    this: Entity<BaseView>,
    view_ix: usize,
    prop: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let theme = _cx.theme();
        let mut ops = v_flex().w_full().py_1();
        for (oix, op) in [
            "is",
            "is not",
            "contains",
            "does not contain",
            ">",
            "<",
            ">=",
            "<=",
            "is empty",
            "is not empty",
        ]
        .iter()
        .enumerate()
        {
            let op = op.to_string();
            let this = this.clone();
            let prop = prop.clone();
            ops = ops.child(
                div()
                    .id(("filter-op", oix))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .child(op.clone()),
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        // Empty-cell operators need no value — write now.
                        if op.starts_with("is ") && op.ends_with("empty") {
                            this.update(cx, |view, cx| {
                                if let SpecSrc::Doc(doc) = &view.spec_src {
                                    doc.update(cx, |doc, cx| {
                                        doc.add_base_view_filter(
                                            view_ix, &prop, &op, "", window, cx,
                                        );
                                    });
                                }
                                view.doc_epoch += 1;
                                cx.notify();
                            });
                            return;
                        }
                        let this = this.clone();
                        let prop = prop.clone();
                        let op = op.clone();
                        window.defer(cx, move |window, cx| {
                            filter_ask_value(this, view_ix, prop, op, window, cx);
                        });
                    }),
            );
        }
        dialog
            .title(format!("{prop} — filter"))
            .w(px(320.))
            .overlay_closable(true)
            .child(ops)
    });
}

fn filter_ask_value(
    this: Entity<BaseView>,
    view_ix: usize,
    prop: String,
    op: String,
    window: &mut Window,
    cx: &mut App,
) {
    let input = this.read(cx).rename_input.clone();
    input.update(cx, |input, cx| {
        input.set_value("", window, cx);
    });
    let title = format!("{prop} {op}");
    let input2 = input.clone();
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let input = input.clone();
        dialog
            .title(title.clone())
            .w(px(320.))
            .child(div().w_full().child(Input::new(&input).appearance(true)))
            .on_ok({
                let this = this.clone();
                let prop = prop.clone();
                let op = op.clone();
                move |_, window, cx| {
                    this.update(cx, |view, cx| {
                        let value = view.rename_input.read(cx).value().trim().to_string();
                        if let SpecSrc::Doc(doc) = &view.spec_src {
                            doc.update(cx, |doc, cx| {
                                doc.add_base_view_filter(view_ix, &prop, &op, &value, window, cx);
                            });
                        }
                        view.doc_epoch += 1;
                        cx.notify();
                    });
                    true
                }
            })
    });
    window.defer(cx, move |window, cx| {
        input2.update(cx, |input, cx| input.focus(window, cx));
    });
}

/// Flatten a view's `filters:` Value into expression strings in file
/// order — indexes align with `splice_view_filter_remove`'s items:
/// `and:`/`or:`/`not:` unwrap to their sequence, sequences recurse,
/// scalars arrive as their source text.
fn filter_terms(node: &Value) -> Vec<String> {
    match node {
        Value::String(s) => vec![s.clone()],
        Value::Sequence(items) => items.iter().flat_map(filter_terms).collect(),
        Value::Mapping(map) => map
            .iter()
            .flat_map(|(k, v)| {
                let inner = match v {
                    Value::Sequence(items) => items.iter().flat_map(filter_terms).collect(),
                    other => filter_terms(other),
                };
                // `or:`/`not:` terms keep their group context so a chip
                // doesn't read as a plain positive condition. Indexes
                // are unchanged — one string per flattened term.
                match k.as_str() {
                    Some("or") => inner.iter().map(|t| format!("or {t}")).collect(),
                    Some("not") => inner.iter().map(|t| format!("not {t}")).collect(),
                    _ => inner,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn eval_filter_node(node: &Value, env: &mut Env) -> Result<bool, String> {
    match node {
        Value::String(src) => Ok(parse_expr(src).and_then(|e| eval(&e, env))?.truthy()),
        Value::Sequence(items) => {
            // A bare list is a conjunction.
            items
                .iter()
                .try_fold(true, |acc, item| Ok(acc && eval_filter_node(item, env)?))
        }
        Value::Mapping(map) => {
            let mut result = true;
            for (key, value) in map {
                match key.as_str() {
                    Some("and") => match value {
                        Value::Sequence(items) => {
                            for item in items {
                                if !eval_filter_node(item, env)? {
                                    result = false;
                                }
                            }
                        }
                        other => result &= eval_filter_node(other, env)?,
                    },
                    Some("or") => match value {
                        Value::Sequence(items) => {
                            let mut any = false;
                            for item in items {
                                any |= eval_filter_node(item, env)?;
                            }
                            result &= any;
                        }
                        other => result &= eval_filter_node(other, env)?,
                    },
                    Some("not") => match value {
                        Value::Sequence(items) => {
                            for item in items {
                                result &= !eval_filter_node(item, env)?;
                            }
                        }
                        other => result &= !eval_filter_node(other, env)?,
                    },
                    _ => {}
                }
            }
            Ok(result)
        }
        _ => Ok(true),
    }
}

/// Columns whose Num cells are unix epochs shown as `YYYY-MM-DD HH:MM`.
fn is_date_column(name: &str) -> bool {
    matches!(
        name,
        "file.mtime" | "file.ctime" | "file.date" | "file.day" | "date"
    ) || name.strip_prefix("note.").is_some_and(|p| p == "date")
}

fn compute(
    spec: &BaseSpec,
    view: &ViewSpec,
    notes: &[PathBuf],
    root: &Path,
    images: &std::collections::HashMap<String, PathBuf>,
    starred: &std::collections::BTreeSet<String>,
    this_path: Option<&Path>,
) -> Computed {
    // Link index: vault-relative path, file name, and bare stem all
    // resolve — `[[a]]` finds notes/a.md, `[[notes/a.md]]` hits directly.
    let mut index: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    for note in notes {
        let rel = note
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        index.insert(rel.to_lowercase(), note.clone());
        if let Some(name) = note.file_name().map(|n| n.to_string_lossy().to_lowercase()) {
            index
                .entry(name.to_string())
                .or_insert_with(|| note.clone());
        }
        if let Some(stem) = note.file_stem().map(|s| s.to_string_lossy().to_lowercase()) {
            index
                .entry(stem.to_string())
                .or_insert_with(|| note.clone());
        }
    }
    let resolve = |target: &str| -> Option<PathBuf> {
        let t = target.trim().replace('\\', "/").to_lowercase();
        index
            .get(&t)
            .or_else(|| index.get(&format!("{t}.md")))
            .or_else(|| index.get(&format!("{t}.base")))
            .cloned()
    };

    let rel_of = |p: &Path| -> String {
        p.strip_prefix(root)
            .map(|s| s.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| p.to_string_lossy().to_string())
    };

    // First pass: every note's row + resolved outgoing links.
    let mut all_rows: Vec<RowData> = Vec::new();
    let mut resolved_targets: Vec<Vec<PathBuf>> = Vec::new();
    let mut resolved_embeds: Vec<Vec<PathBuf>> = Vec::new();
    for note in notes {
        let (mut row, targets, embeds) = row_data(root, note, &resolve);
        // `file.starred` — absolute-path membership in the vault's
        // starred set (the starred/bookmarked file property).
        row.file_meta.insert(
            "starred".into(),
            Lit::Bool(starred.contains(&note.to_string_lossy().to_string())),
        );
        resolved_targets.push(targets.iter().filter_map(|t| resolve(t)).collect());
        // `file.embeds` — `![[x]]` targets resolve against notes AND
        // the image index (`![[img.png]]` is the common case).
        resolved_embeds.push(
            embeds
                .iter()
                .filter_map(|t| resolve(t).or_else(|| images.get(&t.to_lowercase()).cloned()))
                .collect(),
        );
        all_rows.push(row);
    }
    // Link graph: file.links (outgoing, deduped) + file.backlinks (inbound).
    let mut backlinks: std::collections::HashMap<PathBuf, Vec<String>> =
        std::collections::HashMap::new();
    for (i, targets) in resolved_targets.iter().enumerate() {
        for p in targets {
            backlinks
                .entry(p.clone())
                .or_default()
                .push(rel_of(&all_rows[i].path));
        }
    }
    for (i, row) in all_rows.iter_mut().enumerate() {
        let mut links: Vec<String> = resolved_targets[i].iter().map(|p| rel_of(p)).collect();
        links.sort();
        links.dedup();
        row.file_meta.insert(
            "links".into(),
            Lit::List(links.into_iter().map(Lit::Str).collect()),
        );
        let mut embs: Vec<String> = resolved_embeds[i].iter().map(|p| rel_of(p)).collect();
        embs.sort();
        embs.dedup();
        row.file_meta.insert(
            "embeds".into(),
            Lit::List(embs.into_iter().map(Lit::Str).collect()),
        );
        let mut backs = backlinks.get(&row.path).cloned().unwrap_or_default();
        backs.sort();
        backs.dedup();
        row.file_meta.insert(
            "backlinks".into(),
            Lit::List(backs.into_iter().map(Lit::Str).collect()),
        );
    }

    // Default columns: file.name plus every property seen in the vault.
    let mut columns = view.columns.clone();
    let mut error = spec.error.clone();

    // `this` — the note hosting an embedded base — looks itself up in
    // the row set so `this.*` reads the same row any other note sees.
    let this_row = this_path.and_then(|p| all_rows.iter().find(|r| r.path == *p));
    let mut rows_ix = Vec::new();
    for (i, row) in all_rows.iter().enumerate() {
        let mut env = Env {
            row,
            rows: &all_rows,
            formulas: &spec.formulas,
            values: None,
            this_row,
            resolve: &resolve,
            depth: 0,
        };
        let pass = {
            let top_ok = spec
                .filters
                .as_ref()
                .map(|f| eval_filter_node(f, &mut env))
                .unwrap_or(Ok(true));
            let view_ok = view
                .filters
                .as_ref()
                .map(|f| eval_filter_node(f, &mut env))
                .unwrap_or(Ok(true));
            match (top_ok, view_ok) {
                (Ok(a), Ok(b)) => a && b,
                (Err(e), _) | (_, Err(e)) => {
                    error = Some(format!("filter: {e}"));
                    false
                }
            }
        };
        if pass {
            rows_ix.push(i);
        }
    }

    if columns.is_empty() {
        columns.push("file.name".into());
        let mut seen = std::collections::BTreeSet::new();
        for row in rows_ix.iter().map(|i| &all_rows[*i]) {
            for key in row.props.keys() {
                seen.insert(key.clone());
            }
        }
        columns.extend(seen.into_iter().take(11));
        columns.extend(spec.formulas.keys().map(|f| format!("formula.{f}")));
    }

    // Grouping may target a property not listed in `order` — the reference editor
    // groups by any property, so it joins the columns silently. For the
    // table view that also surfaces the group prop as a trailing column.
    if matches!(
        view.kind.as_str(),
        "kanban" | "board" | "table" | "list" | "cards" | "gallery"
    ) {
        if let Some(group) = &view.group_by {
            if !columns.iter().any(|c| c == group) {
                columns.push(group.clone());
            }
        }
    }
    // Calendar views bucket on a date property that may not be in `order`.
    if view.kind == "calendar" {
        let date_col = view
            .date_prop
            .clone()
            .or_else(|| view.group_by.clone())
            .or_else(|| {
                columns
                    .iter()
                    .find(|c| ["date", "due", "deadline"].contains(&c.as_str()))
                    .cloned()
            })
            .unwrap_or_else(|| "date".into());
        if !columns.iter().any(|c| c == &date_col) {
            columns.push(date_col);
        }
    }

    // Evaluate every column once per row so sorts compare values.
    let mut rows: Vec<(&RowData, Vec<Lit>, Option<String>)> = Vec::new();
    for row in rows_ix.iter().map(|i| &all_rows[*i]) {
        let mut env = Env {
            row,
            rows: &all_rows,
            formulas: &spec.formulas,
            values: None,
            this_row,
            resolve: &resolve,
            depth: 0,
        };
        let cells: Vec<Lit> = columns
            .iter()
            .map(
                |col| match parse_expr(col).and_then(|e| eval(&e, &mut env)) {
                    Ok(v) => v,
                    Err(e) => {
                        if error.is_none() {
                            error = Some(format!("column '{col}': {e}"));
                        }
                        Lit::Null
                    }
                },
            )
            .collect();
        let cover = cover_of(row, root, images, view.image_prop.clone());
        rows.push((row, cells, cover));
    }

    if !view.sort.is_empty() {
        let index: Vec<(usize, bool)> = view
            .sort
            .iter()
            .filter_map(|k| {
                columns
                    .iter()
                    .position(|c| c == &k.prop)
                    .map(|i| (i, k.desc))
            })
            .collect();
        rows.sort_by(|a, b| {
            for (i, desc) in &index {
                let ord = lit_cmp(&a.1[*i], &b.1[*i]);
                let ord = if *desc { ord.reverse() } else { ord };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    // `summaries:` aggregate over the filtered row set (before
    // `limit`) — `(column ix, name, display text)` aligned with
    // `headers`. Column props not in `order:` have no cell to land
    // under, so they're skipped.
    let mut summaries: Vec<(usize, String, String)> = Vec::new();
    for (prop, name) in &view.summaries {
        let (Ok(expr), Some(col_ix)) = (parse_expr(prop), columns.iter().position(|c| c == prop))
        else {
            continue;
        };
        let vals: Vec<Lit> = rows
            .iter()
            .map(|(row, _, _)| {
                let mut env = Env {
                    row,
                    rows: &all_rows,
                    formulas: &spec.formulas,
                    values: None,
                    this_row,
                    resolve: &resolve,
                    depth: 0,
                };
                eval(&expr, &mut env).unwrap_or(Lit::Null)
            })
            .collect();
        let lit = summarize_builtin(name, &vals).or_else(|| {
            spec.summaries.get(name).and_then(|expr| {
                let mut env = Env {
                    row: &SUMMARY_ROW,
                    rows: &all_rows,
                    formulas: &spec.formulas,
                    values: Some(vals.clone()),
                    this_row,
                    resolve: &resolve,
                    depth: 0,
                };
                eval(expr, &mut env).ok()
            })
        });
        if let Some(lit) = lit {
            summaries.push((col_ix, name.clone(), lit.display()));
        }
    }

    if let Some(limit) = view.limit {
        rows.truncate(limit);
    }

    // Display pass — timestamps render as local datetimes, link
    // columns as name chips, everything else via Lit::display.
    let rows: Vec<Row> = rows
        .into_iter()
        .map(|(data, cells, cover)| {
            let path = data.path.clone();
            let cells = cells
                .iter()
                .enumerate()
                .map(|(ix, cell)| {
                    let col = &columns[ix];
                    if col == "file.size" {
                        if let Lit::Num(n) = cell {
                            if *n > 0.0 {
                                return Cell {
                                    text: human_size(*n),
                                    link: None,
                                    lit: cell.clone(),
                                };
                            }
                        }
                    }
                    if is_date_column(col) {
                        if let Lit::Num(epoch) = cell {
                            if *epoch > 0.0 {
                                return Cell {
                                    text: crate::history::format_epoch(*epoch as u64)
                                        .chars()
                                        .take(16)
                                        .collect(),
                                    link: None,
                                    lit: cell.clone(),
                                };
                            }
                        }
                    }
                    let link_col = {
                        let prop = col.strip_prefix("note.").unwrap_or(col);
                        data.link_props.contains(prop)
                            || matches!(
                                col.as_str(),
                                "file.links"
                                    | "file.backlinks"
                                    | "file.embeds"
                                    | "links"
                                    | "backlinks"
                                    | "embeds"
                            )
                    };
                    match (cell, link_col) {
                        (Lit::Str(s), true) => Cell {
                            text: stem_of(s),
                            link: Some(PathBuf::from(s)),
                            lit: cell.clone(),
                        },
                        (Lit::List(items), true) => Cell {
                            text: items
                                .iter()
                                .map(|i| stem_of(&i.display()))
                                .collect::<Vec<_>>()
                                .join(", "),
                            link: None,
                            lit: cell.clone(),
                        },
                        _ => Cell {
                            text: cell.display(),
                            link: None,
                            lit: cell.clone(),
                        },
                    }
                })
                .collect();
            Row { path, cells, cover }
        })
        .collect();

    let headers: Vec<String> = columns
        .iter()
        .map(|c| {
            spec.properties
                .get(c)
                .cloned()
                .or_else(|| spec.properties.get(c.trim_start_matches("note.")).cloned())
                .unwrap_or_else(|| match c.as_str() {
                    "file.name" => "Name".into(),
                    "file.path" => "Path".into(),
                    "file.ext" => "Type".into(),
                    "file.folder" => "Folder".into(),
                    "file.size" => "Size".into(),
                    "file.links" => "Links".into(),
                    "file.backlinks" => "Backlinks".into(),
                    "file.embeds" => "Embeds".into(),
                    "file.mtime" => "Modified".into(),
                    "file.ctime" => "Created".into(),
                    "file.starred" => "Starred".into(),
                    other => other
                        .strip_prefix("formula.")
                        .or_else(|| other.strip_prefix("note."))
                        .unwrap_or(other)
                        .to_string(),
                })
        })
        .collect();

    // Kanban/calendar grouping resolves to a column index, defaulting to
    // the first non-file column (a board without a grouping makes no sense).
    let group_col = if view.kind == "calendar" {
        view.date_prop
            .clone()
            .or_else(|| view.group_by.clone())
            .or_else(|| {
                columns
                    .iter()
                    .find(|c| ["date", "due", "deadline"].contains(&c.as_str()))
                    .cloned()
            })
            .unwrap_or_else(|| "date".into())
            .into()
    } else {
        view.group_by
            .clone()
            .filter(|g| columns.iter().any(|c| c == g))
            .or_else(|| {
                columns
                    .iter()
                    .find(|c| !c.starts_with("file.") && !c.starts_with("formula."))
                    .cloned()
            })
            .or_else(|| columns.iter().find(|c| *c != "file.name").cloned())
    };
    let group_ix = group_col.and_then(|g| columns.iter().position(|c| *c == g));

    let available = available_columns(&all_rows, &spec.formulas, &columns);
    let groupable = available_columns(&all_rows, &spec.formulas, &[]);
    // `columnSize:` keys may name the column (`file.mtime`,
    // `note.status`, `formula.x`) or the property the header
    // shows (`status`).
    let col_size: Vec<Option<f32>> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            view.col_size
                .get(c.as_str())
                .or_else(|| view.col_size.get(c.strip_prefix("note.").unwrap_or(c)))
                .or_else(|| headers.get(i).and_then(|h| view.col_size.get(h)))
                .map(|w| *w as f32)
        })
        .collect();
    Computed {
        headers,
        columns,
        rows,
        view_names: spec.views.iter().map(|v| v.name.clone()).collect(),
        view_kinds: spec.views.iter().map(|v| v.kind.clone()).collect(),
        kind: view.kind.clone(),
        group_ix,
        grouped: view.group_by.is_some() && group_ix.is_some(),
        group_desc: view.group_desc,
        group_prop: view
            .group_by
            .as_ref()
            .map(|g| g.strip_prefix("note.").unwrap_or(g).to_string())
            .filter(|g| !g.is_empty() && !g.starts_with("formula.") && !g.starts_with("file.")),
        prefill: prefill_pairs(spec, view),
        summaries,
        available,
        groupable,
        sort_spec: view.sort.first().map(|k| (k.prop.clone(), k.desc)),
        filters_by_view: spec.views.iter().map(|v| v.filters.is_some()).collect(),
        view_filters: spec
            .views
            .iter()
            .map(|v| v.filters.as_ref().map(filter_terms).unwrap_or_default())
            .collect(),
        view_columns: spec.views.iter().map(|v| v.columns.clone()).collect(),
        formula_srcs: spec.formula_srcs.clone(),
        image_fit: view.image_fit.clone(),
        image_aspect: view.image_aspect,
        col_size,
        error,
    }
}

/// Column-chooser candidates: every property/formula/`file.*` in the
/// vault not already displayed in `columns`.
fn available_columns(
    all_rows: &[RowData],
    formulas: &BTreeMap<String, Expr>,
    columns: &[String],
) -> Vec<String> {
    let mut cand = std::collections::BTreeSet::new();
    for row in all_rows {
        cand.extend(row.props.keys().cloned());
    }
    for f in formulas.keys() {
        cand.insert(format!("formula.{f}"));
    }
    for f in [
        "file.name",
        "file.folder",
        "file.path",
        "file.ext",
        "file.size",
        "file.mtime",
        "file.ctime",
        "file.day",
        "file.starred",
        "file.tags",
        "file.links",
        "file.backlinks",
        "file.embeds",
    ] {
        cand.insert(f.to_string());
    }
    cand.into_iter().filter(|c| !columns.contains(c)).collect()
}

/// Render a literal as a YAML scalar for a frontmatter value.
/// `Lit` → `serde_yaml::Value` for frontmatter write-back (kanban drops).
fn lit_to_value(lit: &Lit) -> Value {
    match lit {
        Lit::Null => Value::Null,
        Lit::Bool(b) => Value::Bool(*b),
        Lit::Num(n) if n.fract() == 0.0 => Value::Number((*n as i64).into()),
        Lit::Num(n) => Value::Number(serde_yaml::Number::from(*n)),
        Lit::Str(s) => Value::String(s.clone()),
        Lit::List(items) => Value::Sequence(items.iter().map(lit_to_value).collect()),
    }
}

/// Little floating chip shown while dragging a kanban card.
struct KanbanDrag {
    label: SharedString,
}

/// Drag payload for header reorder — the dragged column's index.
struct ColDrag(usize);

/// Drag payload for view-tab reorder — the dragged view's index.
struct ViewDrag(usize);

impl Render for KanbanDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .child(self.label.clone())
    }
}

fn lit_to_yaml(lit: &Lit) -> Option<String> {
    match lit {
        Lit::Null => None,
        Lit::Bool(b) => Some(b.to_string()),
        Lit::Num(n) => Some(if n.fract() == 0.0 {
            format!("{}", *n as i64)
        } else {
            format!("{n}")
        }),
        Lit::Str(s) => serde_json::to_string(s).ok(),
        Lit::List(items) => {
            let parts: Vec<String> = items.iter().filter_map(lit_to_yaml).collect();
            Some(format!("[{}]", parts.join(", ")))
        }
    }
}

/// `prop == literal` inside an expression contributes to prefill only
/// when it must hold for every row: `&&` chains and `==` at that level.
fn collect_prefill_expr(expr: &Expr, out: &mut Vec<(String, Lit)>) {
    match expr {
        Expr::Binary("&&", a, b) => {
            collect_prefill_expr(a, out);
            collect_prefill_expr(b, out);
        }
        Expr::Binary("==", a, b) => {
            let (prop, lit) = match (a.as_ref(), b.as_ref()) {
                (Expr::Ref(ns, p), Expr::Lit(l)) | (Expr::Lit(l), Expr::Ref(ns, p))
                    if ns.is_none() || ns.as_deref() == Some("note") =>
                {
                    (p, l)
                }
                _ => return,
            };
            out.push((prop.clone(), lit.clone()));
        }
        _ => {}
    }
}

/// Conjunctive filter nodes contribute their `==` predicates; `or`/`not`
/// branches aren't guaranteed for a new row, so they don't.
fn collect_prefill(node: &Value, out: &mut Vec<(String, Lit)>) {
    match node {
        Value::String(src) => {
            if let Ok(expr) = parse_expr(src) {
                collect_prefill_expr(&expr, out);
            }
        }
        Value::Sequence(items) => {
            for item in items {
                collect_prefill(item, out);
            }
        }
        Value::Mapping(map) => {
            for (key, value) in map {
                if key.as_str() == Some("and") {
                    collect_prefill(value, out);
                }
            }
        }
        _ => {}
    }
}

/// Frontmatter pairs making a brand-new note land in this view: base-level
/// `and` filters plus the active view's, the view's winning on a key clash.
fn prefill_pairs(spec: &BaseSpec, view: &ViewSpec) -> Vec<(String, String)> {
    let mut lits = Vec::new();
    if let Some(filters) = &spec.filters {
        collect_prefill(filters, &mut lits);
    }
    if let Some(filters) = &view.filters {
        collect_prefill(filters, &mut lits);
    }
    let mut pairs = std::collections::BTreeMap::new();
    for (key, value) in lits {
        if let Some(yaml) = lit_to_yaml(&value) {
            pairs.insert(key, yaml);
        }
    }
    pairs.into_iter().collect()
}

/// Shared front half of the `order:`/`views:` splices: split `src`
/// into lines with byte offsets and collect the `views:` list's
/// direct item start-lines. Returns `(lines, offs, item_starts,
/// item_indent)` — `item_indent` is the depth the `- ` markers sit at.
fn view_items<'a>(src: &'a str) -> Option<(Vec<&'a str>, Vec<usize>, Vec<usize>, usize)> {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut offs = Vec::with_capacity(lines.len() + 1);
    offs.push(0usize);
    for l in &lines {
        offs.push(offs.last().unwrap() + l.len());
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";

    // The `views:` key, then its direct list items (deeper, same indent).
    let views_ln = lines
        .iter()
        .position(|l| l.trim_start().starts_with("views:"))?;
    let views_ind = indent(lines[views_ln]);
    let mut items: Vec<usize> = Vec::new();
    let mut item_ind = None;
    for (i, line) in lines.iter().enumerate().skip(views_ln + 1) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let ind = indent(line);
        if ind <= views_ind {
            break;
        }
        let text = trimmed.trim_start();
        match item_ind {
            // Nested lists (`filters:`/`order:`) sit deeper than the
            // view items, so the first `- ` depth becomes canonical.
            None if is_item(text) => {
                item_ind = Some(ind);
                items.push(i);
            }
            Some(item_ind) if ind == item_ind && is_item(text) => items.push(i),
            _ => {}
        }
    }
    Some((lines, offs, items, item_ind?))
}

/// Line-span of a `views:` item: from its `- ` line to the next item
/// (or the first line dedented to/above `views:`).
fn view_span(lines: &[&str], items: &[usize], ix: usize, views_ind: usize) -> (usize, usize) {
    let indent = |l: &str| l.len() - l.trim_start().len();
    let start_ln = items[ix];
    let end_ln = items
        .get(ix + 1)
        .copied()
        .or_else(|| {
            (start_ln..lines.len())
                .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) <= views_ind)
        })
        .unwrap_or(lines.len());
    (start_ln, end_ln)
}

/// Shared front half of the `order:` splices: split `src` into lines
/// with byte offsets and locate the `view_ix`-th `views:` item.
/// Returns `(lines, offs, item_start_ln, item_end_ln, key_indent)` —
/// `key_indent` is the indent a view's own keys sit at.
fn view_item<'a>(
    src: &'a str,
    view_ix: usize,
) -> Option<(Vec<&'a str>, Vec<usize>, usize, usize, usize)> {
    let (lines, offs, items, item_ind) = view_items(src)?;
    let views_ind = lines
        .iter()
        .position(|l| l.trim_start().starts_with("views:"))
        .map(|i| lines[i].len() - lines[i].trim_start().len())?;
    if items.len() <= view_ix {
        return None;
    }
    let (start_ln, end_ln) = view_span(&lines, &items, view_ix, views_ind);
    Some((lines, offs, start_ln, end_ln, item_ind + 2))
}

/// Escape a value for embedding inside a single-quoted YAML scalar —
/// `'` doubles per YAML rules.
pub(crate) fn yaml_squote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Append `'expr'` to the `view_ix`-th view's `filters:` — creates
/// `filters: and: - expr` when the view has none, converts the scalar
/// form `filters: 'e'` into an and-block carrying both expressions,
/// and appends inside an existing `and:`/`or:`/`not:` (or direct
/// sequence) block at its item indent. Returns the usual
/// `(byte_start, byte_end, replacement)` splice.
pub fn splice_view_filter(src: &str, view_ix: usize, expr: &str) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let ind = |depth: usize| " ".repeat(depth);
    let item_line = |depth: usize| format!("{}- '{}'\n", ind(depth), yaml_squote(expr));
    // `filters:` sits among the view's own keys at `key_ind`.
    let fkey = (start_ln..end_ln).find(|&i| {
        let depth = lines[i].len() - lines[i].trim_start().len();
        let t = lines[i].trim();
        i > start_ln && depth == key_ind && (t == "filters:" || t.starts_with("filters: "))
    });
    let Some(fl) = fkey else {
        // No filters key — insert a fresh and-block right after the
        // `- type:` line, like the scalar-key insert.
        let insert = offs[start_ln + 1];
        let item = item_line(key_ind + 4);
        let text = format!("{}filters:\n{}  and:\n{}", ind(key_ind), ind(key_ind), item);
        return Some((insert, insert, text));
    };
    let rest = lines[fl].trim()["filters:".len()..].trim();
    if !rest.is_empty() {
        // Scalar form `filters: 'e'` → and-block keeping the old
        // expression alongside the new one. Swallow any deeper
        // lines under the key (there shouldn't be any).
        let old = rest
            .trim_start_matches('\'')
            .trim_end_matches('\'')
            .replace("''", "'");
        let old_item = format!("{}- '{}'\n", ind(key_ind + 4), yaml_squote(&old));
        let new_item = item_line(key_ind + 4);
        let text = format!(
            "{}filters:\n{}  and:\n{}{}",
            ind(key_ind),
            ind(key_ind),
            old_item,
            new_item
        );
        let mut last = fl + 1;
        while last < end_ln
            && !lines[last].trim().is_empty()
            && lines[last].len() - lines[last].trim_start().len() > key_ind
        {
            last += 1;
        }
        return Some((offs[fl], offs.get(last).copied().unwrap_or(src.len()), text));
    }
    // Nested block — append a `- ` item after the deepest last one,
    // or just under the block's first line when it has none (e.g.
    // `and:` with no items yet).
    let mut last_dash: Option<usize> = None;
    let mut first: Option<usize> = None;
    let mut i = fl + 1;
    while i < end_ln {
        let t = lines[i].trim_start();
        if !t.is_empty() {
            let depth = lines[i].len() - t.len();
            if depth <= key_ind {
                break;
            }
            if first.is_none() {
                first = Some(i);
            }
            if t.starts_with("- ") || t == "-" {
                last_dash = Some(i);
            }
        }
        i += 1;
    }
    let (anchor, depth) = match last_dash {
        Some(d) => {
            let depth = lines[d].len() - lines[d].trim_start().len();
            (d + 1, depth)
        }
        None => match first {
            Some(f) => (f + 1, lines[f].len() - lines[f].trim_start().len() + 2),
            // `filters:` with no block at all — fill it in place.
            None => {
                let text = format!(
                    "{}filters:\n{}  and:\n{}",
                    ind(key_ind),
                    ind(key_ind),
                    item_line(key_ind + 4)
                );
                return Some((
                    offs[fl],
                    offs.get(fl + 1).copied().unwrap_or(src.len()),
                    text,
                ));
            }
        },
    };
    let insert = offs.get(anchor).copied().unwrap_or(src.len());
    Some((insert, insert, item_line(depth)))
}

/// Remove the `term_ix`-th `- expr` item under the `view_ix`-th view's
/// `filters:` — the "Remove filter…" write path. Empty group lines
/// (`and:`/`or:`/`not:`) drop with their last item, and an emptied
/// `filters:` key drops entirely — the progressive-delete convention
/// used by the other splices. Handles the scalar `filters: 'e'` form
/// as a single item. Returns `(byte_start, byte_end, replacement)`
/// (empty for pure deletions) or None when no such item exists.
pub fn splice_view_filter_remove(
    src: &str,
    view_ix: usize,
    term_ix: usize,
) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let fl = (start_ln..end_ln).find(|&i| {
        let depth = lines[i].len() - lines[i].trim_start().len();
        let t = lines[i].trim();
        i > start_ln && depth == key_ind && (t == "filters:" || t.starts_with("filters: "))
    })?;
    // Scalar form — one item, delete the whole key line.
    if !lines[fl].trim()["filters:".len()..].trim().is_empty() {
        if term_ix == 0 {
            return Some((
                offs[fl],
                offs.get(fl + 1).copied().unwrap_or(src.len()),
                String::new(),
            ));
        }
        return None;
    }
    // Collect `- ` item lines and group lines (`and:`/`or:`/`not:`)
    // inside the filters block.
    let mut items: Vec<usize> = Vec::new();
    let mut groups: Vec<usize> = Vec::new();
    let mut i = fl + 1;
    while i < end_ln {
        let t = lines[i].trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        let depth = lines[i].len() - lines[i].trim_start().len();
        if depth <= key_ind {
            break;
        }
        if t.starts_with("- ") || t == "-" {
            items.push(i);
        } else if t == "and:" || t == "or:" || t == "not:" {
            groups.push(i);
        }
        i += 1;
    }
    let block_end = i;
    let &il = items.get(term_ix)?;
    let item_depth = lines[il].len() - lines[il].trim_start().len();
    // The item's lines extend until the next `- ` at its depth or any
    // shallower/equal line (multi-line expressions, nested blocks).
    let mut next = il + 1;
    while next < block_end {
        let t = lines[next].trim();
        if t.is_empty() {
            next += 1;
            continue;
        }
        let depth = lines[next].len() - lines[next].trim_start().len();
        if depth <= item_depth {
            break;
        }
        next += 1;
    }
    // Removing the last item under `filters:` kills the whole key.
    if items.len() == 1 {
        return Some((
            offs[fl],
            offs.get(block_end).copied().unwrap_or(src.len()),
            String::new(),
        ));
    }
    // A group line dies when its block holds no other `- ` item.
    // Doomed groups can only be this item's own ancestors, so the
    // splice runs from the shallowest doomed line to the item's end.
    let group_doomed = |g: usize| -> bool {
        let gdepth = lines[g].len() - lines[g].trim_start().len();
        let mut j = g + 1;
        while j < block_end {
            let t = lines[j].trim();
            if !t.is_empty() {
                let d = lines[j].len() - lines[j].trim_start().len();
                if d <= gdepth {
                    break;
                }
                if (t.starts_with("- ") || t == "-") && j != il {
                    return false;
                }
            }
            j += 1;
        }
        true
    };
    let doomed = groups
        .iter()
        .copied()
        .filter(|&g| group_doomed(g))
        .chain([il])
        .min()
        .unwrap_or(il);
    Some((
        offs[doomed],
        offs.get(next).copied().unwrap_or(src.len()),
        String::new(),
    ))
}

/// Set or clear a scalar `key:` on the `view_ix`-th view item —
/// `group_by: status` and friends. `Some(value)` rewrites the existing
/// key line or inserts it after `type:`; `None` deletes the key line.
/// Returns `(byte_start, byte_end, replacement)` or None when the view
/// can't be located (or there's no key to clear).
pub fn splice_view_key(
    src: &str,
    view_ix: usize,
    key: &str,
    value: Option<&str>,
) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let head = format!("{key}:");
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if indent(lines[i]) == key_ind && t.starts_with(&head) {
            // A block value (`sort:\n  - property: …`) leaves its
            // nested lines orphaned unless they leave with the key.
            let mut last = i + 1;
            while last < end_ln && !lines[last].trim().is_empty() && indent(lines[last]) > key_ind {
                last += 1;
            }
            return Some(match value {
                Some(v) => (
                    offs[i],
                    offs[last],
                    format!("{}{head} {}\n", " ".repeat(key_ind), yaml_name(v)),
                ),
                None => (offs[i], offs[last], String::new()),
            });
        }
    }
    let v = value?;
    // Missing key — insert right after the item's `- ` line.
    Some((
        offs[start_ln + 1],
        offs[start_ln + 1],
        format!("{}{head} {}\n", " ".repeat(key_ind), yaml_name(v)),
    ))
}

/// Set or delete `entry: value` inside the view's `{map_key}: …`
/// nested mapping (`summaries:`/`columnSize:` write paths). Creates
/// the mapping after the `- ` line when absent, converts a flow
/// `{k: v}` map to block form on write, and removes the map key when
/// its last entry is deleted. `None` value deletes the entry.
pub fn splice_view_map_entry(
    src: &str,
    view_ix: usize,
    map_key: &str,
    entry_key: &str,
    value: Option<&str>,
) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let entry_ind = key_ind + 2;
    let head = format!("{map_key}:");
    let ehead = format!("{entry_key}:");
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if indent(lines[i]) != key_ind || !t.starts_with(&head) {
            continue;
        }
        let tail = t[head.len()..].trim();
        // The map's own lines run to the first line at ≤ key_ind.
        let mut map_end = i + 1;
        while map_end < end_ln
            && !lines[map_end].trim().is_empty()
            && indent(lines[map_end]) > key_ind
        {
            map_end += 1;
        }
        if !tail.is_empty() && !tail.starts_with('{') {
            // `summaries: foo` — a scalar, not a mapping; can't splice.
            return None;
        }
        if tail.starts_with('{') {
            // Flow `{a: f, b: g}` — rewrite as block form, upserting.
            let inner = tail.trim_start_matches('{').trim_end_matches('}');
            let mut entries: Vec<String> = Vec::new();
            let mut replaced = false;
            for part in inner.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some((k, _)) = part.split_once(':') {
                    if k.trim() == entry_key {
                        if let Some(v) = value {
                            entries.push(format!("{entry_key}: {}", yaml_name(v)));
                        }
                        replaced = true;
                        continue;
                    }
                }
                entries.push(part.to_string());
            }
            if let Some(v) = value.filter(|_| !replaced) {
                entries.push(format!("{entry_key}: {}", yaml_name(v)));
            }
            if entries.is_empty() {
                return Some((offs[i], offs[map_end], String::new()));
            }
            let body: String = entries
                .iter()
                .map(|e| format!("{}{e}\n", " ".repeat(entry_ind)))
                .collect();
            return Some((
                offs[i],
                offs[map_end],
                format!("{}{head}\n{body}", " ".repeat(key_ind)),
            ));
        }
        // Block form — find the entry line.
        for j in (i + 1)..map_end {
            let e = lines[j].trim_start();
            if indent(lines[j]) == entry_ind && e.starts_with(&ehead) {
                return match value {
                    Some(v) => Some((
                        offs[j],
                        offs[j + 1],
                        format!("{}{ehead} {}\n", " ".repeat(entry_ind), yaml_name(v)),
                    )),
                    None => {
                        // Deleting the last entry removes the map key too.
                        let entries = (i + 1..map_end)
                            .filter(|&k| {
                                !lines[k].trim().is_empty() && indent(lines[k]) == entry_ind
                            })
                            .count();
                        if entries <= 1 {
                            Some((offs[i], offs[map_end], String::new()))
                        } else {
                            Some((offs[j], offs[j + 1], String::new()))
                        }
                    }
                };
            }
        }
        // Entry missing — append at the map's end.
        let v = value?;
        return Some((
            offs[map_end],
            offs[map_end],
            format!("{}{ehead} {}\n", " ".repeat(entry_ind), yaml_name(v)),
        ));
    }
    // No map key — insert `map_key:` + first entry after the `- ` line.
    let v = value?;
    Some((
        offs[start_ln + 1],
        offs[start_ln + 1],
        format!(
            "{}{head}\n{}{ehead} {}\n",
            " ".repeat(key_ind),
            " ".repeat(entry_ind),
            yaml_name(v)
        ),
    ))
}

/// Text-splice a scalar entry under a spec-root mapping —
/// `{map_key}: {entry}: v` — the `formulas:` write path. Creates the
/// map before `views:` when absent; `None` deletes the entry (and the
/// map when it was the last). Returns `(byte_start, byte_end,
/// replacement)` or None.
pub fn splice_root_map_entry(
    src: &str,
    map_key: &str,
    entry_key: &str,
    value: Option<&str>,
) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut offs = Vec::with_capacity(lines.len() + 1);
    offs.push(0usize);
    for l in &lines {
        offs.push(offs.last().unwrap() + l.len());
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let at = |i: usize| offs.get(i).copied().unwrap_or(src.len());
    let ehead = format!("{}:", yaml_name(entry_key));

    let map_ln = lines
        .iter()
        .position(|l| indent(l) == 0 && l.trim_start().starts_with(&format!("{map_key}:")));
    let Some(map_ln) = map_ln else {
        let v = value?;
        let views_ln = lines
            .iter()
            .position(|l| indent(l) == 0 && l.trim_start().starts_with("views:"));
        let at = views_ln.map(at).unwrap_or(src.len());
        return Some((at, at, format!("{map_key}:\n  {ehead} {v}\n")));
    };

    let map_end = (map_ln + 1..lines.len())
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) == 0)
        .unwrap_or(lines.len());
    let entry_ind = (map_ln + 1..map_end)
        .filter(|&i| !lines[i].trim().is_empty())
        .map(|i| indent(lines[i]))
        .next()
        .unwrap_or(2);

    let entry_ln = (map_ln + 1..map_end)
        .find(|&i| indent(lines[i]) == entry_ind && lines[i].trim_start().starts_with(&ehead));
    let Some(entry_ln) = entry_ln else {
        let v = value?;
        return Some((
            at(map_end),
            at(map_end),
            format!("{}{ehead} {v}\n", " ".repeat(entry_ind)),
        ));
    };

    // The entry's block ends at the next line at ≤ entry_ind — scalar
    // entries occupy exactly one line, but tolerate deeper children.
    let block_end = (entry_ln + 1..map_end)
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) <= entry_ind)
        .unwrap_or(map_end);

    match value {
        Some(v) => Some((
            offs[entry_ln],
            offs[block_end],
            format!("{}{ehead} {v}\n", " ".repeat(entry_ind)),
        )),
        None => {
            let other_entries = (map_ln + 1..map_end)
                .filter(|&i| {
                    !lines[i].trim().is_empty() && indent(lines[i]) == entry_ind && i != entry_ln
                })
                .count();
            if other_entries > 0 {
                Some((offs[entry_ln], offs[block_end], String::new()))
            } else {
                Some((offs[map_ln], offs[map_end], String::new()))
            }
        }
    }
}

/// Text-splice `properties: {prop: {displayName: v}}` at spec root —
/// the header "Rename column" write path. Creates `properties:` (and
/// the entry) before `views:` when absent; `None` deletes the
/// displayName line, the entry when it's left empty, and `properties:`
/// when it was the last entry. Returns `(byte_start, byte_end,
/// replacement)` or None.
pub fn splice_root_display_name(
    src: &str,
    prop: &str,
    value: Option<&str>,
) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut offs = Vec::with_capacity(lines.len() + 1);
    offs.push(0usize);
    for l in &lines {
        offs.push(offs.last().unwrap() + l.len());
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let at = |i: usize| offs.get(i).copied().unwrap_or(src.len());
    let ehead = format!("{}:", yaml_name(prop));

    // Find the top-level `properties:` key.
    let props_ln = lines
        .iter()
        .position(|l| indent(l) == 0 && l.trim_start().starts_with("properties:"));
    let Some(props_ln) = props_ln else {
        // No `properties:` — create it before `views:` (or at EOF).
        let v = value?;
        let views_ln = lines
            .iter()
            .position(|l| indent(l) == 0 && l.trim_start().starts_with("views:"));
        let at = views_ln.map(at).unwrap_or(src.len());
        return Some((
            at,
            at,
            format!(
                "properties:\n  {ehead}\n    displayName: {}\n",
                yaml_name(v)
            ),
        ));
    };

    // The map's entries sit one level deeper than the key; detect the
    // first entry's indent so non-2-space YAML still splices cleanly.
    let map_end = (props_ln + 1..lines.len())
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) == 0)
        .unwrap_or(lines.len());
    let entry_ind = (props_ln + 1..map_end)
        .filter(|&i| !lines[i].trim().is_empty())
        .map(|i| indent(lines[i]))
        .next()
        .unwrap_or(2);
    let disp_ind = entry_ind + 2;

    // Locate the `{prop}:` entry inside the map.
    let entry_ln = (props_ln + 1..map_end)
        .find(|&i| indent(lines[i]) == entry_ind && lines[i].trim_start().starts_with(&ehead));
    let Some(entry_ln) = entry_ln else {
        // Entry missing — append `{prop}:\n  displayName:` at map end.
        let v = value?;
        return Some((
            at(map_end),
            at(map_end),
            format!(
                "{}{ehead}\n{}displayName: {}\n",
                " ".repeat(entry_ind),
                " ".repeat(disp_ind),
                yaml_name(v)
            ),
        ));
    };

    // The entry's own block ends at the next line at ≤ entry_ind.
    let block_end = (entry_ln + 1..map_end)
        .find(|&i| !lines[i].trim().is_empty() && indent(lines[i]) <= entry_ind)
        .unwrap_or(map_end);
    // Does the entry already carry a `displayName:` line?
    let disp_ln = (entry_ln + 1..block_end).find(|&i| {
        indent(lines[i]) == disp_ind && lines[i].trim_start().starts_with("displayName:")
    });
    // Entry lines besides the key line itself.
    let siblings = (entry_ln + 1..block_end)
        .filter(|&i| {
            !lines[i].trim().is_empty() && indent(lines[i]) == disp_ind && Some(i) != disp_ln
        })
        .count();

    match (disp_ln, value) {
        // Rewrite the displayName in place.
        (Some(d), Some(v)) => Some((
            offs[d],
            offs[d + 1],
            format!("{}displayName: {}\n", " ".repeat(disp_ind), yaml_name(v)),
        )),
        // Clear it — drop the entry too when displayName was its only
        // key, and `properties:` when it was the last entry.
        (Some(d), None) => {
            if siblings > 0 {
                return Some((offs[d], offs[d + 1], String::new()));
            }
            let other_entries = (props_ln + 1..map_end)
                .filter(|&i| {
                    !lines[i].trim().is_empty() && indent(lines[i]) == entry_ind && i != entry_ln
                })
                .count();
            if other_entries > 0 {
                Some((offs[entry_ln], offs[block_end], String::new()))
            } else {
                Some((offs[props_ln], offs[map_end], String::new()))
            }
        }
        // Entry exists but lacks displayName — insert under it.
        (None, Some(v)) => Some((
            offs[entry_ln + 1],
            offs[entry_ln + 1],
            format!("{}displayName: {}\n", " ".repeat(disp_ind), yaml_name(v)),
        )),
        (None, None) => None,
    }
}

/// Text-splice `prop` onto the `order:` list of the `view_ix`-th view in
/// a `.base` spec — the column-chooser write path. Returns
/// `(byte_start, byte_end, replacement)` for select-and-replace, or None
/// when the view can't be located. Text-level (not serde) so comments
/// and formatting elsewhere in the spec survive.
pub fn splice_order(src: &str, view_ix: usize, prop: &str) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";

    // An existing `order:` key inside this view item — block list or
    // flow form — wins over appending a fresh key at the item's end.
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if !t.starts_with("order:") || indent(lines[i]) != key_ind {
            continue;
        }
        let after = t["order:".len()..].trim();
        if after.starts_with('[') {
            // Flow: rewrite the one line with the prop appended.
            let inner = after
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim_end_matches(',')
                .trim();
            let items_str = if inner.is_empty() {
                prop.to_string()
            } else {
                format!("{inner}, {prop}")
            };
            return Some((
                offs[i],
                offs[i + 1],
                format!("{}order: [{items_str}]\n", " ".repeat(key_ind)),
            ));
        }
        // Block list: entries are `- ` lines indented under the key.
        let mut last_entry = i;
        for (j, line_j) in lines.iter().enumerate().take(end_ln).skip(i + 1) {
            let t = line_j.trim_end();
            if t.is_empty() {
                continue;
            }
            let ind = indent(line_j);
            if ind > key_ind && is_item(t.trim_start()) {
                last_entry = j;
            } else {
                break;
            }
        }
        let at = offs[last_entry + 1];
        return Some((at, at, format!("{}- {prop}\n", " ".repeat(key_ind + 2))));
    }

    // No `order:` — append one at the item's end (still its own keys).
    let at = offs[end_ln];
    Some((
        at,
        at,
        format!(
            "{}order:\n{}- {prop}\n",
            " ".repeat(key_ind),
            " ".repeat(key_ind + 2)
        ),
    ))
}

/// Like `splice_order` but inserts `prop` before (`after: false`) or
/// after `anchor`, an existing `order:` entry — the header
/// "Insert column left/right" write path. Falls back to appending when
/// the anchor isn't listed, and to creating `order:` when absent.
pub fn splice_order_at(
    src: &str,
    view_ix: usize,
    prop: &str,
    anchor: &str,
    after: bool,
) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";

    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if !t.starts_with("order:") || indent(lines[i]) != key_ind {
            continue;
        }
        let tail = t["order:".len()..].trim();
        if tail.starts_with('[') {
            // Flow form — split the items, insert around the anchor.
            let inner = tail
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim_end_matches(',')
                .trim();
            let mut items: Vec<String> = if inner.is_empty() {
                Vec::new()
            } else {
                inner.split(',').map(|s| s.trim().to_string()).collect()
            };
            let pos = items
                .iter()
                .position(|it| unquote(it) == anchor)
                .map(|p| if after { p + 1 } else { p })
                .unwrap_or(items.len());
            items.insert(pos, prop.to_string());
            return Some((
                offs[i],
                offs[i + 1],
                format!("{}order: [{}]\n", " ".repeat(key_ind), items.join(", ")),
            ));
        }
        // Block list — find the anchor's `- ` line; insert before its
        // offset or after it, appending when the anchor isn't listed.
        let mut anchor_ln = None;
        let mut last_entry = i;
        for (j, line_j) in lines.iter().enumerate().take(end_ln).skip(i + 1) {
            let t = line_j.trim_end();
            if t.is_empty() {
                continue;
            }
            if indent(line_j) > key_ind && is_item(t.trim_start()) {
                last_entry = j;
                if unquote(t.trim_start()["- ".len()..].trim()) == anchor {
                    anchor_ln = Some(j);
                }
            } else {
                break;
            }
        }
        let at = match anchor_ln {
            Some(l) if after => offs[l + 1],
            Some(l) => offs[l],
            None => offs[last_entry + 1],
        };
        return Some((at, at, format!("{}- {prop}\n", " ".repeat(key_ind + 2))));
    }

    // No `order:` — append one at the item's end.
    let at = offs[end_ln];
    Some((
        at,
        at,
        format!(
            "{}order:\n{}- {prop}\n",
            " ".repeat(key_ind),
            " ".repeat(key_ind + 2)
        ),
    ))
}

/// A YAML scalar with one level of matching quotes stripped — `order:`
/// entries may be `- "my prop"` or `['a']`.
fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(s)
}

/// Text-splice the removal of `prop` from the `view_ix`-th view's
/// `order:` — the header "Hide column" write path. When the view has
/// no `order:` (everything on show), writes `current_cols` minus
/// `prop` as a fresh block list. Emptying the list collapses to
/// `order: [file.name]` so the table keeps its row key.
pub fn drop_order(
    src: &str,
    view_ix: usize,
    prop: &str,
    current_cols: &[String],
) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";

    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if !t.starts_with("order:") || indent(lines[i]) != key_ind {
            continue;
        }
        let after = t["order:".len()..].trim();
        if after.starts_with('[') {
            // Flow — rebuild the line without `prop`.
            let inner = after
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim_end_matches(',')
                .trim();
            let listed: Vec<&str> = inner
                .split(',')
                .map(|s| unquote(s.trim()))
                .filter(|s| !s.is_empty())
                .collect();
            if !listed.contains(&prop) {
                return None;
            }
            let kept: Vec<&str> = listed.into_iter().filter(|c| *c != prop).collect();
            let items_str = if kept.is_empty() {
                "file.name".to_string()
            } else {
                kept.join(", ")
            };
            return Some((
                offs[i],
                offs[i + 1],
                format!("{}order: [{items_str}]\n", " ".repeat(key_ind)),
            ));
        }
        // Block list — delete the `- prop` line, or collapse a
        // single-entry list to `[file.name]`.
        let mut entries: Vec<usize> = Vec::new();
        for (j, line_j) in lines.iter().enumerate().take(end_ln).skip(i + 1) {
            let t = line_j.trim_end();
            if t.is_empty() {
                continue;
            }
            let ind = indent(line_j);
            if ind > key_ind && is_item(t.trim_start()) {
                entries.push(j);
            } else {
                break;
            }
        }
        let hit = entries
            .iter()
            .copied()
            .find(|&j| unquote(lines[j].trim_start().trim_start_matches('-').trim()) == prop)?;
        if entries.len() == 1 {
            return Some((
                offs[i],
                offs[entries[0] + 1],
                format!("{}order: [file.name]\n", " ".repeat(key_ind)),
            ));
        }
        return Some((offs[hit], offs[hit + 1], String::new()));
    }

    // No `order:` — the displayed set is `current_cols`; write it
    // minus `prop` so only that column disappears.
    let mut kept: Vec<&str> = current_cols
        .iter()
        .map(|c| c.as_str())
        .filter(|c| *c != prop)
        .collect();
    if kept.is_empty() {
        kept.push("file.name");
    }
    let mut insert = format!("{}order:\n", " ".repeat(key_ind));
    for c in kept {
        insert.push_str(&format!("{}- {c}\n", " ".repeat(key_ind + 2)));
    }
    let at = offs[end_ln];
    Some((at, at, insert))
}

/// Text-splice a full column sequence into the `view_ix`-th view's
/// `order:` — the header drag-reorder write path. An existing key's
/// span (block entries or the one flow line) is replaced with a
/// canonical block list; a view with no `order:` gains one at its end.
pub fn reorder_order(src: &str, view_ix: usize, cols: &[String]) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";
    let block = || {
        let mut s = format!("{}order:\n", " ".repeat(key_ind));
        for c in cols {
            s.push_str(&format!("{}- {c}\n", " ".repeat(key_ind + 2)));
        }
        s
    };
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if !t.starts_with("order:") || indent(lines[i]) != key_ind {
            continue;
        }
        let after = t["order:".len()..].trim();
        if after.starts_with('[') {
            return Some((offs[i], offs[i + 1], block()));
        }
        let mut last = i;
        for (j, line_j) in lines.iter().enumerate().take(end_ln).skip(i + 1) {
            let t = line_j.trim_end();
            if t.is_empty() {
                continue;
            }
            if indent(line_j) > key_ind && is_item(t.trim_start()) {
                last = j;
            } else {
                break;
            }
        }
        return Some((offs[i], offs[last + 1], block()));
    }
    let at = offs[end_ln];
    Some((at, at, block()))
}

/// Text-splice a fresh view item onto the top-level `views:` list —
/// the "+ view" write path. A spec with no `views:` key gains one at
/// the end of the file; the item's indent follows the existing items'
/// (defaulting to the `views:` indent + 2 for an empty list). `name`
/// is quoted only when it needs to be.
pub fn splice_view(src: &str, name: &str, kind: &str) -> Option<(usize, usize, String)> {
    let lines: Vec<&str> = src.split_inclusive('\n').collect();
    let mut offs = Vec::with_capacity(lines.len() + 1);
    offs.push(0usize);
    for l in &lines {
        offs.push(offs.last().unwrap() + l.len());
    }
    let indent = |l: &str| l.len() - l.trim_start().len();
    let is_item = |t: &str| t.starts_with("- ") || t == "-";
    let name_yaml = yaml_name(name);

    let Some(views_ln) = lines
        .iter()
        .position(|l| l.trim_start().starts_with("views:"))
    else {
        let mut text = String::new();
        if !src.is_empty() && !src.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "views:\n  - type: {kind}\n    name: {name_yaml}\n"
        ));
        return Some((src.len(), src.len(), text));
    };

    let views_ind = indent(lines[views_ln]);
    let mut item_ind = None;
    let mut end_ln = lines.len();
    for (i, line) in lines.iter().enumerate().skip(views_ln + 1) {
        let t = line.trim_end();
        if t.is_empty() || t.trim_start().starts_with('#') {
            continue;
        }
        if indent(line) <= views_ind {
            end_ln = i;
            break;
        }
        if item_ind.is_none() && is_item(t.trim_start()) {
            item_ind = Some(indent(line));
        }
    }
    let ind = item_ind.unwrap_or(views_ind + 2);
    let mut item = format!(
        "{}- type: {kind}\n{}  name: {name_yaml}\n",
        " ".repeat(ind),
        " ".repeat(ind)
    );
    if end_ln == lines.len() && !src.ends_with('\n') {
        item.insert(0, '\n');
    }
    let at = offs[end_ln];
    Some((at, at, item))
}

/// `name` rendered as a YAML scalar — unquoted when safe.
fn yaml_name(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'))
    {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Rewrite the `view_ix`-th view's `name:` — the tab context-menu
/// "Rename view" write path. A view without a `name:` key gets one
/// right after its `type:` line.
pub fn splice_name(src: &str, view_ix: usize, name: &str) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let value = yaml_name(name);
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if indent(lines[i]) == key_ind && t.starts_with("name:") {
            return Some((
                offs[i],
                offs[i + 1],
                format!("{}name: {value}\n", " ".repeat(key_ind)),
            ));
        }
    }
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if indent(lines[i]) == key_ind && t.starts_with("type:") {
            return Some((
                offs[i + 1],
                offs[i + 1],
                format!("{}name: {value}\n", " ".repeat(key_ind)),
            ));
        }
    }
    None
}

/// Delete the `view_ix`-th view item outright — the tab context-menu
/// "Delete view" write path.
pub fn drop_view(src: &str, view_ix: usize) -> Option<(usize, usize, String)> {
    let (_, offs, start_ln, end_ln, _) = view_item(src, view_ix)?;
    Some((offs[start_ln], offs[end_ln], String::new()))
}

/// Copy the `view_ix`-th view item right after itself, renamed —
/// the tab context-menu "Duplicate view" write path. An item with
/// no `name:` line gains one after `type:`.
pub fn duplicate_view(src: &str, view_ix: usize, name: &str) -> Option<(usize, usize, String)> {
    let (lines, offs, start_ln, end_ln, key_ind) = view_item(src, view_ix)?;
    let indent = |l: &str| l.len() - l.trim_start().len();
    let mut item = String::new();
    let mut named = false;
    for i in start_ln..end_ln {
        let t = lines[i].trim_start();
        if !named && indent(lines[i]) == key_ind && t.starts_with("name:") {
            item.push_str(&format!(
                "{}name: {}\n",
                " ".repeat(key_ind),
                yaml_name(name)
            ));
            named = true;
        } else {
            item.push_str(lines[i]);
        }
    }
    if !named {
        let mut out = String::new();
        for l in item.split_inclusive('\n') {
            out.push_str(l);
            if !named && l.trim_start().starts_with("type:") {
                out.push_str(&format!(
                    "{}name: {}\n",
                    " ".repeat(key_ind),
                    yaml_name(name)
                ));
                named = true;
            }
        }
        item = out;
    }
    let at = offs[end_ln];
    Some((at, at, item))
}

/// Move the `from`-th view item to position `to` — the tab
/// drag-reorder write path. The item's whole text span travels,
/// comments and formatting included; content outside the `views:`
/// list is untouched.
pub fn reorder_views(src: &str, from: usize, to: usize) -> Option<(usize, usize, String)> {
    let (lines, offs, items, _) = view_items(src)?;
    if from == to || from >= items.len() || to >= items.len() {
        return None;
    }
    let views_ind = lines
        .iter()
        .position(|l| l.trim_start().starts_with("views:"))
        .map(|i| lines[i].len() - lines[i].trim_start().len())?;
    let spans: Vec<(usize, usize)> = (0..items.len())
        .map(|i| view_span(&lines, &items, i, views_ind))
        .collect();
    let mut order: Vec<usize> = (0..items.len()).collect();
    let v = order.remove(from);
    order.insert(to, v);
    let mut mid = String::new();
    for &i in &order {
        mid.push_str(&src[offs[spans[i].0]..offs[spans[i].1]]);
    }
    Some((offs[spans[0].0], offs[spans[items.len() - 1].1], mid))
}

// ------------------------------------------------------------------
// View — renders inside the workspace for `.base` documents.
// ------------------------------------------------------------------

/// Where the base spec comes from: a `.base` document, or an inline
/// ```` ```base ```` code fence embedded in a note.
enum SpecSrc {
    Doc(Entity<Document>),
    Inline(String),
}

pub struct BaseView {
    spec_src: SpecSrc,
    workspace: WeakEntity<Workspace>,
    vault: Entity<Vault>,
    view_ix: usize,
    /// ```` ```base ```` embeds only: the note hosting the fence —
    /// bound as `this` in filters/formulas.
    this_path: Option<PathBuf>,
    /// Calendar view: months offset from the current month.
    cal_offset: i32,
    /// Collapsed table group bands — (view index, band text).
    collapsed_groups: std::collections::HashSet<(usize, String)>,
    /// Interactive header sort: (column index, descending).
    sort: Option<(usize, bool)>,
    /// Per-view search box — the base search; filters rows live
    /// on note name + every displayed cell.
    search: Entity<InputState>,
    /// View-tab "Rename view…" dialog input.
    rename_input: Entity<InputState>,
    /// In-progress column drag-resize: (column ix, grab x, start
    /// width, live width). Written to `columnSize:` on release.
    col_resize: Option<(usize, f32, f32, f32)>,
    notes_epoch: u64,
    doc_epoch: u64,
    cache_key: Option<(u64, u64, usize)>,
    cached: Option<std::rc::Rc<Computed>>,
    _subscriptions: Vec<Subscription>,
}

impl BaseView {
    /// `saved_view_ix` — the view index persisted for this file (from
    /// `Settings::base_views`); the caller reads it because this runs
    /// inside the workspace's update borrow.
    /// `saved_sorts` — `(view_name, column_header, descending)` rows
    /// snapshotted from settings by the caller: Workspace can't be
    /// read while its update borrow is held.
    pub fn new(
        doc: Entity<Document>,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
        saved_view_ix: usize,
        saved_sorts: Vec<(String, String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let doc_sub = cx.subscribe_in(&doc, window, |this, _doc, event, _window, cx| {
            if matches!(
                event,
                crate::document::DocumentEvent::Changed | crate::document::DocumentEvent::Saved
            ) {
                this.doc_epoch += 1;
                cx.notify();
            }
        });
        let mut view = Self::init(SpecSrc::Doc(doc), vault, workspace, window, cx);
        view.view_ix = saved_view_ix;
        // Restore a header sort saved for this file + view.
        if !saved_sorts.is_empty() {
            let computed = view.computed(cx);
            if let Some(view_name) = computed.view_names.get(view.view_ix) {
                if let Some((_, col, desc)) = saved_sorts.iter().find(|(v, _, _)| v == view_name) {
                    if let Some(ix) = computed.headers.iter().position(|h| h == col) {
                        view.sort = Some((ix, *desc));
                    }
                }
            }
        }
        view._subscriptions.push(doc_sub);
        view
    }

    /// Inline embed — spec text fixed at creation; the parent Document
    /// swaps in a fresh view when the fence's contents change.
    pub fn for_inline(
        spec: String,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
        this_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::init(SpecSrc::Inline(spec), vault, workspace, window, cx);
        view.this_path = this_path;
        view
    }

    fn init(
        spec_src: SpecSrc,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let vault_sub = cx.subscribe_in(&vault, window, |this, _vault, _event, _window, cx| {
            this.notes_epoch += 1;
            cx.notify();
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search or prop=value"));
        let search_sub = cx.subscribe(&search, |_this, _search, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        Self {
            spec_src,
            workspace,
            vault,
            view_ix: 0,
            this_path: None,
            cal_offset: 0,
            collapsed_groups: std::collections::HashSet::new(),
            sort: None,
            search,
            rename_input: cx.new(|cx| InputState::new(window, cx)),
            col_resize: None,
            notes_epoch: 0,
            doc_epoch: 0,
            cache_key: None,
            cached: None,
            _subscriptions: vec![vault_sub, search_sub],
        }
    }

    /// Name of the spec's view at `view_ix`, when the source parses.
    fn spec_view_name(&self, cx: &App) -> Option<String> {
        let yaml = match &self.spec_src {
            SpecSrc::Doc(doc) => doc.read(cx).editor.read(cx).value().to_string(),
            SpecSrc::Inline(spec) => spec.clone(),
        };
        parse_spec(&yaml)
            .views
            .get(self.view_ix)
            .map(|v| v.name.clone())
    }

    /// ⌘F on a rendered `.base` view focuses its row filter box
    ///.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    /// Set the interactive header sort — `desc` `None` clears. On doc
    /// specs the choice persists per (file, view, header-name) in
    /// settings; the header name is robust to view reorders.
    fn apply_header_sort(
        &mut self,
        ix: usize,
        desc: Option<bool>,
        header: String,
        cx: &mut Context<Self>,
    ) {
        self.sort = desc.map(|d| (ix, d));
        if let SpecSrc::Doc(doc) = &self.spec_src {
            let path = doc.read(cx).path.to_string_lossy().to_string();
            if let (Some(ws), Some(vn)) = (self.workspace.upgrade(), self.spec_view_name(cx)) {
                let saved = desc.map(|d| (header, d));
                ws.update(cx, |ws, _cx| ws.remember_base_sort(path, vn, saved));
            }
        }
        cx.notify();
    }

    /// `![[db.base#View]]` — switch the embed to the named view
    /// (case-insensitive). Unknown names keep the first view.
    pub fn select_view_by_name(&mut self, name: &str, cx: &mut Context<Self>) {
        let yaml = match &self.spec_src {
            SpecSrc::Doc(doc) => doc.read(cx).editor.read(cx).value().to_string(),
            SpecSrc::Inline(spec) => spec.clone(),
        };
        let spec = parse_spec(&yaml);
        if let Some(ix) = spec
            .views
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
        {
            self.view_ix = ix;
            self.cached = None;
            cx.notify();
        }
    }

    fn computed(&mut self, cx: &App) -> std::rc::Rc<Computed> {
        let key = (self.doc_epoch, self.notes_epoch, self.view_ix);
        if self.cache_key == Some(key) {
            if let Some(cached) = &self.cached {
                return cached.clone();
            }
        }
        let (yaml, doc_path) = match &self.spec_src {
            SpecSrc::Doc(doc) => {
                let doc = doc.read(cx);
                (
                    doc.editor.read(cx).value().to_string(),
                    Some(doc.path.clone()),
                )
            }
            SpecSrc::Inline(spec) => (spec.clone(), None),
        };
        let spec = parse_spec(&yaml);
        // A mid-reload or invalid spec can leave no views at all —
        // render the error state instead of indexing `len() - 1`.
        if spec.views.is_empty() {
            let computed = std::rc::Rc::new(Computed {
                headers: Vec::new(),
                columns: Vec::new(),
                rows: Vec::new(),
                view_names: Vec::new(),
                view_kinds: Vec::new(),
                kind: "table".into(),
                group_ix: None,
                grouped: false,
                group_desc: false,
                group_prop: None,
                prefill: Vec::new(),
                available: Vec::new(),
                groupable: Vec::new(),
                sort_spec: None,
                filters_by_view: Vec::new(),
                view_filters: Vec::new(),
                view_columns: Vec::new(),
                formula_srcs: BTreeMap::new(),
                summaries: Vec::new(),
                image_fit: None,
                image_aspect: None,
                col_size: Vec::new(),
                error: Some(
                    spec.error
                        .clone()
                        .unwrap_or_else(|| "no views defined".into()),
                ),
            });
            self.cache_key = Some(key);
            self.cached = Some(computed.clone());
            return computed;
        }
        let view = &spec.views[self.view_ix.min(spec.views.len() - 1)];
        let (notes, root, images, starred) = {
            let vault = self.vault.read(cx);
            (
                vault.notes.clone(),
                vault.root.clone().unwrap_or_default(),
                vault.images.clone(),
                vault.starred.clone(),
            )
        };
        // The base file itself never belongs in its own result set.
        let notes: Vec<_> = notes
            .into_iter()
            .filter(|n| Some(n) != doc_path.as_ref())
            .collect();
        let computed = std::rc::Rc::new(compute(
            &spec,
            view,
            &notes,
            &root,
            &images.borrow(),
            &starred,
            self.this_path.as_deref(),
        ));
        self.cache_key = Some(key);
        self.cached = Some(computed.clone());
        computed
    }
}

impl BaseView {
    /// `type: calendar` — a month grid bucketing rows on the view's date
    /// property (`date:`/`dateProperty:` in the spec; falls back to a
    /// `date`/`due`/`deadline` column). Rows without a parseable date
    /// don't appear on the grid but count in the footer.
    fn render_calendar(
        &self,
        this: &Entity<Self>,
        computed: &Computed,
        rows: &[&Row],
        cx: &App,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let today = chrono::Local::now().date_naive();
        let month0 = today.with_day(1).unwrap_or(today);
        let shown = if self.cal_offset >= 0 {
            month0 + chrono::Months::new(self.cal_offset as u32)
        } else {
            month0 - chrono::Months::new((-self.cal_offset) as u32)
        };
        let first = shown.with_day(1).unwrap_or(shown);
        let lead = first.weekday().num_days_from_monday() as i64;
        let grid_start = first - chrono::Duration::days(lead);
        let month = first.month();

        // Bucket rows by parsed date.
        let gix = computed.group_ix.unwrap_or(usize::MAX);
        let mut by_day: std::collections::HashMap<chrono::NaiveDate, Vec<(String, PathBuf)>> =
            std::collections::HashMap::new();
        for row in rows {
            let Some(day) = row.cells.get(gix).and_then(|c| parse_date(&c.text)) else {
                continue;
            };
            let title = row
                .cells
                .first()
                .map(|c| c.text.clone())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| stem_of(&row.path.to_string_lossy()));
            by_day
                .entry(day)
                .or_default()
                .push((title, row.path.clone()));
        }

        // Month navigation.
        let nav = h_flex().w_full().px_3().py_2().justify_between().child(
            div()
                .text_sm()
                .font_semibold()
                .text_color(theme.foreground)
                .child(first.format("%B %Y").to_string()),
        );
        let nav = nav.child(
            h_flex().gap_2().children(
                [("cal-today", "Today"), ("cal-prev", "‹"), ("cal-next", "›")]
                    .into_iter()
                    .map(|(id, label)| {
                        let this = this.clone();
                        div()
                            .id((id, 0usize))
                            .px_2()
                            .py(px(2.))
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .child(label)
                            .on_click(move |_, _window, cx| {
                                this.update(cx, |view, cx| {
                                    match id {
                                        "cal-today" => view.cal_offset = 0,
                                        "cal-prev" => view.cal_offset -= 1,
                                        _ => view.cal_offset += 1,
                                    }
                                    cx.notify();
                                });
                            })
                    }),
            ),
        );

        let root = self.vault.read(cx).root.clone();
        let mut grid = v_flex().w_full().border_1().border_color(theme.border);
        // Weekday header.
        let mut wk = h_flex().w_full().border_b_1().border_color(theme.border);
        for (i, name) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            .iter()
            .enumerate()
        {
            let mut cell = div().flex_1().min_w_0().py_1().text_center();
            if i < 6 {
                cell = cell.border_r_1().border_color(theme.border);
            }
            wk = wk.child(
                cell.text_xs()
                    .text_color(theme.muted_foreground)
                    .child(*name),
            );
        }
        grid = grid.child(wk);

        // 6 week rows.
        for w in 0..6 {
            let mut row_el = h_flex().w_full();
            if w < 5 {
                row_el = row_el.border_b_1().border_color(theme.border);
            }
            for c in 0..7 {
                let day = grid_start + chrono::Duration::days((w * 7 + c) as i64);
                let in_month = day.month() == month;
                let is_today = day == today;
                let items = by_day.get(&day).cloned().unwrap_or_default();
                let mut cell = div()
                    .id(("cal-day", (w * 7 + c) as usize))
                    .flex_1()
                    .min_w_0()
                    .h(px(84.))
                    .p_1()
                    .v_flex()
                    .gap_0p5();
                if c < 6 {
                    cell = cell.border_r_1().border_color(theme.border);
                }
                if !in_month {
                    cell = cell.bg(theme.secondary.opacity(0.4));
                }
                if is_today {
                    cell = cell.border_color(theme.accent);
                }
                let num = div()
                    .w_full()
                    .text_right()
                    .text_xs()
                    .text_color(if is_today {
                        theme.accent
                    } else if in_month {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(is_today, |d| d.font_semibold())
                    .child(format!("{}", day.day()));
                cell = cell.child(num);
                for (k, (title, path)) in items.iter().take(2).enumerate() {
                    let path = path.clone();
                    let workspace = self.workspace.clone();
                    cell = cell.child(
                        div()
                            .id(("cal-note", (w * 7 + c) as usize * 16 + k))
                            .w_full()
                            .px_1()
                            .rounded(px(3.))
                            .bg(theme.accent)
                            .text_xs()
                            .truncate()
                            .text_color(theme.accent_foreground)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.muted.opacity(0.5)))
                            .child(title.clone())
                            .on_click(move |ev, window, cx| {
                                open_path_click(&workspace, path.clone(), ev, window, cx)
                            }),
                    );
                }
                if items.len() > 2 {
                    cell = cell.child(
                        div()
                            .w_full()
                            .px_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("+{} more", items.len() - 2)),
                    );
                }
                // Empty-cell click → create/open that day's note
                // (`YYYY-MM-DD.md`), like the calendar.
                let workspace = self.workspace.clone();
                let root = root.clone();
                cell = cell.cursor_pointer().on_click(move |_, window, cx| {
                    if root.is_some() {
                        let _ = workspace.update(cx, |ws, cx| ws.open_daily_at(day, window, cx));
                    }
                });
                row_el = row_el.child(cell);
            }
            grid = grid.child(row_el);
        }

        v_flex()
            .w_full()
            .child(nav)
            .child(div().w_full().px_3().pb_2().child(grid))
    }
}

impl Render for BaseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let computed = self.computed(cx);
        let this = cx.entity();

        // Effective column sizes — `columnSize:` overlaid with the
        // live drag-resize width so the column tracks the pointer.
        let mut eff_sizes = computed.col_size.clone();
        if let Some((rix, _, _, w)) = self.col_resize {
            if let Some(s) = eff_sizes.get_mut(rix) {
                *s = Some(w);
            }
        }

        // Search box filter — whitespace-separated terms ANDed across
        // whatever the current view renders (table/cards/kanban/list/
        // calendar all iterate `visible`). `prop=value`, `prop!=value`,
        // `prop~text` and bare `prop=` (empty cell) match a named
        // column; any other term contains-matches the file stem + every
        // cell, like the quick filter.
        let query = self.search.read(cx).value().trim().to_lowercase();
        let visible: Vec<&Row> = computed
            .rows
            .iter()
            .filter(|r| {
                query
                    .split_whitespace()
                    .all(|term| search_term_match(r, &computed, term))
            })
            .collect();

        // Toolbar row: view switcher on the left, "new note" on the
        // right. Views only appear when the spec declares >1.
        let mut tabs = h_flex().gap_1();
        let view_icon = |kind: Option<&str>| match kind {
            Some("cards") | Some("gallery") => assets::IconName::GalleryVerticalEnd,
            Some("kanban") | Some("board") => assets::IconName::SquareKanban,
            Some("calendar") => assets::IconName::Calendar,
            Some("list") => assets::IconName::List,
            _ => assets::IconName::Table,
        };
        if computed.view_names.len() > 1 || matches!(self.spec_src, SpecSrc::Doc(_)) {
            for (ix, name) in computed.view_names.iter().enumerate() {
                let selected = ix == self.view_ix;
                let icon = view_icon(computed.view_kinds.get(ix).map(String::as_str));
                tabs = tabs.child(
                    div()
                        .id(("base-view", ix))
                        .px_2()
                        .py_0p5()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .text_xs()
                        .when(selected, |d| {
                            d.bg(theme.secondary).text_color(theme.foreground)
                        })
                        .when(!selected, |d| d.text_color(theme.muted_foreground))
                        .child(
                            h_flex()
                                .gap_1()
                                .items_center()
                                .child(Icon::new(icon).size_3p5())
                                .child(name.clone()),
                        )
                        .on_click({
                            let this = this.clone();
                            move |_, _window, cx| {
                                this.update(cx, |view, cx| {
                                    view.view_ix = ix;
                                    view.sort = None;
                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                        let key = doc.read(cx).path.to_string_lossy().to_string();
                                        if let Some(ws) = view.workspace.upgrade() {
                                            ws.update(cx, |ws, _cx| {
                                                ws.remember_base_view(key, ix);
                                            });
                                        }
                                    }
                                    cx.notify();
                                });
                            }
                        })
                        // Drag a tab onto another → the view takes that
                        // slot; the item's whole span moves in `views:`.
                        // `.base` files only — inline fences are read-only.
                        .when(matches!(self.spec_src, SpecSrc::Doc(_)), |d| {
                            let this = this.clone();
                            d.on_drag(ViewDrag(ix), {
                                let label: SharedString = name.clone().into();
                                move |_, _, _, cx| {
                                    cx.new(|_| KanbanDrag {
                                        label: label.clone(),
                                    })
                                }
                            })
                            .drag_over::<ViewDrag>(|style, _, _, cx| {
                                style.border_color(cx.theme().accent)
                            })
                            .on_drop::<ViewDrag>(move |src: &ViewDrag, window, cx| {
                                this.update(cx, |view, cx| {
                                    let SpecSrc::Doc(doc) = &view.spec_src else {
                                        return;
                                    };
                                    let doc = doc.clone();
                                    let (from, to) = (src.0, ix);
                                    if from == to {
                                        return;
                                    }
                                    // Remap the selection across the move.
                                    let sel = view.view_ix;
                                    let new_sel = if sel == from {
                                        to
                                    } else if from < sel && sel <= to {
                                        sel - 1
                                    } else if to <= sel && sel < from {
                                        sel + 1
                                    } else {
                                        sel
                                    };
                                    let key =
                                        doc.read(cx).path.to_string_lossy().to_string();
                                    if doc.update(cx, |doc, cx| {
                                        doc.reorder_base_views(from, to, window, cx)
                                    }) {
                                        view.view_ix = new_sel;
                                        if let Some(ws) = view.workspace.upgrade() {
                                            ws.update(cx, |ws, _cx| {
                                                ws.remember_base_view(key, new_sel);
                                            });
                                        }
                                        view.doc_epoch += 1;
                                    }
                                });
                            })
                        })
                        // Right-click a tab → rename/delete the view it
                        // names (the view menu). Inline ```base
                        // embeds are read-only and get an empty menu.
                        .context_menu({
                            let this = this.clone();
                            let name = name.clone();
                            let names = computed.view_names.clone();
                            let multi = computed.view_names.len() > 1;
                            let groupable = computed.groupable.clone();
                            let sort_spec = computed.sort_spec.clone();
                            let is_cal = computed.view_kinds.get(ix).map(String::as_str)
                                == Some("calendar");
                            let filtered = computed
                                .filters_by_view
                                .get(ix)
                                .copied()
                                .unwrap_or(false);
                            let filter_terms = computed
                                .view_filters
                                .get(ix)
                                .cloned()
                                .unwrap_or_default();
                            let view_cols = computed
                                .view_columns
                                .get(ix)
                                .cloned()
                                .unwrap_or_default();
                            let has_art = matches!(
                                computed.view_kinds.get(ix).map(String::as_str),
                                Some("cards") | Some("kanban")
                            );
                            move |menu, _window, cx| {
                                if !matches!(this.read(cx).spec_src, SpecSrc::Doc(_)) {
                                    return menu;
                                }
                                let menu = menu.item(
                                    PopupMenuItem::new("Rename view…")
                                        .icon(assets::IconName::SquarePen)
                                        .on_click({
                                            let this = this.clone();
                                            let name = name.clone();
                                            move |_, window, cx| {
                                                let input =
                                                    this.read(cx).rename_input.clone();
                                                input.update(cx, |input, cx| {
                                                    input.set_value(&name, window, cx);
                                                });
                                                let this = this.clone();
                                                let input2 = input.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let input = input.clone();
                                                        dialog
                                                            .title("Rename view")
                                                            .w(px(320.))
                                                            .child(
                                                                div().w_full().child(
                                                                    Input::new(&input)
                                                                        .appearance(true),
                                                                ),
                                                            )
                                                            .on_ok({
                                                                let this = this.clone();
                                                                move |_, window, cx| {
                                                                    this.update(
                                                                        cx,
                                                                        |view, cx| {
                                                                            let text = view
                                                                                .rename_input
                                                                                .read(cx)
                                                                                .value()
                                                                                .trim()
                                                                                .to_string();
                                                                            if text.is_empty() {
                                                                                return;
                                                                            }
                                                                            if let SpecSrc::Doc(
                                                                                doc,
                                                                            ) = &view.spec_src
                                                                            {
                                                                                doc.update(
                                                                                    cx,
                                                                                    |doc, cx| {
                                                                                        doc.rename_base_view(
                                                                                            ix,
                                                                                            &text,
                                                                                            window,
                                                                                            cx,
                                                                                        );
                                                                                    },
                                                                                );
                                                                            }
                                                                            view.doc_epoch += 1;
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                    true
                                                                }
                                                            })
                                                    },
                                                );
                                                window.defer(cx, move |window, cx| {
                                                    input2.update(cx, |input, cx| {
                                                        input.focus(window, cx);
                                                    });
                                                });
                                            }
                                        }),
                                );
                                let menu = menu.item(
                                    PopupMenuItem::new("Duplicate view")
                                        .icon(assets::IconName::Copy)
                                        .on_click({
                                            let this = this.clone();
                                            let names = names.clone();
                                            move |_, window, cx| {
                                                this.update(cx, |view, cx| {
                                                    let mut name =
                                                        format!("{} copy", names[ix]);
                                                    let mut n = 2;
                                                    while names.iter().any(|t| t == &name) {
                                                        name = format!("{} copy {n}", names[ix]);
                                                        n += 1;
                                                    }
                                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                                        let key = doc
                                                            .read(cx)
                                                            .path
                                                            .to_string_lossy()
                                                            .to_string();
                                                        if doc.update(cx, |doc, cx| {
                                                            doc.duplicate_base_view(
                                                                ix, &name, window, cx,
                                                            )
                                                        }) {
                                                            // The copy lands right
                                                            // after its source —
                                                            // select it.
                                                            view.view_ix = ix + 1;
                                                            if let Some(ws) =
                                                                view.workspace.upgrade()
                                                            {
                                                                ws.update(cx, |ws, _cx| {
                                                                    ws.remember_base_view(
                                                                        key,
                                                                        ix + 1,
                                                                    );
                                                                });
                                                            }
                                                            view.doc_epoch += 1;
                                                        }
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                );
                                let menu = menu.item(
                                    PopupMenuItem::new("Group by…")
                                        .icon(assets::IconName::Group)
                                        .on_click({
                                            let this = this.clone();
                                            let candidates = groupable.clone();
                                            move |_, window, cx| {
                                                let candidates = candidates.clone();
                                                let this = this.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let theme = _cx.theme();
                                                        let mut list =
                                                            v_flex().w_full().py_1();
                                                        let this2 = this.clone();
                                                        let row = |label: String,
                                                                   row_ix: usize,
                                                                   value: Option<String>,
                                                                   theme: &gpui_kit::component::theme::Theme| {
                                                            let this = this2.clone();
                                                            div()
                                                                .id(("group-pick", row_ix))
                                                                .w_full()
                                                                .px_3()
                                                                .py_1p5()
                                                                .cursor_pointer()
                                                                .hover(|s| s.bg(theme.muted))
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .text_color(theme.foreground)
                                                                        .child(label),
                                                                )
                                                                .on_click(move |_, window, cx| {
                                                                    this.update(cx, |view, cx| {
                                                                        if let SpecSrc::Doc(doc) =
                                                                            &view.spec_src
                                                                        {
                                                                            doc.update(cx, |doc, cx| {
                                                                                doc.set_base_view_key(
                                                                                    ix,
                                                                                    "group_by",
                                                                                    value.as_deref(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            });
                                                                        }
                                                                        view.doc_epoch += 1;
                                                                        cx.notify();
                                                                    });
                                                                    window.close_dialog(cx);
                                                                })
                                                        };
                                                        list = list.child(row(
                                                            "No grouping".to_string(),
                                                            0,
                                                            None,
                                                            theme,
                                                        ));
                                                        for (nix, prop) in
                                                            candidates.iter().enumerate()
                                                        {
                                                            list = list.child(row(
                                                                prop.clone(),
                                                                nix + 1,
                                                                Some(prop.clone()),
                                                                theme,
                                                            ));
                                                        }
                                                        dialog
                                                            .title("Group by")
                                                            .w(px(320.))
                                                            .overlay_closable(true)
                                                            .child(
                                                                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                    list.max_h(px(320.)),
                                                                ),
                                                            )
                                                    },
                                                );
                                            }
                                        }),
                                );
                                let menu = menu.item(
                                    PopupMenuItem::new("Sort by…")
                                        .icon(assets::IconName::ArrowDownAZ)
                                        .on_click({
                                            let this = this.clone();
                                            let candidates = groupable.clone();
                                            let sort_spec = sort_spec.clone();
                                            move |_, window, cx| {
                                                let candidates = candidates.clone();
                                                let sort_spec = sort_spec.clone();
                                                let this = this.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let theme = _cx.theme();
                                                        let mut list =
                                                            v_flex().w_full().py_1();
                                                        let this2 = this.clone();
                                                        let row = |label: String,
                                                                   row_ix: usize,
                                                                   value: Option<String>,
                                                                   theme: &gpui_kit::component::theme::Theme| {
                                                            let this = this2.clone();
                                                            div()
                                                                .id(("sort-pick", row_ix))
                                                                .w_full()
                                                                .px_3()
                                                                .py_1p5()
                                                                .cursor_pointer()
                                                                .hover(|s| s.bg(theme.muted))
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .text_color(theme.foreground)
                                                                        .child(label),
                                                                )
                                                                .on_click(move |_, window, cx| {
                                                                    this.update(cx, |view, cx| {
                                                                        if let SpecSrc::Doc(doc) =
                                                                            &view.spec_src
                                                                        {
                                                                            doc.update(cx, |doc, cx| {
                                                                                doc.set_base_view_key(
                                                                                    ix,
                                                                                    "sort",
                                                                                    value.as_deref(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            });
                                                                        }
                                                                        view.doc_epoch += 1;
                                                                        cx.notify();
                                                                    });
                                                                    window.close_dialog(cx);
                                                                })
                                                        };
                                                        list = list.child(row(
                                                            "No sorting".to_string(),
                                                            0,
                                                            None,
                                                            theme,
                                                        ));
                                                        for (nix, prop) in
                                                            candidates.iter().enumerate()
                                                        {
                                                            // Clicking the prop the
                                                            // view already sorts on
                                                            // flips its direction.
                                                            let desc = sort_spec
                                                                .as_ref()
                                                                .map(|(p, d)| {
                                                                    p == prop && !*d
                                                                })
                                                                .unwrap_or(false);
                                                            let val = if desc {
                                                                format!("-{prop}")
                                                            } else {
                                                                prop.clone()
                                                            };
                                                            list = list.child(row(
                                                                prop.clone(),
                                                                nix + 1,
                                                                Some(val),
                                                                theme,
                                                            ));
                                                        }
                                                        dialog
                                                            .title("Sort by")
                                                            .w(px(320.))
                                                            .overlay_closable(true)
                                                            .child(
                                                                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                    list.max_h(px(320.)),
                                                                ),
                                                            )
                                                    },
                                                );
                                            }
                                        }),
                                );
                                let menu = menu.item(
                                    PopupMenuItem::new("Columns…")
                                        .icon(assets::IconName::Table2)
                                        .on_click({
                                            let this = this.clone();
                                            let candidates = groupable.clone();
                                            let view_cols = view_cols.clone();
                                            move |_, window, cx| {
                                                let candidates = candidates.clone();
                                                let view_cols = view_cols.clone();
                                                let this = this.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let theme = _cx.theme();
                                                        let mut list =
                                                            v_flex().w_full().py_1();
                                                        for (nix, prop) in
                                                            candidates.iter().enumerate()
                                                        {
                                                            if prop == "file.name" {
                                                                continue;
                                                            }
                                                            let checked =
                                                                view_cols.contains(prop);
                                                            let prop = prop.clone();
                                                            let this = this.clone();
                                                            let cols = view_cols.clone();
                                                            list = list.child(
                                                                div()
                                                                    .id(("col-pick", nix))
                                                                    .w_full()
                                                                    .px_3()
                                                                    .py_1p5()
                                                                    .cursor_pointer()
                                                                    .hover(|s| {
                                                                        s.bg(theme.muted)
                                                                    })
                                                                    .child(
                                                                        div()
                                                                            .text_sm()
                                                                            .text_color(theme.foreground)
                                                                            .child(if checked {
                                                                                format!("✓ {prop}")
                                                                            } else {
                                                                                format!("  {prop}")
                                                                            }),
                                                                    )
                                                                    .on_click(
                                                                        move |_, window, cx| {
                                                                            this.update(
                                                                                cx,
                                                                                |view, cx| {
                                                                                    if let SpecSrc::Doc(doc) =
                                                                                        &view.spec_src
                                                                                    {
                                                                                        doc.update(
                                                                                            cx,
                                                                                            |doc, cx| {
                                                                                                if checked {
                                                                                                    doc.remove_base_column(
                                                                                                        ix, &prop,
                                                                                                        &cols, window,
                                                                                                        cx,
                                                                                                    );
                                                                                                } else {
                                                                                                    doc.add_base_column(
                                                                                                        ix, &prop,
                                                                                                        window, cx,
                                                                                                    );
                                                                                                }
                                                                                            },
                                                                                        );
                                                                                    }
                                                                                    view.doc_epoch += 1;
                                                                                    cx.notify();
                                                                                },
                                                                            );
                                                                            window.close_dialog(cx);
                                                                        },
                                                                    ),
                                                            );
                                                        }
                                                        dialog
                                                            .title("Columns")
                                                            .w(px(320.))
                                                            .overlay_closable(true)
                                                            .child(
                                                                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                    list.max_h(px(320.)),
                                                                ),
                                                            )
                                                    },
                                                );
                                            }
                                        }),
                                );
                                let menu = menu.item(
                                    PopupMenuItem::new("Filter by…")
                                        .icon(assets::IconName::Funnel)
                                        .on_click({
                                            let this = this.clone();
                                            let candidates = groupable.clone();
                                            move |_, window, cx| {
                                                filter_pick_prop(
                                                    this.clone(),
                                                    ix,
                                                    candidates.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                );
                                let menu = if filtered {
                                    let menu = menu.item(
                                        PopupMenuItem::new("Remove filter…")
                                            .icon(assets::IconName::FunnelX)
                                            .on_click({
                                                let this = this.clone();
                                                // Expression texts in
                                                // file order — matches
                                                // the splice's index.
                                                let terms = filter_terms.clone();
                                                move |_, window, cx| {
                                                    let terms = terms.clone();
                                                    let this = this.clone();
                                                    window.open_dialog(
                                                        cx,
                                                        move |dialog, _window, _cx| {
                                                            let theme = _cx.theme();
                                                            let mut list =
                                                                v_flex().w_full().py_1();
                                                            for (tix, term) in
                                                                terms.iter().enumerate()
                                                            {
                                                                let term = term.clone();
                                                                let this = this.clone();
                                                                list = list.child(
                                                                    div()
                                                                        .id(("filter-rm", tix))
                                                                        .w_full()
                                                                        .px_3()
                                                                        .py_1p5()
                                                                        .cursor_pointer()
                                                                        .hover(|s| {
                                                                            s.bg(theme.muted)
                                                                        })
                                                                        .child(
                                                                            div()
                                                                                .text_sm()
                                                                                .text_color(
                                                                                    theme.foreground,
                                                                                )
                                                                                .child(
                                                                                    term.clone(),
                                                                                ),
                                                                        )
                                                                        .on_click(
                                                                            move |_, window, cx| {
                                                                                window.close_dialog(cx);
                                                                                this.update(cx, |view, cx| {
                                                                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                                                                        doc.update(cx, |doc, cx| {
                                                                                            doc.remove_base_view_filter(
                                                                                                ix, tix, window, cx,
                                                                                            );
                                                                                        });
                                                                                    }
                                                                                    view.doc_epoch += 1;
                                                                                    cx.notify();
                                                                                });
                                                                            },
                                                                        ),
                                                                );
                                                            }
                                                            dialog
                                                                .title("Remove filter")
                                                                .w(px(320.))
                                                                .overlay_closable(true)
                                                                .child(
                                                                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                        list.max_h(px(320.)),
                                                                    ),
                                                                )
                                                        },
                                                    );
                                                }
                                            }),
                                    );
                                    menu.item(
                                        PopupMenuItem::new("Clear filters")
                                            .icon(assets::IconName::FunnelX)
                                            .on_click({
                                                let this = this.clone();
                                                move |_, window, cx| {
                                                    this.update(cx, |view, cx| {
                                                        if let SpecSrc::Doc(doc) = &view.spec_src {
                                                            doc.update(cx, |doc, cx| {
                                                                doc.set_base_view_key(
                                                                    ix,
                                                                    "filters",
                                                                    None,
                                                                    window,
                                                                    cx,
                                                                );
                                                            });
                                                        }
                                                        view.doc_epoch += 1;
                                                        cx.notify();
                                                    });
                                                }
                                            }),
                                    )
                                } else {
                                    menu
                                };
                                let menu = menu.item(
                                    PopupMenuItem::new("Row limit…")
                                        .icon(assets::IconName::ArrowDownWideNarrow)
                                        .on_click({
                                            let this = this.clone();
                                            move |_, window, cx| {
                                                let input =
                                                    this.read(cx).rename_input.clone();
                                                input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx);
                                                });
                                                let this = this.clone();
                                                let input2 = input.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let input = input.clone();
                                                        dialog
                                                            .title("Row limit — blank clears")
                                                            .w(px(320.))
                                                            .child(
                                                                div().w_full().child(
                                                                    Input::new(&input)
                                                                        .appearance(true),
                                                                ),
                                                            )
                                                            .on_ok({
                                                                let this = this.clone();
                                                                move |_, window, cx| {
                                                                    this.update(
                                                                        cx,
                                                                        |view, cx| {
                                                                            let text = view
                                                                                .rename_input
                                                                                .read(cx)
                                                                                .value()
                                                                                .trim()
                                                                                .to_string();
                                                                            // NaN input: keep the
                                                                            // spec untouched.
                                                                            let val = if text
                                                                                .is_empty()
                                                                            {
                                                                                None
                                                                            } else if text
                                                                                .parse::<usize>()
                                                                                .is_ok()
                                                                            {
                                                                                Some(text)
                                                                            } else {
                                                                                return;
                                                                            };
                                                                            if let SpecSrc::Doc(
                                                                                doc,
                                                                            ) = &view.spec_src
                                                                            {
                                                                                doc.update(
                                                                                    cx,
                                                                                    |doc, cx| {
                                                                                        doc.set_base_view_key(
                                                                                            ix,
                                                                                            "limit",
                                                                                            val.as_deref(),
                                                                                            window,
                                                                                            cx,
                                                                                        );
                                                                                    },
                                                                                );
                                                                            }
                                                                            view.doc_epoch += 1;
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                    true
                                                                }
                                                            })
                                                    },
                                                );
                                                window.defer(cx, move |window, cx| {
                                                    input2.update(cx, |input, cx| {
                                                        input.focus(window, cx);
                                                    });
                                                });
                                            }
                                        }),
                                );
                                let menu = if is_cal {
                                    menu.item(
                                        PopupMenuItem::new("Date property…")
                                            .icon(assets::IconName::Calendar)
                                            .on_click({
                                                let this = this.clone();
                                                let candidates = groupable.clone();
                                                move |_, window, cx| {
                                                    let candidates = candidates.clone();
                                                    let this = this.clone();
                                                    window.open_dialog(
                                                        cx,
                                                        move |dialog, _window, _cx| {
                                                            let theme = _cx.theme();
                                                            let mut list =
                                                                v_flex().w_full().py_1();
                                                            for (nix, prop) in
                                                                candidates.iter().enumerate()
                                                            {
                                                                let prop = prop.clone();
                                                                let this = this.clone();
                                                                list = list.child(
                                                                    div()
                                                                        .id(("date-pick", nix))
                                                                        .w_full()
                                                                        .px_3()
                                                                        .py_1p5()
                                                                        .cursor_pointer()
                                                                        .hover(|s| {
                                                                            s.bg(theme.muted)
                                                                        })
                                                                        .child(
                                                                            div()
                                                                                .text_sm()
                                                                                .text_color(theme.foreground)
                                                                                .child(prop.clone()),
                                                                        )
                                                                        .on_click(move |_, window, cx| {
                                                                            this.update(cx, |view, cx| {
                                                                                if let SpecSrc::Doc(doc) =
                                                                                    &view.spec_src
                                                                                {
                                                                                    doc.update(cx, |doc, cx| {
                                                                                        doc.set_base_view_key(
                                                                                            ix,
                                                                                            "date",
                                                                                            Some(&prop),
                                                                                            window,
                                                                                            cx,
                                                                                        );
                                                                                    });
                                                                                }
                                                                                view.doc_epoch += 1;
                                                                                cx.notify();
                                                                            });
                                                                            window.close_dialog(cx);
                                                                        }),
                                                                );
                                                            }
                                                            dialog
                                                                .title("Date property")
                                                                .w(px(320.))
                                                                .overlay_closable(true)
                                                                .child(
                                                                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                        list.max_h(px(320.)),
                                                                    ),
                                                                )
                                                        },
                                                    );
                                                }
                                            }),
                                    )
                                } else {
                                    menu
                                };
                                let menu = if has_art {
                                    menu.item(
                                        PopupMenuItem::new("Card image…")
                                            .icon(assets::IconName::Image)
                                            .on_click({
                                                let this = this.clone();
                                                let candidates = groupable.clone();
                                                move |_, window, cx| {
                                                    let candidates = candidates.clone();
                                                    let this = this.clone();
                                                    window.open_dialog(
                                                        cx,
                                                        move |dialog, _window, _cx| {
                                                            let theme = _cx.theme();
                                                            let mut list =
                                                                v_flex().w_full().py_1();
                                                            // "Default" clears image: so the
                                                            // cover/banner/image fallback list
                                                            // applies again.
                                                            for (nix, (label, prop)) in
                                                                [("Default", None)]
                                                                    .into_iter()
                                                                    .chain(candidates.iter().map(
                                                                        |p| {
                                                                            (
                                                                                p.as_str(),
                                                                                Some(p.clone()),
                                                                            )
                                                                        },
                                                                    ))
                                                                    .enumerate()
                                                            {
                                                                let prop = prop.clone();
                                                                let label = label.to_string();
                                                                let this = this.clone();
                                                                list = list.child(
                                                                    div()
                                                                        .id(("img-pick", nix))
                                                                        .w_full()
                                                                        .px_3()
                                                                        .py_1p5()
                                                                        .cursor_pointer()
                                                                        .hover(|s| {
                                                                            s.bg(theme.muted)
                                                                        })
                                                                        .child(
                                                                            div()
                                                                                .text_sm()
                                                                                .text_color(theme.foreground)
                                                                                .child(label),
                                                                        )
                                                                        .on_click(move |_, window, cx| {
                                                                            this.update(cx, |view, cx| {
                                                                                if let SpecSrc::Doc(doc) =
                                                                                    &view.spec_src
                                                                                {
                                                                                    doc.update(cx, |doc, cx| {
                                                                                        doc.set_base_view_key(
                                                                                            ix,
                                                                                            "image",
                                                                                            prop.as_deref(),
                                                                                            window,
                                                                                            cx,
                                                                                        );
                                                                                    });
                                                                                }
                                                                                view.doc_epoch += 1;
                                                                                cx.notify();
                                                                            });
                                                                            window.close_dialog(cx);
                                                                        }),
                                                                );
                                                            }
                                                            dialog
                                                                .title("Card image property")
                                                                .w(px(320.))
                                                                .overlay_closable(true)
                                                                .child(
                                                                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                                        list.max_h(px(320.)),
                                                                    ),
                                                                )
                                                        },
                                                    );
                                                }
                                            }),
                                    )
                                    // the card-art knobs —
                                    // `imageFit:` cover/contain and a
                                    // numeric `imageAspectRatio:`.
                                    .item(
                                        PopupMenuItem::new("Image fit…")
                                            .icon(assets::IconName::Images)
                                            .on_click({
                                                let this = this.clone();
                                                move |_, window, cx| {
                                                    let this = this.clone();
                                                    window.open_dialog(
                                                        cx,
                                                        move |dialog, _window, _cx| {
                                                            let theme = _cx.theme();
                                                            let mut list =
                                                                v_flex().w_full().py_1();
                                                            for (nix, (label, val)) in [
                                                                ("Cover", Some("cover")),
                                                                ("Contain", Some("contain")),
                                                                ("Default", None),
                                                            ]
                                                            .into_iter()
                                                            .enumerate()
                                                            {
                                                                let this = this.clone();
                                                                list = list.child(
                                                                    div()
                                                                        .id(("fit-pick", nix))
                                                                        .w_full()
                                                                        .px_3()
                                                                        .py_1p5()
                                                                        .cursor_pointer()
                                                                        .hover(|s| {
                                                                            s.bg(theme.muted)
                                                                        })
                                                                        .child(
                                                                            div()
                                                                                .text_sm()
                                                                                .text_color(theme.foreground)
                                                                                .child(label),
                                                                        )
                                                                        .on_click(move |_, window, cx| {
                                                                            this.update(cx, |view, cx| {
                                                                                if let SpecSrc::Doc(doc) =
                                                                                    &view.spec_src
                                                                                {
                                                                                    doc.update(cx, |doc, cx| {
                                                                                        doc.set_base_view_key(
                                                                                            ix,
                                                                                            "imageFit",
                                                                                            val,
                                                                                            window,
                                                                                            cx,
                                                                                        );
                                                                                    });
                                                                                }
                                                                                view.doc_epoch += 1;
                                                                                cx.notify();
                                                                            });
                                                                            window.close_dialog(cx);
                                                                        }),
                                                                );
                                                            }
                                                            dialog
                                                                .title("Image fit")
                                                                .w(px(240.))
                                                                .overlay_closable(true)
                                                                .child(list)
                                                        },
                                                    );
                                                }
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new("Image aspect ratio…")
                                            .icon(assets::IconName::Image)
                                            .on_click({
                                                let this = this.clone();
                                                move |_, window, cx| {
                                                    let input =
                                                        this.read(cx).rename_input.clone();
                                                    input.update(cx, |input, cx| {
                                                        input.set_value("", window, cx);
                                                    });
                                                    let this = this.clone();
                                                    let input2 = input.clone();
                                                    window.open_dialog(
                                                        cx,
                                                        move |dialog, _window, _cx| {
                                                            let input = input.clone();
                                                            dialog
                                                                .title(
                                                                    "Aspect ratio (w/h) — blank clears",
                                                                )
                                                                .w(px(320.))
                                                                .child(
                                                                    div().w_full().child(
                                                                        Input::new(&input)
                                                                            .appearance(true),
                                                                    ),
                                                                )
                                                                .on_ok({
                                                                    let this = this.clone();
                                                                    move |_, window, cx| {
                                                                        this.update(
                                                                            cx,
                                                                            |view, cx| {
                                                                                let text = view
                                                                                    .rename_input
                                                                                    .read(cx)
                                                                                    .value()
                                                                                    .trim()
                                                                                    .to_string();
                                                                                let val = if text
                                                                                    .is_empty()
                                                                                {
                                                                                    None
                                                                                } else {
                                                                                    // "16:9" or a plain
                                                                                    // decimal both work.
                                                                                    let r = text
                                                                                        .split(':')
                                                                                        .filter_map(|p| {
                                                                                            p.trim()
                                                                                                .parse::<f32>()
                                                                                                .ok()
                                                                                        })
                                                                                        .reduce(|w, h| {
                                                                                            w / h
                                                                                        });
                                                                                    match r {
                                                                                        Some(r)
                                                                                            if r
                                                                                                .is_finite()
                                                                                                && r > 0.0 =>
                                                                                        {
                                                                                            Some(format!(
                                                                                                "{r:.2}"
                                                                                            ))
                                                                                        }
                                                                                        _ => return,
                                                                                    }
                                                                                };
                                                                                if let SpecSrc::Doc(
                                                                                    doc,
                                                                                ) = &view.spec_src
                                                                                {
                                                                                    doc.update(
                                                                                        cx,
                                                                                        |doc, cx| {
                                                                                            doc.set_base_view_key(
                                                                                                ix,
                                                                                                "imageAspectRatio",
                                                                                                val.as_deref(),
                                                                                                window,
                                                                                                cx,
                                                                                            );
                                                                                        },
                                                                                    );
                                                                                }
                                                                                view.doc_epoch += 1;
                                                                                cx.notify();
                                                                            },
                                                                        );
                                                                        true
                                                                    }
                                                                })
                                                        },
                                                    );
                                                    window.defer(cx, move |window, cx| {
                                                        input2.update(cx, |input, cx| {
                                                            input.focus(window, cx);
                                                        });
                                                    });
                                                }
                                            }),
                                    )
                                } else {
                                    menu
                                };
                                if !multi {
                                    return menu;
                                }
                                menu.item(
                                    PopupMenuItem::new("Delete view")
                                        .icon(assets::IconName::Delete)
                                        .on_click({
                                            let this = this.clone();
                                            move |_, window, cx| {
                                                this.update(cx, |view, cx| {
                                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                                        doc.update(cx, |doc, cx| {
                                                            doc.remove_base_view(
                                                                ix, window, cx,
                                                            );
                                                        });
                                                    }
                                                    // Keep the selection on a
                                                    // surviving view.
                                                    if ix < view.view_ix {
                                                        view.view_ix -= 1;
                                                    } else if ix == view.view_ix {
                                                        view.view_ix = ix.saturating_sub(1);
                                                    }
                                                    view.doc_epoch += 1;
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                )
                            }
                        }),
                );
            }
            // `+` tab — append a fresh view to the spec (the "New view" affordance), then select it.
            if matches!(self.spec_src, SpecSrc::Doc(_)) {
                let this = this.clone();
                let names = computed.view_names.clone();
                tabs = tabs.child(
                    div()
                        .id("base-add-view")
                        .px_2()
                        .py_0p5()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .hover(|s| s.text_color(theme.accent))
                        .child(Icon::new(assets::IconName::Plus).size_3p5())
                        .on_click(move |_, window, cx| {
                            let this = this.clone();
                            let taken = names.clone();
                            window.open_dialog(cx, move |dialog, _window, _cx| {
                                let theme = _cx.theme();
                                let mut list = v_flex().w_full().py_1();
                                for (ix, (label, kind)) in [
                                    ("Table", "table"),
                                    ("Cards", "cards"),
                                    ("Board", "board"),
                                    ("List", "list"),
                                    ("Calendar", "calendar"),
                                ]
                                .into_iter()
                                .enumerate()
                                {
                                    let this = this.clone();
                                    let taken = taken.clone();
                                    list = list.child(
                                        div()
                                            .id(("view-pick", ix))
                                            .w_full()
                                            .px_3()
                                            .py_1p5()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme.muted))
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        Icon::new(view_icon(Some(kind)))
                                                            .size_3p5()
                                                            .text_color(theme.muted_foreground),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(theme.foreground)
                                                            .child(label),
                                                    ),
                                            )
                                            .on_click(move |_, window, cx| {
                                                this.update(cx, |view, cx| {
                                                    let mut name = label.to_string();
                                                    let mut n = 2;
                                                    while taken.iter().any(|t| t == &name) {
                                                        name = format!("{label} {n}");
                                                        n += 1;
                                                    }
                                                    let new_ix = taken.len();
                                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                                        let key = doc
                                                            .read(cx)
                                                            .path
                                                            .to_string_lossy()
                                                            .to_string();
                                                        doc.update(cx, |doc, cx| {
                                                            doc.add_base_view(
                                                                &name, kind, window, cx,
                                                            );
                                                        });
                                                        view.view_ix = new_ix;
                                                        view.sort = None;
                                                        if let Some(ws) = view.workspace.upgrade() {
                                                            ws.update(cx, |ws, _cx| {
                                                                ws.remember_base_view(key, new_ix);
                                                            });
                                                        }
                                                    }
                                                    view.doc_epoch += 1;
                                                    cx.notify();
                                                });
                                                window.close_dialog(cx);
                                            }),
                                    );
                                }
                                dialog
                                    .title("New view")
                                    .w(px(240.))
                                    .overlay_closable(true)
                                    .child(list)
                            });
                        }),
                );
            }
        }

        let vault_root = self.vault.read(cx).root.clone();
        let toolbar = h_flex()
            .w_full()
            .px_3()
            .pt_2()
            .pb_1()
            .justify_between()
            // More views than width? The strip scrolls sideways
            // instead of clipping tabs under the search box.
            .child(
                gpui_kit::component::scroll::ScrollableElement::overflow_x_scrollbar(
                    tabs.flex_1().min_w_0(),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(px(150.))
                            .child(Input::new(&self.search).appearance(true)),
                    )
                    .child(
                        Button::new("base-new-note")
                            .ghost()
                            .xsmall()
                            .icon(assets::IconName::Plus)
                            .tooltip("New note matching this view")
                            .on_click({
                                let this = this.clone();
                                move |_, window, cx| {
                                    let Some(root) = vault_root.clone() else {
                                        return;
                                    };
                                    let (prefill, workspace) = this.update(cx, |view, cx| {
                                        (view.computed(cx).prefill.clone(), view.workspace.clone())
                                    });
                                    if let Some(workspace) = workspace.upgrade() {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.new_note_in(
                                                root.clone(),
                                                prefill.clone(),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }
                            }),
                    ),
            );

        let cards = matches!(computed.kind.as_str(), "cards" | "gallery");
        let kanban = matches!(computed.kind.as_str(), "kanban" | "board");
        let calendar = computed.kind == "calendar";
        let list = computed.kind == "list";
        let header = h_flex()
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .children(computed.headers.iter().enumerate().map(|(ix, h)| {
                let this = this.clone();
                let this_menu = this.clone();
                let this_drag = this.clone();
                let sorted = self.sort.filter(|(c, _)| *c == ix);
                let writable = matches!(self.spec_src, SpecSrc::Doc(_));
                sized_cell(div().id(("base-h", ix)), &eff_sizes, ix)
                    .relative()
                    .text_xs()
                    .font_semibold()
                    .text_color(theme.muted_foreground)
                    .truncate()
                    .cursor_pointer()
                    .child(match sorted {
                        Some((_, true)) => format!("{h} ▼"),
                        Some(_) => format!("{h} ▲"),
                        None => h.clone(),
                    })
                    // Drag-resize handle on the cell's right edge —
                    // live width overlays `eff_sizes` while dragging,
                    // `columnSize:` writes on release. Column 0 flexes,
                    // so only sized columns offer it.
                    .when(
                        writable && (ix != 0 || eff_sizes[ix].is_some()),
                        |d| {
                            let this = this.clone();
                            let start_w = eff_sizes.get(ix).copied().flatten().unwrap_or(140.);
                            d.child(
                                div()
                                    .id(("base-rs", ix))
                                    .absolute()
                                    .occlude()
                                    .right_0()
                                    .top_0()
                                    .w(px(6.))
                                    .h_full()
                                    .cursor_col_resize()
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        move |ev: &gpui::MouseDownEvent, _window, cx| {
                                            let x = f32::from(ev.position.x);
                                            this.update(cx, |view, _cx| {
                                                view.col_resize = Some((ix, x, start_w, start_w));
                                            });
                                            cx.stop_propagation();
                                        },
                                    ),
                            )
                        },
                    )
                    .on_click({
                        let h_name = h.clone();
                        move |_, _window, cx| {
                            // none → asc → desc → none
                            let h_name = h_name.clone();
                            this.update(cx, |view, cx| {
                                let next = match view.sort {
                                    Some((c, false)) if c == ix => Some(true),
                                    Some((c, true)) if c == ix => None,
                                    _ => Some(false),
                                };
                                view.apply_header_sort(ix, next, h_name.clone(), cx);
                            });
                        }
                    })
                    // Drag a header onto another → the column takes that
                    // slot; the new sequence is written to `order:` in
                    // the spec. `.base` files only — inline fences have
                    // no writable spec of their own.
                    .when(matches!(self.spec_src, SpecSrc::Doc(_)), |d| {
                        let this = this_drag.clone();
                        d.on_drag(ColDrag(ix), {
                            let label: SharedString = h.clone().into();
                            move |_, _, _, cx| {
                                cx.new(|_| KanbanDrag {
                                    label: label.clone(),
                                })
                            }
                        })
                        .drag_over::<ColDrag>(|style, _, _, cx| {
                            style.border_color(cx.theme().accent)
                        })
                        .on_drop::<ColDrag>(move |src: &ColDrag, window, cx| {
                            this.update(cx, |view, cx| {
                                let SpecSrc::Doc(doc) = &view.spec_src else {
                                    return;
                                };
                                let doc = doc.clone();
                                let mut cols = view.computed(cx).columns.clone();
                                let (from, to) = (src.0, ix);
                                if from >= cols.len() || from == to {
                                    return;
                                }
                                let col = cols.remove(from);
                                cols.insert(to, col);
                                if doc.update(cx, |doc, cx| {
                                    doc.reorder_base_columns(view.view_ix, &cols, window, cx)
                                }) {
                                    view.doc_epoch += 1;
                                }
                            });
                        })
                    })
                    // Right-click a header → hide that column (splices it
                    // out of `order:`). `file.name` is the row key and
                    // stays, and inline fences have no writable spec — an
                    // empty menu never renders, so the trigger is a no-op.
                    .context_menu({
                        let this = this_menu.clone();
                        let writable = matches!(self.spec_src, SpecSrc::Doc(_));
                        let col = computed.columns.get(ix).cloned().unwrap_or_default();
                        let h_name = h.clone();
                        let available = computed.available.clone();
                        let formula_srcs = computed.formula_srcs.clone();
                        move |menu, _window, _cx| {
                            let menu = menu
                                .item(
                                    PopupMenuItem::new("Sort ascending")
                                        .icon(assets::IconName::ArrowDownAZ)
                                        .on_click({
                                            let this = this.clone();
                                            let h_name = h_name.clone();
                                            move |_, _window, cx| {
                                                this.update(cx, |view, cx| {
                                                    view.apply_header_sort(
                                                        ix,
                                                        Some(false),
                                                        h_name.clone(),
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Sort descending")
                                        .icon(assets::IconName::ArrowDownWideNarrow)
                                        .on_click({
                                            let this = this.clone();
                                            let h_name = h_name.clone();
                                            move |_, _window, cx| {
                                                this.update(cx, |view, cx| {
                                                    view.apply_header_sort(
                                                        ix,
                                                        Some(true),
                                                        h_name.clone(),
                                                        cx,
                                                    );
                                                });
                                            }
                                        }),
                                );
                            // Rename {col}… — `properties: {col:
                            // {displayName:}}` at spec root; blank
                            // clears it back to the prop name. Applies
                            // to file.name too, so it sits above the
                            // file.name early return.
                            let menu = if writable {
                                let this = this.clone();
                                let col = col.clone();
                                let h_name = h_name.clone();
                                menu.item(
                                    PopupMenuItem::new(format!("Rename {col}…"))
                                        .icon(assets::IconName::SquarePen)
                                        .on_click(move |_, window, cx| {
                                            let input =
                                                this.read(cx).rename_input.clone();
                                            input.update(cx, |input, cx| {
                                                input.set_value(&h_name, window, cx);
                                            });
                                            let this = this.clone();
                                            let col = col.clone();
                                            let input2 = input.clone();
                                            window.open_dialog(
                                                cx,
                                                move |dialog, _window, _cx| {
                                                    let input = input.clone();
                                                    dialog
                                                        .title(format!("Rename {col}"))
                                                        .w(px(320.))
                                                        .child(
                                                            div().w_full().child(
                                                                Input::new(&input)
                                                                    .appearance(true),
                                                            ),
                                                        )
                                                        .on_ok({
                                                            let this = this.clone();
                                                            let col = col.clone();
                                                            move |_, window, cx| {
                                                                this.update(
                                                                    cx,
                                                                    |view, cx| {
                                                                        let text = view
                                                                            .rename_input
                                                                            .read(cx)
                                                                            .value()
                                                                            .trim()
                                                                            .to_string();
                                                                        let val =
                                                                            (!text.is_empty())
                                                                                .then_some(
                                                                                    text.as_str(),
                                                                                );
                                                                        if let SpecSrc::Doc(
                                                                            doc,
                                                                        ) = &view.spec_src
                                                                        {
                                                                            doc.update(
                                                                                cx,
                                                                                |doc, cx| {
                                                                                    doc.set_base_display_name(
                                                                                        &col,
                                                                                        val,
                                                                                        window,
                                                                                        cx,
                                                                                    );
                                                                                },
                                                                            );
                                                                        }
                                                                        view.doc_epoch += 1;
                                                                        cx.notify();
                                                                    },
                                                                );
                                                                true
                                                            }
                                                        })
                                                },
                                            );
                                            window.defer(cx, move |window, cx| {
                                                input2.update(cx, |input, cx| {
                                                    input.focus(window, cx);
                                                });
                                            });
                                        }),
                                )
                            } else {
                                menu
                            };
                            if !writable || col == "file.name" {
                                return menu;
                            }
                            // Insert left/right opens the same property
                            // chooser as "+", then splices the pick into
                            // `order:` next to this column.
                            let insert_item = |label: &'static str, after: bool| {
                                let this = this.clone();
                                let col = col.clone();
                                let candidates = available.clone();
                                PopupMenuItem::new(label)
                                    .icon(assets::IconName::Plus)
                                    .on_click(move |_, window, cx| {
                                        let this2 = this.clone();
                                        let col = col.clone();
                                        let candidates = candidates.clone();
                                        window.open_dialog(cx, move |dialog, _window, _cx| {
                                            let theme = _cx.theme();
                                            let mut list = v_flex().w_full().py_1();
                                            for (nix, prop) in candidates.iter().enumerate() {
                                                let prop = prop.clone();
                                                let this = this2.clone();
                                                let col = col.clone();
                                                list = list.child(
                                                    div()
                                                        .id(("ins-col", nix))
                                                        .w_full()
                                                        .px_3()
                                                        .py_1p5()
                                                        .cursor_pointer()
                                                        .hover(|s| s.bg(theme.muted))
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.foreground)
                                                                .child(prop.clone()),
                                                        )
                                                        .on_click(move |_, window, cx| {
                                                            this.update(cx, |view, cx| {
                                                                let SpecSrc::Doc(doc) =
                                                                    &view.spec_src
                                                                else {
                                                                    return;
                                                                };
                                                                let doc = doc.clone();
                                                                if doc.update(cx, |doc, cx| {
                                                                    doc.insert_base_column(
                                                                        view.view_ix,
                                                                        &prop,
                                                                        &col,
                                                                        after,
                                                                        window,
                                                                        cx,
                                                                    )
                                                                }) {
                                                                    view.doc_epoch += 1;
                                                                }
                                                            });
                                                            window.close_dialog(cx);
                                                        }),
                                                );
                                            }
                                            dialog
                                                .title(label)
                                                .w(px(320.))
                                                .overlay_closable(true)
                                                .child(
                                                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                        list.max_h(px(320.)),
                                                    ),
                                                )
                                        });
                                    })
                            };
                            let menu = menu
                                .item(insert_item("Insert column left", false))
                                .item(insert_item("Insert column right", true));
                            // Summarize… — `summaries: {col: fn}` under
                            // the view, picked from the built-in
                            // set; None deletes the entry.
                            let menu = menu.item({
                                let this = this.clone();
                                let col = col.clone();
                                PopupMenuItem::new(format!("Summarize {col}…"))
                                    .icon(assets::IconName::Sigma)
                                    .on_click(move |_, window, cx| {
                                        let this2 = this.clone();
                                        let col = col.clone();
                                        window.open_dialog(cx, move |dialog, _window, _cx| {
                                            let theme = _cx.theme();
                                            let mut list = v_flex().w_full().py_1();
                                            for (nix, (label, val)) in [
                                                ("None", None),
                                                ("Sum", Some("sum")),
                                                ("Average", Some("average")),
                                                ("Median", Some("median")),
                                                ("Min", Some("min")),
                                                ("Max", Some("max")),
                                                ("Range", Some("range")),
                                                ("Count", Some("count")),
                                                ("Unique", Some("unique")),
                                                ("Filled", Some("filled")),
                                                ("Empty", Some("empty")),
                                                ("Checked", Some("checked")),
                                                ("Unchecked", Some("unchecked")),
                                                ("Earliest", Some("earliest")),
                                                ("Latest", Some("latest")),
                                            ]
                                            .into_iter()
                                            .enumerate()
                                            {
                                                let this = this2.clone();
                                                let col = col.clone();
                                                list = list.child(
                                                    div()
                                                        .id(("sum-pick", nix))
                                                        .w_full()
                                                        .px_3()
                                                        .py_1p5()
                                                        .cursor_pointer()
                                                        .hover(|s| s.bg(theme.muted))
                                                        .child(
                                                            div()
                                                                .text_sm()
                                                                .text_color(theme.foreground)
                                                                .child(label),
                                                        )
                                                        .on_click(move |_, window, cx| {
                                                            this.update(cx, |view, cx| {
                                                                let SpecSrc::Doc(doc) =
                                                                    &view.spec_src
                                                                else {
                                                                    return;
                                                                };
                                                                let doc = doc.clone();
                                                                if doc.update(cx, |doc, cx| {
                                                                    doc.set_base_view_map_entry(
                                                                        view.view_ix,
                                                                        "summaries",
                                                                        &col,
                                                                        val,
                                                                        window,
                                                                        cx,
                                                                    )
                                                                }) {
                                                                    view.doc_epoch += 1;
                                                                }
                                                            });
                                                            window.close_dialog(cx);
                                                        }),
                                                );
                                            }
                                            dialog
                                                .title(format!("Summarize {col}"))
                                                .w(px(240.))
                                                .overlay_closable(true)
                                                .child(
                                                    gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                        list.max_h(px(320.)),
                                                    ),
                                                )
                                        });
                                    })
                            });
                            // Edit formula… — only on `formula.*`
                            // columns; rewrites `formulas: {name:
                            // 'expr'}`, blank deletes the formula.
                            let menu = if let Some(fname) =
                                col.strip_prefix("formula.")
                            {
                                let fname = fname.to_string();
                                menu.item({
                                    let this = this.clone();
                                    let formula_srcs = formula_srcs.clone();
                                    PopupMenuItem::new(format!("Edit formula.{fname}…"))
                                        .icon(assets::IconName::SquareFunction)
                                        .on_click(move |_, window, cx| {
                                            let input =
                                                this.read(cx).rename_input.clone();
                                            let src = formula_srcs
                                                .get(&fname)
                                                .cloned()
                                                .unwrap_or_default();
                                            input.update(cx, |input, cx| {
                                                input.set_value(&src, window, cx);
                                            });
                                            let this = this.clone();
                                            let fname = fname.clone();
                                            let input2 = input.clone();
                                            window.open_dialog(
                                                cx,
                                                move |dialog, _window, _cx| {
                                                    let input = input.clone();
                                                    dialog
                                                        .title(format!(
                                                            "formula.{fname}"
                                                        ))
                                                        .w(px(360.))
                                                        .child(
                                                            div().w_full().child(
                                                                Input::new(&input)
                                                                    .appearance(true),
                                                            ),
                                                        )
                                                        .on_ok({
                                                            let this = this.clone();
                                                            let fname = fname.clone();
                                                            move |_, window, cx| {
                                                                this.update(
                                                                    cx,
                                                                    |view, cx| {
                                                                        let text = view
                                                                            .rename_input
                                                                            .read(cx)
                                                                            .value()
                                                                            .trim()
                                                                            .to_string();
                                                                        let val =
                                                                            (!text.is_empty())
                                                                                .then_some(
                                                                                    text.as_str(),
                                                                                );
                                                                        if let SpecSrc::Doc(
                                                                            doc,
                                                                        ) = &view.spec_src
                                                                        {
                                                                            doc.update(
                                                                                cx,
                                                                                |doc, cx| {
                                                                                    doc.set_base_formula(
                                                                                        &fname,
                                                                                        val,
                                                                                        window,
                                                                                        cx,
                                                                                    );
                                                                                },
                                                                            );
                                                                        }
                                                                        view.doc_epoch += 1;
                                                                        cx.notify();
                                                                    },
                                                                );
                                                                true
                                                            }
                                                        })
                                                },
                                            );
                                            window.defer(cx, move |window, cx| {
                                                input2.update(cx, |input, cx| {
                                                    input.focus(window, cx);
                                                });
                                            });
                                        })
                                })
                            } else {
                                menu
                            };
                            // Column width… — `columnSize: {col: px}`,
                            // blank resets to the auto width.
                            let menu = menu.item({
                                let this = this.clone();
                                let col = col.clone();
                                PopupMenuItem::new(format!("{col} width…"))
                                    .icon(assets::IconName::MoveHorizontal)
                                    .on_click(move |_, window, cx| {
                                        let input =
                                            this.read(cx).rename_input.clone();
                                        input.update(cx, |input, cx| {
                                            input.set_value("", window, cx);
                                        });
                                        let this = this.clone();
                                        let col = col.clone();
                                        let input2 = input.clone();
                                        window.open_dialog(
                                            cx,
                                            move |dialog, _window, _cx| {
                                                let input = input.clone();
                                                dialog
                                                    .title(format!(
                                                        "{col} width (px) — blank resets"
                                                    ))
                                                    .w(px(320.))
                                                    .child(
                                                        div().w_full().child(
                                                            Input::new(&input)
                                                                .appearance(true),
                                                        ),
                                                    )
                                                    .on_ok({
                                                        let this = this.clone();
                                                        let col = col.clone();
                                                        move |_, window, cx| {
                                                            this.update(
                                                                cx,
                                                                |view, cx| {
                                                                    let text = view
                                                                        .rename_input
                                                                        .read(cx)
                                                                        .value()
                                                                        .trim()
                                                                        .to_string();
                                                                    let val = if text
                                                                        .is_empty()
                                                                    {
                                                                        None
                                                                    } else if text
                                                                        .parse::<f64>()
                                                                        .is_ok()
                                                                    {
                                                                        Some(text)
                                                                    } else {
                                                                        return;
                                                                    };
                                                                    if let SpecSrc::Doc(
                                                                        doc,
                                                                    ) = &view.spec_src
                                                                    {
                                                                        doc.update(
                                                                            cx,
                                                                            |doc, cx| {
                                                                                doc.set_base_view_map_entry(
                                                                                    view.view_ix,
                                                                                    "columnSize",
                                                                                    &col,
                                                                                    val.as_deref(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            },
                                                                        );
                                                                    }
                                                                    view.doc_epoch += 1;
                                                                    cx.notify();
                                                                },
                                                            );
                                                            true
                                                        }
                                                    })
                                            },
                                        );
                                        window.defer(cx, move |window, cx| {
                                            input2.update(cx, |input, cx| {
                                                input.focus(window, cx);
                                            });
                                        });
                                    })
                            });
                            let this = this.clone();
                            let col = col.clone();
                            menu.item(
                                PopupMenuItem::new(format!("Hide {col}"))
                                    .icon(assets::IconName::EyeOff)
                                    .on_click(move |_, window, cx| {
                                        this.update(cx, |view, cx| {
                                            let SpecSrc::Doc(doc) = &view.spec_src else {
                                                return;
                                            };
                                            let doc = doc.clone();
                                            let cols = view.computed(cx).columns.clone();
                                            if doc.update(cx, |doc, cx| {
                                                doc.remove_base_column(
                                                    view.view_ix,
                                                    &col,
                                                    &cols,
                                                    window,
                                                    cx,
                                                )
                                            }) {
                                                view.doc_epoch += 1;
                                            }
                                        });
                                    }),
                            )
                        }
                    })
            }))
            // Column chooser — the `+` cell lists vault properties not on
            // show; picking one splices it into the view's `order:` in
            // source. `.base` files only — inline fences have no writable
            // spec of their own.
            .when(
                matches!(self.spec_src, SpecSrc::Doc(_)),
                |header| {
                    let this = this.clone();
                    let available = computed.available.clone();
                    header.child(
                        div()
                            .id("base-add-col")
                            .w(px(28.))
                            .flex_none()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| s.text_color(theme.accent))
                            .child("+")
                            .on_click(move |_, window, cx| {
                                let view_ix = this.read(cx).view_ix;
                                let candidates = available.clone();
                                let this = this.clone();
                                window.open_dialog(cx, move |dialog, _window, _cx| {
                                    let theme = _cx.theme();
                                    let mut list = v_flex().w_full().py_1();
                                    // "New formula…" — define a formula
                                    // (name → expr dialogs), then add
                                    // `formula.{name}` to the view.
                                    let this_f = this.clone();
                                    list = list.child(
                                        div()
                                            .id("new-formula")
                                            .w_full()
                                            .px_3()
                                            .py_1p5()
                                            .cursor_pointer()
                                            .hover(|s| s.bg(theme.muted))
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(theme.accent)
                                                    .child("New formula…"),
                                            )
                                            .on_click(move |_, window, cx| {
                                                window.close_dialog(cx);
                                                let this = this_f.clone();
                                                window.defer(cx, move |window, cx| {
                                                let input =
                                                    this.read(cx).rename_input.clone();
                                                input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx);
                                                });
                                                let input2 = input.clone();
                                                let input_focus = input.clone();
                                                window.open_dialog(
                                                    cx,
                                                    move |dialog, _window, _cx| {
                                                        let input = input.clone();
                                                        dialog
                                                            .title("New formula — name")
                                                            .w(px(320.))
                                                            .child(
                                                                div().w_full().child(
                                                                    Input::new(&input)
                                                                        .appearance(true),
                                                                ),
                                                            )
                                                            .on_ok({
                                                                let this = this.clone();
                                                                let input2 = input2.clone();
                                                                move |_, window, cx| {
                                                                    let name = input2
                                                                        .read(cx)
                                                                        .value()
                                                                        .trim()
                                                                        .to_string();
                                                                    let valid = !name
                                                                        .is_empty()
                                                                        && name.chars().all(
                                                                            |c| c
                                                                                .is_alphanumeric()
                                                                                || c == '_'
                                                                                || c == '-',
                                                                        );
                                                                    if !valid {
                                                                        return true;
                                                                    }
                                                                    // Second dialog: the
                                                                    // expression itself.
                                                                    let this = this.clone();
                                                                    let input3 =
                                                                        input2.clone();
                                                                    let name2 = name.clone();
                                                                    window.defer(
                                                                        cx,
                                                                        move |window, cx| {
                                                                            input3.update(
                                                                                cx,
                                                                                |input, cx| {
                                                                                    input.set_value(
                                                                                        "",
                                                                                        window,
                                                                                        cx,
                                                                                    );
                                                                                },
                                                                            );
                                                                            let this =
                                                                                this.clone();
                                                                            let input4 =
                                                                                input3.clone();
                                                                            let input5 =
                                                                                input3.clone();
                                                                            let name3 =
                                                                                name2.clone();
                                                                            window.open_dialog(
                                                                                cx,
                                                                                move |dialog, _window, _cx| {
                                                                                    let input = input4.clone();
                                                                                    dialog
                                                                                        .title(
                                                                                            format!(
                                                                                                "formula.{name3} — expression"
                                                                                            ),
                                                                                        )
                                                                                        .w(px(360.))
                                                                                        .child(
                                                                                            div().w_full()
                                                                                                .child(
                                                                                                    Input::new(
                                                                                                        &input,
                                                                                                    )
                                                                                                    .appearance(
                                                                                                        true,
                                                                                                    ),
                                                                                                ),
                                                                                        )
                                                                                        .on_ok(
                                                                                            {
                                                                                                let this = this
                                                                                                    .clone();
                                                                                                let input5 = input5.clone();
                                                                                                let name2 = name2.clone();
                                                                                                move |_, window, cx| {
                                                                                                    let expr = input5
                                                                                                        .read(cx)
                                                                                                        .value()
                                                                                                        .trim()
                                                                                                        .to_string();
                                                                                                    this.update(
                                                                                                        cx,
                                                                                                        |view, cx| {
                                                                                                            if let SpecSrc::Doc(
                                                                                                                doc,
                                                                                                            ) = &view
                                                                                                                .spec_src
                                                                                                            {
                                                                                                                doc.update(
                                                                                                                    cx,
                                                                                                                    |doc, cx| {
                                                                                                                        if !expr
                                                                                                                            .is_empty()
                                                                                                                        {
                                                                                                                            doc.set_base_formula(
                                                                                                                                &name2,
                                                                                                                                Some(
                                                                                                                                    &expr,
                                                                                                                                ),
                                                                                                                                window,
                                                                                                                                cx,
                                                                                                                            );
                                                                                                                        }
                                                                                                                        doc.add_base_column(
                                                                                                                            view_ix,
                                                                                                                            &format!(
                                                                                                                                "formula.{name2}"
                                                                                                                            ),
                                                                                                                            window,
                                                                                                                            cx,
                                                                                                                        );
                                                                                                                    },
                                                                                                                );
                                                                                                            }
                                                                                                            view.doc_epoch += 1;
                                                                                                            cx.notify();
                                                                                                        },
                                                                                                    );
                                                                                                    true
                                                                                                }
                                                                                            },
                                                                                        )
                                                                                },
                                                                            );
                                                                            input3.update(
                                                                                cx,
                                                                                |input, cx| {
                                                                                    input.focus(
                                                                                        window, cx,
                                                                                    );
                                                                                },
                                                                            );
                                                                        },
                                                                    );
                                                                    true
                                                                }
                                                            })
                                                    },
                                                );
                                                window.defer(cx, move |window, cx| {
                                                    input_focus.update(cx, |input, cx| {
                                                        input.focus(window, cx);
                                                    });
                                                });
                                                });
                                            }),
                                    );
                                    for (ix, prop) in candidates.iter().enumerate() {
                                        let prop = prop.clone();
                                        let this = this.clone();
                                        list = list.child(
                                            div()
                                                .id(("col-pick", ix))
                                                .w_full()
                                                .px_3()
                                                .py_1p5()
                                                .cursor_pointer()
                                                .hover(|s| s.bg(theme.muted))
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(theme.foreground)
                                                        .child(prop.clone()),
                                                )
                                                .on_click(move |_, window, cx| {
                                                    this.update(cx, |view, cx| {
                                                        if let SpecSrc::Doc(doc) = &view.spec_src {
                                                            doc.update(cx, |doc, cx| {
                                                                doc.add_base_column(
                                                                    view_ix, &prop, window, cx,
                                                                );
                                                            });
                                                        }
                                                        view.doc_epoch += 1;
                                                        cx.notify();
                                                    });
                                                    window.close_dialog(cx);
                                                }),
                                        );
                                    }
                                    dialog
                                        .title("Add column")
                                        .w(px(320.))
                                        .overlay_closable(true)
                                        .child(
                                            gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                                                list.max_h(px(320.)),
                                            ),
                                        )
                                });
                            }),
                    )
                },
            );

        // A column is editable when it reads one frontmatter property —
        // `status` / `note.status`, not `file.*`/`formula.*`/expressions.
        let editable_prop = |cix: usize| {
            computed
                .columns
                .get(cix)
                .and_then(|col| match parse_expr(col).ok() {
                    Some(Expr::Ref(ns, name)) if ns.is_none() || ns.as_deref() == Some("note") => {
                        Some(name)
                    }
                    _ => None,
                })
        };
        let mut rows = v_flex().w_full();
        if calendar {
            rows = rows.child(self.render_calendar(&this, &computed, &visible, cx));
        } else if kanban {
            // Group rows on the resolved column's display value —
            // `groupBy: {direction}` orders the columns by value.
            let gix = computed.group_ix.unwrap_or(usize::MAX);
            let mut ordered: Vec<&Row> = visible.to_vec();
            if computed.grouped {
                ordered.sort_by(|a, b| {
                    let ord = lit_cmp(&a.cells[gix].lit, &b.cells[gix].lit);
                    if computed.group_desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
            }
            let mut groups: Vec<(String, Vec<&Row>)> = Vec::new();
            for row in ordered {
                let value = row
                    .cells
                    .get(gix)
                    .map(|v| v.text.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "No value".into());
                match groups.iter_mut().find(|(name, _)| *name == value) {
                    Some((_, items)) => items.push(row),
                    None => groups.push((value, vec![row])),
                }
            }
            // Columns stretch to the board's full height (default
            // `items` alignment, no items_start) so a card dropped
            // anywhere under a column still lands on it.
            let mut board = h_flex().gap_3().p_3().h_full();
            for (gcol, (name, items)) in groups.into_iter().enumerate() {
                let group_lit = items
                    .first()
                    .and_then(|row| row.cells.get(gix))
                    .map(|cell| cell.lit.clone())
                    .unwrap_or(Lit::Null);
                let mut col = v_flex()
                    .id(("kanban-col", gcol))
                    .w(px(240.))
                    .h_full()
                    .flex_none()
                    .gap_1()
                    .p_2()
                    .border_1()
                    .border_color(theme.border.opacity(0.0))
                    .bg(theme.secondary)
                    .rounded(theme.radius);
                // Drag a card onto a column → set the grouped property on
                // the note's frontmatter. "No value" clears it (Null).
                if let Some(prop) = computed.group_prop.clone() {
                    let workspace = self.workspace.clone();
                    let value = lit_to_value(&group_lit);
                    col = col
                        .drag_over::<PathBuf>(|style, _, _, cx| {
                            style.border_color(cx.theme().accent)
                        })
                        .on_drop::<PathBuf>(move |path: &PathBuf, window, cx| {
                            let _ = workspace.update(cx, |workspace, cx| {
                                workspace.set_note_property(
                                    path.clone(),
                                    &prop,
                                    value.clone(),
                                    window,
                                    cx,
                                );
                            });
                        });
                }
                col = col.child(
                    h_flex()
                        .justify_between()
                        .child(
                            div()
                                .text_xs()
                                .font_semibold()
                                .text_color(theme.foreground)
                                .child(name),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{}", items.len())),
                        ),
                );
                for (cix, row) in items.iter().enumerate() {
                    let path = row.path.clone();
                    let workspace = self.workspace.clone();
                    let mut card = v_flex()
                        .id(("kanban-card", gcol * 1000 + cix))
                        .gap_0p5()
                        .p_2()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .on_drag(path.clone(), {
                            let label: SharedString = row
                                .cells
                                .first()
                                .map(|cell| cell.text.clone().into())
                                .unwrap_or_default();
                            move |_, _, _, cx| {
                                cx.new(|_| KanbanDrag {
                                    label: label.clone(),
                                })
                            }
                        })
                        .on_click(move |ev, window, cx| {
                            open_path_click(&workspace, path.clone(), ev, window, cx)
                        })
                        .on_mouse_move({
                            let workspace = self.workspace.clone();
                            let path = row.path.clone();
                            move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.peek_at(
                                        crate::app::PeekKind::Note(path.clone()),
                                        ev.position,
                                        cx,
                                    )
                                });
                            }
                        })
                        .on_hover({
                            let workspace = self.workspace.clone();
                            let path = row.path.clone();
                            move |hovered: &bool, _window, cx| {
                                if !*hovered {
                                    let _ = workspace.update(cx, |ws, cx| {
                                        ws.hide_peek(&crate::app::PeekKind::Note(path.clone()), cx)
                                    });
                                }
                            }
                        })
                        .context_menu({
                            let ws = self.workspace.clone();
                            let path = row.path.clone();
                            move |menu, _window, _cx| {
                                row_context_menu(menu, path.clone(), ws.clone())
                            }
                        });
                    if let Some(cover) = &row.cover {
                        let source: gpui_kit::ImageSource = cover
                            .strip_prefix("file://")
                            .map(|p| std::path::PathBuf::from(p).into())
                            .unwrap_or_else(|| cover.clone().into());
                        // `imageFit: contain` letterboxes instead of
                        // cropping; `imageAspectRatio: r` replaces the
                        // fixed strip height.
                        let fit = if computed.image_fit.as_deref() == Some("contain") {
                            ObjectFit::Contain
                        } else {
                            ObjectFit::Cover
                        };
                        let mut art = img(source).w_full().rounded(theme.radius);
                        art = match computed.image_aspect {
                            Some(r) if r > 0.0 => art.h(px(220. / r as f32)),
                            _ => art.h(px(64.)),
                        };
                        card = card.child(art.object_fit(fit));
                    }
                    card = card.child(
                        div()
                            .text_sm()
                            .text_color(theme.foreground)
                            .truncate()
                            .child(
                                row.cells
                                    .first()
                                    .map(|c| c.text.clone())
                                    .unwrap_or_default(),
                            ),
                    );
                    col = col.child(card);
                }
                // "+ New" at the column foot — creates a note holding
                // the column's value (plus the view's filter prefill).
                if let Some(prop) = computed.group_prop.clone() {
                    let workspace = self.workspace.clone();
                    let vault_root = self.vault.read(cx).root.clone();
                    let mut fm = computed.prefill.clone();
                    if let Some(yaml) = lit_to_yaml(&group_lit) {
                        fm.push((prop, yaml));
                    }
                    col = col.child(
                        Button::new(("kanban-new", gcol))
                            .ghost()
                            .xsmall()
                            .w_full()
                            .icon(assets::IconName::Plus)
                            .label("New")
                            .on_click(move |_, window, cx| {
                                let Some(root) = vault_root.clone() else {
                                    return;
                                };
                                let _ = workspace.update(cx, |workspace, cx| {
                                    workspace.new_note_in(root.clone(), fm.clone(), window, cx);
                                });
                            }),
                    );
                }
                board = board.child(col);
            }
            rows = rows.child(board);
        } else if cards {
            // `group_by` sections: a full-width band between groups,
            // then that group's cards. Groups order by lit value.
            let gix = computed.group_ix.unwrap_or(usize::MAX);
            let mut order: Vec<usize> = (0..visible.len()).collect();
            let mut group_counts: std::collections::HashMap<&str, usize> = Default::default();
            if computed.grouped {
                order.sort_by(|a, b| {
                    let ord = lit_cmp(&visible[*a].cells[gix].lit, &visible[*b].cells[gix].lit);
                    if computed.group_desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
                for row in visible.iter().copied() {
                    let key = row
                        .cells
                        .get(gix)
                        .map(|c| c.text.trim())
                        .unwrap_or_default();
                    *group_counts.entry(key).or_default() += 1;
                }
            }
            let mut last_group: Option<&str> = None;
            let mut grid = div().flex().flex_wrap().gap_3().p_3();
            for ix in order {
                let row = visible[ix];
                if computed.grouped {
                    let key = row
                        .cells
                        .get(gix)
                        .map(|c| c.text.trim())
                        .unwrap_or_default();
                    if last_group != Some(key) {
                        last_group = Some(key);
                        let label = if key.is_empty() {
                            format!(
                                "No {}",
                                computed.headers.get(gix).cloned().unwrap_or_default()
                            )
                        } else {
                            key.to_string()
                        };
                        grid = grid.child(
                            div()
                                .w_full()
                                .flex_none()
                                .border_b_1()
                                .border_color(theme.border)
                                .pb_1()
                                .text_xs()
                                .font_semibold()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    label,
                                    group_counts.get(key).copied().unwrap_or(0)
                                )),
                        );
                    }
                }
                let path = row.path.clone();
                let workspace = self.workspace.clone();
                let mut card = v_flex()
                    .id(("base-card", ix))
                    .w(px(210.))
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .overflow_hidden()
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.accent))
                    .on_click(move |ev, window, cx| {
                        open_path_click(&workspace, path.clone(), ev, window, cx)
                    })
                    .on_mouse_move({
                        let workspace = self.workspace.clone();
                        let path = row.path.clone();
                        move |ev: &gpui::MouseMoveEvent, _window, cx| {
                            let _ = workspace.update(cx, |ws, cx| {
                                ws.peek_at(
                                    crate::app::PeekKind::Note(path.clone()),
                                    ev.position,
                                    cx,
                                )
                            });
                        }
                    })
                    .on_hover({
                        let workspace = self.workspace.clone();
                        let path = row.path.clone();
                        move |hovered: &bool, _window, cx| {
                            if !*hovered {
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.hide_peek(&crate::app::PeekKind::Note(path.clone()), cx)
                                });
                            }
                        }
                    })
                    .context_menu({
                        let ws = self.workspace.clone();
                        let path = row.path.clone();
                        move |menu, _window, _cx| row_context_menu(menu, path.clone(), ws.clone())
                    });
                if let Some(cover) = &row.cover {
                    let source: gpui_kit::ImageSource = cover
                        .strip_prefix("file://")
                        .map(|p| std::path::PathBuf::from(p).into())
                        .unwrap_or_else(|| cover.clone().into());
                    // `imageFit:`/`imageAspectRatio:` — same the reference editor
                    // keys as kanban card art.
                    let fit = if computed.image_fit.as_deref() == Some("contain") {
                        ObjectFit::Contain
                    } else {
                        ObjectFit::Cover
                    };
                    let mut art = img(source).w_full();
                    art = match computed.image_aspect {
                        Some(r) if r > 0.0 => art.h(px(210. / r as f32)),
                        _ => art.h(px(110.)),
                    };
                    card = card.child(art.object_fit(fit));
                }
                let mut body = v_flex().p_2().gap_0p5();
                for (cix, cell) in row.cells.iter().enumerate() {
                    if cix == 0 {
                        body = body.child(
                            div()
                                .text_sm()
                                .font_semibold()
                                .text_color(theme.foreground)
                                .truncate()
                                .child(cell.text.clone()),
                        );
                    } else if cix <= 3 && !cell.text.trim().is_empty() {
                        body = body.child(prop_cell(
                            &computed.headers[cix],
                            cell,
                            theme.muted_foreground,
                            theme.accent,
                        ));
                    }
                }
                grid = grid.child(card.child(body));
            }
            rows = rows.child(grid);
        } else if list {
            // the list view: one compact row per note — cover
            // thumb, name, then the first few non-empty properties
            // inline. No column header; sort comes from the spec.
            // `group_by` bands the rows like the table view does.
            let gix = computed.group_ix.unwrap_or(usize::MAX);
            let mut order: Vec<usize> = (0..visible.len()).collect();
            let mut group_counts: std::collections::HashMap<&str, usize> = Default::default();
            if computed.grouped {
                order.sort_by(|a, b| {
                    let ord = lit_cmp(&visible[*a].cells[gix].lit, &visible[*b].cells[gix].lit);
                    if computed.group_desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
                for row in visible.iter().copied() {
                    let key = row
                        .cells
                        .get(gix)
                        .map(|c| c.text.trim())
                        .unwrap_or_default();
                    *group_counts.entry(key).or_default() += 1;
                }
            }
            let mut last_group: Option<&str> = None;
            let mut items = v_flex().w_full().p_2().gap_0p5();
            for ix in order {
                let row = visible[ix];
                if computed.grouped {
                    let key = row
                        .cells
                        .get(gix)
                        .map(|c| c.text.trim())
                        .unwrap_or_default();
                    if last_group != Some(key) {
                        last_group = Some(key);
                        let label = if key.is_empty() {
                            format!(
                                "No {}",
                                computed.headers.get(gix).cloned().unwrap_or_default()
                            )
                        } else {
                            key.to_string()
                        };
                        items = items.child(
                            div()
                                .id(("base-list-group", ix))
                                .px_2()
                                .py_1()
                                .mt_1()
                                .border_b_1()
                                .border_color(theme.border)
                                .text_xs()
                                .font_semibold()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "{} · {}",
                                    label,
                                    group_counts.get(key).copied().unwrap_or(0)
                                )),
                        );
                    }
                }
                let path = row.path.clone();
                let workspace = self.workspace.clone();
                let mut line = h_flex()
                    .id(("base-list", ix))
                    .w_full()
                    .gap_2()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted.opacity(0.5)))
                    .on_click(move |ev, window, cx| {
                        open_path_click(&workspace, path.clone(), ev, window, cx)
                    })
                    .on_mouse_move({
                        let workspace = self.workspace.clone();
                        let path = row.path.clone();
                        move |ev: &gpui::MouseMoveEvent, _window, cx| {
                            let _ = workspace.update(cx, |ws, cx| {
                                ws.peek_at(
                                    crate::app::PeekKind::Note(path.clone()),
                                    ev.position,
                                    cx,
                                )
                            });
                        }
                    })
                    .on_hover({
                        let workspace = self.workspace.clone();
                        let path = row.path.clone();
                        move |hovered: &bool, _window, cx| {
                            if !*hovered {
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.hide_peek(&crate::app::PeekKind::Note(path.clone()), cx)
                                });
                            }
                        }
                    })
                    .context_menu({
                        let ws = self.workspace.clone();
                        let path = row.path.clone();
                        move |menu, _window, _cx| row_context_menu(menu, path.clone(), ws.clone())
                    });
                if let Some(cover) = &row.cover {
                    let source: gpui_kit::ImageSource = cover
                        .strip_prefix("file://")
                        .map(|p| std::path::PathBuf::from(p).into())
                        .unwrap_or_else(|| cover.clone().into());
                    line = line.child(
                        img(source)
                            .w(px(22.))
                            .h(px(22.))
                            .flex_none()
                            .rounded(theme.radius)
                            .object_fit(ObjectFit::Cover),
                    );
                }
                line = line.child(
                    div().w(px(160.)).flex_none().text_sm().truncate().child(
                        row.cells
                            .first()
                            .map(|c| c.text.clone())
                            .unwrap_or_default(),
                    ),
                );
                let mut shown = 0;
                for (cix, cell) in row.cells.iter().enumerate().skip(1) {
                    if shown >= 3 || cell.text.trim().is_empty() {
                        continue;
                    }
                    shown += 1;
                    line = line.child(
                        prop_cell(
                            &computed.headers[cix],
                            cell,
                            theme.muted_foreground,
                            theme.accent,
                        )
                        .flex_1(),
                    );
                }
                items = items.child(line);
            }
            rows = rows.child(items);
        } else {
            // Interactive header sort orders rows at render time —
            // `Computed` stays cached; comparisons use the raw `Lit`.
            let mut order: Vec<usize> = (0..visible.len()).collect();
            let gix = computed.group_ix.unwrap_or(usize::MAX);
            // Grouped views sort by the group column first so same-value
            // rows land in consecutive runs; the header sort then applies
            // within each group.
            if computed.grouped {
                order.sort_by(|a, b| {
                    let ord = lit_cmp(&visible[*a].cells[gix].lit, &visible[*b].cells[gix].lit);
                    let ord = if computed.group_desc {
                        ord.reverse()
                    } else {
                        ord
                    };
                    ord.then_with(|| {
                        self.sort
                            .map(|(cix, desc)| {
                                let ord = lit_cmp(
                                    &visible[*a].cells[cix].lit,
                                    &visible[*b].cells[cix].lit,
                                );
                                if desc {
                                    ord.reverse()
                                } else {
                                    ord
                                }
                            })
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
            } else if let Some((cix, desc)) = self.sort {
                order.sort_by(|a, b| {
                    let ord = lit_cmp(&visible[*a].cells[cix].lit, &visible[*b].cells[cix].lit);
                    if desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
            }
            // Per-group row counts for "value · n" headers.
            let mut group_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            if computed.grouped {
                for row in visible.iter().copied() {
                    *group_counts
                        .entry(row.cells[gix].text.as_str())
                        .or_default() += 1;
                }
            }
            let mut last_group: Option<&str> = None;
            let mut group_collapsed = false;
            for (ix, &rix) in order.iter().enumerate() {
                let row = visible[rix];
                let path = row.path.clone();
                let workspace = self.workspace.clone();
                if computed.grouped && last_group != Some(row.cells[gix].text.as_str()) {
                    last_group = Some(row.cells[gix].text.as_str());
                    let gkey = (self.view_ix, row.cells[gix].text.clone());
                    group_collapsed = self.collapsed_groups.contains(&gkey);
                    let label = if row.cells[gix].text.trim().is_empty() {
                        format!(
                            "No {}",
                            computed
                                .headers
                                .get(gix)
                                .cloned()
                                .unwrap_or_else(|| "value".into())
                        )
                    } else {
                        row.cells[gix].text.clone()
                    };
                    let count = group_counts
                        .get(row.cells[gix].text.as_str())
                        .copied()
                        .unwrap_or(0);
                    let chevron = if group_collapsed {
                        assets::IconName::ChevronRight
                    } else {
                        assets::IconName::ChevronDown
                    };
                    rows = rows.child(
                        div()
                            .id(("base-group", ix))
                            .w_full()
                            .px_3()
                            .py_1()
                            .border_b_1()
                            .border_color(theme.border.opacity(0.5))
                            .bg(theme.muted.opacity(0.3))
                            .cursor_pointer()
                            .on_click({
                                let this = this.clone();
                                let gkey = gkey.clone();
                                move |_, _window, cx| {
                                    this.update(cx, |view, cx| {
                                        if !view.collapsed_groups.insert(gkey.clone()) {
                                            view.collapsed_groups.remove(&gkey);
                                        }
                                        cx.notify();
                                    });
                                }
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        Icon::new(chevron)
                                            .size_3()
                                            .text_color(theme.muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(theme.muted_foreground)
                                            .truncate()
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{count}")),
                                    ),
                            ),
                    );
                }
                if group_collapsed {
                    continue;
                }
                rows = rows.child(
                    div()
                        .id(("base-row", ix))
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .border_b_1()
                        .border_color(theme.border.opacity(0.5))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted.opacity(0.5)))
                        .child(h_flex().w_full().children(row.cells.iter().enumerate().map(
                            |(cix, cell)| {
                                if let Some(target) = &cell.link {
                                    let target = target.clone();
                                    let workspace = self.workspace.clone();
                                    sized_cell(
                                        div().id(("base-cell-link", ix * 4096 + cix)),
                                        &eff_sizes,
                                        cix,
                                    )
                                    .text_sm()
                                    .truncate()
                                    .text_color(theme.accent)
                                    .cursor_pointer()
                                    .on_click(move |ev, window, cx| {
                                        cx.stop_propagation();
                                        open_path_click(&workspace, target.clone(), ev, window, cx)
                                    })
                                    .child(cell.text.clone())
                                    .into_any_element()
                                } else if let Some(prop) = editable_prop(cix) {
                                    let path = row.path.clone();
                                    let current = cell.text.clone();
                                    let workspace = self.workspace.clone();
                                    sized_cell(
                                        div().id(("base-cell", ix * 4096 + cix)),
                                        &eff_sizes,
                                        cix,
                                    )
                                    .text_sm()
                                    .truncate()
                                    .text_color(theme.foreground)
                                    .cursor_pointer()
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        let _ = workspace.update(cx, |ws, cx| {
                                            ws.edit_note_property(
                                                path.clone(),
                                                prop.clone(),
                                                current.clone(),
                                                window,
                                                cx,
                                            );
                                        });
                                    })
                                    .when(matches!(cell.lit, Lit::Num(_)), |d| d.text_right())
                                    .child(match &cell.lit {
                                        Lit::Bool(b) => {
                                            bool_icon(*b, theme.muted_foreground, theme.accent)
                                        }
                                        _ => cell.text.clone().into_any_element(),
                                    })
                                    .into_any_element()
                                } else {
                                    sized_cell(div(), &eff_sizes, cix)
                                        .text_sm()
                                        .truncate()
                                        .text_color(theme.foreground)
                                        .when(matches!(cell.lit, Lit::Num(_)), |d| d.text_right())
                                        .child(match &cell.lit {
                                            Lit::Bool(b) => {
                                                bool_icon(*b, theme.muted_foreground, theme.accent)
                                            }
                                            _ => cell.text.clone().into_any_element(),
                                        })
                                        .into_any_element()
                                }
                            },
                        )))
                        .on_click(move |ev, window, cx| {
                            open_path_click(&workspace, path.clone(), ev, window, cx)
                        })
                        .on_mouse_move({
                            let workspace = self.workspace.clone();
                            let path = row.path.clone();
                            move |ev: &gpui::MouseMoveEvent, _window, cx| {
                                let _ = workspace.update(cx, |ws, cx| {
                                    ws.peek_at(
                                        crate::app::PeekKind::Note(path.clone()),
                                        ev.position,
                                        cx,
                                    )
                                });
                            }
                        })
                        .on_hover({
                            let workspace = self.workspace.clone();
                            let path = row.path.clone();
                            move |hovered: &bool, _window, cx| {
                                if !*hovered {
                                    let _ = workspace.update(cx, |ws, cx| {
                                        ws.hide_peek(&crate::app::PeekKind::Note(path.clone()), cx)
                                    });
                                }
                            }
                        })
                        .context_menu({
                            let ws = self.workspace.clone();
                            let path = row.path.clone();
                            move |menu, _window, _cx| {
                                row_context_menu(menu, path.clone(), ws.clone())
                            }
                        }),
                );
            }
        }
        // `summaries:` footer row — aggregate cells under their
        // column, same grid as the header.
        if !computed.summaries.is_empty() && !cards && !kanban && !calendar && !list {
            rows = rows.child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .border_t_1()
                    .border_color(theme.border)
                    .children(computed.headers.iter().enumerate().map(|(ix, _)| {
                        let cell = computed.summaries.iter().find(|(c, _, _)| *c == ix);
                        sized_cell(div(), &eff_sizes, ix)
                            .text_xs()
                            .truncate()
                            // Summary values are aggregates — always
                            // numeric and right-aligned.
                            .text_right()
                            .text_color(theme.muted_foreground)
                            .child(
                                cell.map(|(_, name, val)| format!("{name}: {val}"))
                                    .unwrap_or_default(),
                            )
                    })),
            );
        }

        if visible.is_empty() && computed.error.is_none() {
            rows = rows.child(
                div()
                    .px_3()
                    .py_6()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No notes match."),
            );
        }

        v_flex()
            .size_full()
            // Column drag-resize: track the pointer while a header
            // handle is grabbed, write `columnSize:` on release.
            .on_mouse_move({
                let this = this.clone();
                move |ev: &gpui::MouseMoveEvent, _window, cx| {
                    this.update(cx, |view, cx| {
                        let Some((rix, gx, sw, _)) = view.col_resize else {
                            return;
                        };
                        let x = f32::from(ev.position.x);
                        let live = (sw + x - gx).max(48.);
                        if view.col_resize != Some((rix, gx, sw, live)) {
                            view.col_resize = Some((rix, gx, sw, live));
                            cx.notify();
                        }
                    });
                }
            })
            .on_mouse_up(gpui::MouseButton::Left, {
                let this = this.clone();
                move |_, window, cx| {
                    this.update(cx, |view, cx| {
                        let Some((rix, _, _, live)) = view.col_resize.take() else {
                            return;
                        };
                        let col = view
                            .computed(cx)
                            .columns
                            .get(rix)
                            .cloned()
                            .unwrap_or_default();
                        if let SpecSrc::Doc(doc) = &view.spec_src {
                            doc.update(cx, |doc, cx| {
                                doc.set_base_view_map_entry(
                                    view.view_ix,
                                    "columnSize",
                                    &col,
                                    Some(&format!("{}", live.round() as i64)),
                                    window,
                                    cx,
                                );
                            });
                        }
                        view.doc_epoch += 1;
                        cx.notify();
                    });
                }
            })
            .child(toolbar)
            // The active view's `filters:` terms as removable chips — the ×
            // splices the term out via `remove_base_view_filter`, so active
            // filters are visible instead of buried in the tab menu.
            // Embedded bases render the chips read-only.
            .children({
                let terms = computed
                    .view_filters
                    .get(self.view_ix)
                    .cloned()
                    .unwrap_or_default();
                let writable = matches!(self.spec_src, SpecSrc::Doc(_));
                let addable = writable && !computed.groupable.is_empty();
                if terms.is_empty() && !addable {
                    None
                } else {
                    let mut bar = h_flex()
                        .w_full()
                        .px_3()
                        .pb_1()
                        .gap_1()
                        .flex_wrap()
                        .items_center();
                    for (tix, term) in terms.iter().enumerate() {
                        let this = this.clone();
                        bar = bar.child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_0p5()
                                .rounded(px(3.))
                                .bg(theme.muted.opacity(0.4))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.foreground)
                                        .child(term.clone()),
                                )
                                .when(writable, |chip| {
                                    chip.child(
                                        div()
                                            .id(("filter-chip", tix))
                                            .flex()
                                            .cursor_pointer()
                                            .text_color(theme.muted_foreground)
                                            .hover(|s| s.text_color(theme.danger))
                                            .child(Icon::new(assets::IconName::X).size(px(10.)))
                                            .on_click(move |_, window, cx| {
                                                this.update(cx, |view, cx| {
                                                    if let SpecSrc::Doc(doc) = &view.spec_src {
                                                        doc.update(cx, |doc, cx| {
                                                            doc.remove_base_view_filter(
                                                                view.view_ix,
                                                                tix,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                    view.doc_epoch += 1;
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                }),
                        );
                    }
                    if addable {
                        let this = this.clone();
                        let groupable = computed.groupable.clone();
                        let view_ix = self.view_ix;
                        bar = bar.child(
                            div()
                                .id("filter-chip-add")
                                .flex()
                                .cursor_pointer()
                                .items_center()
                                .gap_1()
                                .px_2()
                                .py_0p5()
                                .rounded(px(3.))
                                .text_color(theme.muted_foreground)
                                .hover(|s| {
                                    s.bg(theme.muted.opacity(0.4)).text_color(theme.foreground)
                                })
                                .child(Icon::new(assets::IconName::Plus).size(px(10.)))
                                .child(div().text_xs().child("Filter"))
                                .on_click(move |_, window, cx| {
                                    filter_pick_prop(
                                        this.clone(),
                                        view_ix,
                                        groupable.clone(),
                                        window,
                                        cx,
                                    );
                                }),
                        );
                    }
                    Some(bar)
                }
            })
            .when(!cards && !kanban && !calendar && !list, |v| v.child(header))
            .children(computed.error.iter().map(|e| {
                div()
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(e.clone())
            }))
            // Kanban columns run past the pane's width — the board
            // scrolls horizontally (and vertically) rather than
            // clipping the rightmost column.
            .child(if kanban {
                gpui_kit::component::scroll::ScrollableElement::overflow_scrollbar(
                    rows.flex_1().min_h_0(),
                )
            } else {
                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                    rows.flex_1().min_h_0(),
                )
            })
            .child(
                div()
                    .w_full()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(if calendar {
                        let undated = computed
                            .rows
                            .iter()
                            .filter(|r| {
                                r.cells
                                    .get(computed.group_ix.unwrap_or(usize::MAX))
                                    .and_then(|c| parse_date(&c.text))
                                    .is_none()
                            })
                            .count();
                        format!(
                            "{} {} · {} undated",
                            visible.len(),
                            if visible.len() == 1 { "note" } else { "notes" },
                            undated
                        )
                    } else {
                        let noun = if visible.len() == 1 { "note" } else { "notes" };
                        // Show the pre-filter total when the search box
                        // is hiding rows — "8 of 10 notes".
                        if query.is_empty() || visible.len() == computed.rows.len() {
                            format!("{} {}", visible.len(), noun)
                        } else {
                            format!("{} of {} {}", visible.len(), computed.rows.len(), noun)
                        }
                    }),
            )
    }
}

#[cfg(test)]
mod moment_tests {
    use super::{format_moment, moment_locale_weeks};

    fn at(y: i32, m: u32, d: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(14, 5, 0)
            .unwrap()
    }

    #[test]
    fn formats_plain_tokens_and_literals() {
        assert_eq!(format_moment(at(2026, 10, 9), "YYYY-MM-DD"), "2026-10-09");
        assert_eq!(
            format_moment(at(2026, 10, 9), "YYYYMMDDHHmm"),
            "202610091405"
        );
        assert_eq!(
            format_moment(at(2026, 10, 9), "[Week] YYYY 100%"),
            "Week 2026 100%"
        );
        assert_eq!(
            format_moment(at(2026, 10, 9), "D. MMMM · Øy"),
            "9. October · Øy"
        );
    }

    #[test]
    fn formats_quarters_and_weeks() {
        assert_eq!(format_moment(at(2026, 10, 9), "YYYY-[Q]Q"), "2026-Q4");
        assert_eq!(format_moment(at(2026, 2, 1), "YYYY-[Q]Q"), "2026-Q1");
        // Thu 2026-10-09: ISO week 41; locale week (Sun-first) 41.
        assert_eq!(format_moment(at(2026, 10, 9), "GGGG-[W]WW"), "2026-W41");
        assert_eq!(format_moment(at(2026, 10, 9), "gggg-[W]ww"), "2026-W41");
        // Thu 2026-12-31: its Sun–Sat week holds Jan 1 2027 → locale
        // 2027-W01; ISO puts it in 2026-W53.
        assert_eq!(format_moment(at(2026, 12, 31), "gggg-[W]ww"), "2027-W01");
        assert_eq!(format_moment(at(2026, 12, 31), "GGGG-[W]WW"), "2026-W53");
        // Fri 2027-01-01 is ISO 2026-W53, locale 2027-W01.
        assert_eq!(format_moment(at(2027, 1, 1), "GGGG-[W]W"), "2026-W53");
        assert_eq!(format_moment(at(2027, 1, 3), "gggg-[W]w"), "2027-W2");
    }

    #[test]
    fn detects_locale_week_patterns_outside_literals() {
        assert!(moment_locale_weeks("gggg-[W]ww"));
        assert!(!moment_locale_weeks("GGGG-[W]WW"));
        assert!(!moment_locale_weeks("YYYY-MM-[week]"));
    }

    #[test]
    fn formats_ordinal_days() {
        assert_eq!(format_moment(at(2026, 10, 9), "Do MMMM"), "9th October");
        assert_eq!(format_moment(at(2026, 10, 1), "Do"), "1st");
        assert_eq!(format_moment(at(2026, 10, 22), "Do"), "22nd");
        assert_eq!(format_moment(at(2026, 10, 13), "Do"), "13th");
    }
}

#[cfg(test)]
mod tests {
    use super::{eval, eval_filter_node, parse_expr, parse_spec, prefill_pairs, Env, Lit, RowData};
    use serde_yaml::Value;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn env_with(pairs: &[(&str, Lit)]) -> (Vec<RowData>, BTreeMap<String, super::Expr>) {
        let mut props = BTreeMap::new();
        for (k, v) in pairs {
            props.insert(k.to_string(), v.clone());
        }
        (
            vec![RowData {
                path: PathBuf::from("notes/x.md"),
                props,
                link_props: Default::default(),
                file_meta: BTreeMap::new(),
            }],
            BTreeMap::new(),
        )
    }

    fn evals(src: &str, pairs: &[(&str, Lit)]) -> Lit {
        let (rows, formulas) = env_with(pairs);
        let no_resolve = |_: &str| None;
        let mut env = Env {
            row: &rows[0],
            rows: &rows,
            formulas: &formulas,
            values: None,
            this_row: None,
            resolve: &no_resolve,
            depth: 0,
        };
        eval(&parse_expr(src).unwrap(), &mut env).unwrap()
    }

    #[test]
    fn comparisons_and_logic() {
        let p = [
            ("status", Lit::Str("done".into())),
            ("rating", Lit::Num(4.0)),
        ];
        assert_eq!(evals(r#"status == "done""#, &p), Lit::Bool(true));
        assert_eq!(evals("rating > 3 and rating < 5", &p), Lit::Bool(true));
        assert_eq!(evals("not (status == \"todo\")", &p), Lit::Bool(true));
        assert_eq!(evals("status != \"todo\" or false", &p), Lit::Bool(true));
    }

    #[test]
    fn methods_and_arithmetic() {
        let p = [
            ("path", Lit::Str("notes/daily/2026.md".into())),
            (
                "tags",
                Lit::List(vec![Lit::Str("a".into()), Lit::Str("b".into())]),
            ),
            ("n", Lit::Num(10.0)),
        ];
        assert_eq!(evals(r#"path.startsWith("notes/")"#, &p), Lit::Bool(true));
        assert_eq!(evals(r#"tags.contains("b")"#, &p), Lit::Bool(true));
        assert_eq!(evals("n / 4 + 1", &p), Lit::Num(3.5));
        assert_eq!(
            evals(r#"path.trim().title()"#, &p),
            Lit::Str("Notes/daily/2026.md".into())
        );
        assert_eq!(
            evals(r#"path.split("/").slice(1, 2).join(",")"#, &p),
            Lit::Str("daily,2026.md".into())
        );
        assert_eq!(evals(r#"tags.reverse().last()"#, &p), Lit::Str("a".into()));
        assert_eq!(evals(r#"tags.indexOf("b")"#, &p), Lit::Num(1.0));
        assert_eq!(
            evals(r#"replace(path, "notes", "vault")"#, &p),
            Lit::Str("vault/daily/2026.md".into())
        );
        assert_eq!(
            evals(r#"path.split("/").sort().join(",")"#, &p),
            Lit::Str("2026.md,daily,notes".into())
        );
        let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
        assert_eq!(
            evals(&format!(r#"today() == date("{today}")"#), &p),
            Lit::Bool(true)
        );
    }

    #[test]
    fn date_functions() {
        let p: [(&str, Lit); 0] = [];
        // Month clamp: Jan 31 + 1 month → Feb 28.
        assert_eq!(
            evals(
                r#"format(dateAdd(date("2026-01-31"), "1 month"), "YYYY-MM-DD")"#,
                &p
            ),
            Lit::Str("2026-02-28".into())
        );
        assert_eq!(
            evals(
                r#"format(dateSubtract(date("2026-06-15"), "2 weeks"), "YYYY-MM-DD")"#,
                &p
            ),
            Lit::Str("2026-06-01".into())
        );
        assert_eq!(evals(r#"duration("1 week")"#, &p), Lit::Num(604800.0));
        assert_eq!(evals(r#"year(date("2026-01-31"))"#, &p), Lit::Num(2026.0));
        // 2026-01-31 is a Saturday → ISO weekday 6.
        assert_eq!(evals(r#"weekday(date("2026-01-31"))"#, &p), Lit::Num(6.0));
        assert_eq!(
            evals(r#"format(date("2026-01-31"), "MMM Do")"#, &p),
            Lit::Str("Jan 31".into())
        );
        assert_eq!(
            evals(r#"format(datetime("2026-01-31 09:30"), "HH:mm")"#, &p),
            Lit::Str("09:30".into())
        );
        assert_eq!(evals(r#"number("42")"#, &p), Lit::Num(42.0));
        assert_eq!(evals(r#"list("a").join(",")"#, &p), Lit::Str("a".into()));
    }

    #[test]
    fn rollup_and_link() {
        // Two notes: x links to y; y carries the aggregated field.
        let resolve = |t: &str| {
            (t == "notes/y.md" || t == "y" || t == "y.md").then(|| PathBuf::from("notes/y.md"))
        };
        let mut y_props = BTreeMap::new();
        y_props.insert("hours".to_string(), Lit::Num(3.0));
        let mut x_props = BTreeMap::new();
        x_props.insert("related".to_string(), Lit::Str("notes/y.md".into()));
        let rows = vec![
            RowData {
                path: PathBuf::from("notes/x.md"),
                props: x_props,
                link_props: Default::default(),
                file_meta: BTreeMap::new(),
            },
            RowData {
                path: PathBuf::from("notes/y.md"),
                props: y_props,
                link_props: Default::default(),
                file_meta: BTreeMap::new(),
            },
        ];
        let formulas = BTreeMap::new();
        let this_row = None;
        let mut env = Env {
            row: &rows[0],
            rows: &rows,
            formulas: &formulas,
            values: None,
            this_row,
            resolve: &resolve,
            depth: 0,
        };
        let run = |src: &str, env: &mut Env| eval(&parse_expr(src).unwrap(), env).unwrap();
        assert_eq!(run(r#"link("y")"#, &mut env), Lit::Str("notes/y.md".into()));
        assert_eq!(
            run(r#"related.contains(link("y"))"#, &mut env),
            Lit::Bool(true)
        );
        assert_eq!(
            run(r#"rollup(related, "hours", "sum")"#, &mut env),
            Lit::Num(3.0)
        );
        assert_eq!(
            run(r#"rollup(related, "missing", "count")"#, &mut env),
            Lit::Num(0.0)
        );
    }

    #[test]
    fn list_predicates() {
        let p = [(
            "tags",
            Lit::List(vec![
                Lit::Str("a".into()),
                Lit::Str("b".into()),
                Lit::Str("c".into()),
            ]),
        )];
        assert_eq!(
            evals(r#"tags.containsAll(["a", "b"])"#, &p),
            Lit::Bool(true)
        );
        assert_eq!(
            evals(r#"tags.containsAll(["a", "z"])"#, &p),
            Lit::Bool(false)
        );
        assert_eq!(
            evals(r#"tags.containsAny(["x", "b"])"#, &p),
            Lit::Bool(true)
        );
        assert_eq!(
            evals(r#"tags.containsAny(["x", "z"])"#, &p),
            Lit::Bool(false)
        );
        assert_eq!(
            evals(r#"tags.containsNone(["x", "z"])"#, &p),
            Lit::Bool(true)
        );
        assert_eq!(evals(r#"tags.containsNone(["b"])"#, &p), Lit::Bool(false));
        // function form
        assert_eq!(evals(r#"containsAny(tags, ["c"])"#, &p), Lit::Bool(true));
    }

    #[test]
    fn filter_tree() {
        let (rows, formulas) = env_with(&[("status", Lit::Str("draft".into()))]);
        let no_resolve = |_: &str| None;
        let mut env = Env {
            row: &rows[0],
            rows: &rows,
            formulas: &formulas,
            values: None,
            this_row: None,
            resolve: &no_resolve,
            depth: 0,
        };
        let spec: Value = serde_yaml::from_str(
            "and:\n  - 'status == \"draft\"'\n  - or:\n      - 'false'\n      - 'true'",
        )
        .unwrap();
        assert!(eval_filter_node(&spec, &mut env).unwrap());
    }

    #[test]
    fn prefill() {
        let spec = parse_spec(
            r#"
filters:
  and:
    - 'file.ext == "md"'
    - 'status == "draft"'
views:
  - type: table
    name: Drafts
    filters:
      and:
        - 'priority == 2'
        - or:
            - 'kind == "x"'
            - 'kind == "y"'
"#,
        );
        let view = &spec.views[0];
        let pairs = prefill_pairs(&spec, view);
        // file.* refs are metadata (not frontmatter), or-branches aren't
        // guaranteed — only the two `and` equalities prefill.
        assert_eq!(
            pairs,
            vec![
                ("priority".to_string(), "2".to_string()),
                ("status".to_string(), "\"draft\"".to_string()),
            ]
        );
    }

    #[test]
    fn splice_order() {
        let spec = r#"views:
  - type: table
    name: All notes
    group_by: file.folder
    order:
      - file.name
      - status
    sort: file.name
  - type: cards
    name: Gallery
    order: [file.name, cover]
"#;
        // Block list — append under the first view's `order:`.
        let (s, e, ins) = super::splice_order(spec, 0, "tags").unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("      - status\n      - tags\n"));
        assert!(out.contains("order: [file.name, cover]"));
        // Flow list — the second view's `order:` line is rewritten.
        let (s, e, ins) = super::splice_order(spec, 1, "rating").unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("order: [file.name, cover, rating]"));
        assert!(out.contains("      - status\n"));
        // No `order:` at all — appended at the end of the view item.
        let bare = "views:\n  - type: table\n    name: Bare\n";
        let (s, e, ins) = super::splice_order(bare, 0, "status").unwrap();
        let out = format!("{}{}{}", &bare[..s], ins, &bare[e..]);
        assert!(out.contains("    order:\n      - status\n"));
        // A second view keeps the first untouched.
        assert!(super::splice_order(spec, 5, "x").is_none());
    }

    #[test]
    fn splice_order_at() {
        let spec = r#"views:
  - type: table
    name: All notes
    order:
      - file.name
      - status
      - file.mtime
  - type: cards
    name: Gallery
    order: [file.name, cover]
"#;
        // Block list — before the anchor.
        let (s, e, ins) = super::splice_order_at(spec, 0, "file.folder", "status", false).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("      - file.folder\n      - status\n"));
        // Block list — after the anchor.
        let (s, e, ins) = super::splice_order_at(spec, 0, "tags", "status", true).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("      - status\n      - tags\n      - file.mtime\n"));
        // Block list — anchor missing appends after the last entry.
        let (s, e, ins) = super::splice_order_at(spec, 0, "due", "nope", false).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("      - file.mtime\n      - due\n"));
        // Flow list — before/after rewrite the one line positionally.
        let (s, e, ins) = super::splice_order_at(spec, 1, "rating", "cover", false).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("order: [file.name, rating, cover]"));
        let (s, e, ins) = super::splice_order_at(spec, 1, "rating", "file.name", true).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("order: [file.name, rating, cover]"));
        // No `order:` — appended at the end of the view item.
        let bare = "views:\n  - type: table\n    name: Bare\n";
        let (s, e, ins) = super::splice_order_at(bare, 0, "status", "x", true).unwrap();
        let out = format!("{}{}{}", &bare[..s], ins, &bare[e..]);
        assert!(out.contains("    order:\n      - status\n"));
    }

    #[test]
    fn splice_view_map_entry() {
        let spec = r#"views:
  - type: table
    name: All notes
    summaries:
      words: average
    order: [file.name]
  - type: table
    name: Flow
    summaries: {size: sum}
  - type: cards
    name: Bare
"#;
        // Block map — a new entry appends at the map's end.
        let (s, e, ins) =
            super::splice_view_map_entry(spec, 0, "summaries", "size", Some("sum")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("      words: average\n      size: sum\n    order:"));
        // Same view, second entry — appends under the map.
        let (s, e, ins) =
            super::splice_view_map_entry(&out, 0, "summaries", "count", Some("unique")).unwrap();
        let out2 = format!("{}{}{}", &out[..s], ins, &out[e..]);
        assert!(out2.contains("      size: sum\n      count: unique\n"));
        // Rewrite an entry in place.
        let (s, e, ins) =
            super::splice_view_map_entry(&out2, 0, "summaries", "size", Some("max")).unwrap();
        let out3 = format!("{}{}{}", &out2[..s], ins, &out2[e..]);
        assert!(out3.contains("      words: average\n      size: max\n      count: unique\n"));
        // Delete one of three — the map key survives.
        let (s, e, ins) =
            super::splice_view_map_entry(&out3, 0, "summaries", "size", None).unwrap();
        let out4 = format!("{}{}{}", &out3[..s], ins, &out3[e..]);
        assert!(out4.contains("    summaries:\n      words: average\n"));
        // Down to one entry — the map key still survives.
        let (s, e, ins) =
            super::splice_view_map_entry(&out4, 0, "summaries", "words", None).unwrap();
        let out5 = format!("{}{}{}", &out4[..s], ins, &out4[e..]);
        assert!(out5.contains("    summaries:\n      count: unique\n"));
        // Delete the last — the whole `summaries:` block leaves.
        let (s, e, ins) =
            super::splice_view_map_entry(&out5, 0, "summaries", "count", None).unwrap();
        let out6 = format!("{}{}{}", &out5[..s], ins, &out5[e..]);
        assert!(!out6.contains("    summaries:\n"));
        assert!(out6.contains("summaries: {size: sum}"));
        // Flow map — converts to block with the entry upserted.
        let (s, e, ins) =
            super::splice_view_map_entry(spec, 1, "summaries", "count", Some("unique")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("    summaries:\n      size: sum\n      count: unique\n"));
        // No `summaries:` — created right after the `- ` line.
        let (s, e, ins) =
            super::splice_view_map_entry(spec, 2, "summaries", "size", Some("sum")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("  - type: cards\n    summaries:\n      size: sum\n    name: Bare\n"));
        // Nothing to delete when the key is absent.
        assert!(super::splice_view_map_entry(spec, 2, "summaries", "size", None).is_none());
    }

    #[test]
    fn splice_root_map_entry() {
        let spec = r#"filters:
  and:
    - 'file.ext == "md"'
formulas:
  words: 'file.size / 5'
  hours: 'rollup(file.backlinks, "hours", "sum")'
views:
  - type: table
    name: All notes
"#;
        // Rewrite an existing entry in place.
        let (s, e, ins) =
            super::splice_root_map_entry(spec, "formulas", "words", Some("'file.size / 6'"))
                .unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("  words: 'file.size / 6'"));
        assert!(out.contains("hours:"));
        // New entry — appended at map end, before views:.
        let (s, e, ins) =
            super::splice_root_map_entry(spec, "formulas", "days", Some("'today()'")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("  days: 'today()'\nviews:\n"));
        // Delete one of two — map survives.
        let (s, e, ins) = super::splice_root_map_entry(spec, "formulas", "words", None).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(!out.contains("words:"));
        assert!(out.contains("formulas:\n  hours:"));
        // Delete the last — the whole map key leaves.
        let (s, e, ins) = super::splice_root_map_entry(&out, "formulas", "hours", None).unwrap();
        let out2 = format!("{}{}{}", &out[..s], ins, &out[e..]);
        assert!(!out2.contains("formulas:"));
        assert!(out2.contains("views:"));
        // No map — created before views:.
        let bare = "views:\n  - type: table\n    name: T\n";
        let (s, e, ins) = super::splice_root_map_entry(bare, "formulas", "w", Some("'1'")).unwrap();
        let out = format!("{}{}{}", &bare[..s], ins, &bare[e..]);
        assert!(out.starts_with("formulas:\n  w: '1'\nviews:\n"));
        // Nothing to delete when the map is absent.
        assert!(super::splice_root_map_entry(bare, "formulas", "w", None).is_none());
    }

    #[test]
    fn splice_root_display_name() {
        let spec = r#"filters:
  and:
    - 'file.ext == "md"'
properties:
  status:
    displayName: Status
  count:
    displayName: Count
views:
  - type: table
    name: All notes
"#;
        // Rewrite an existing displayName in place.
        let (s, e, ins) = super::splice_root_display_name(spec, "status", Some("State")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("displayName: State"));
        assert!(out.contains("displayName: Count"));
        // New entry — appended at the map's end before views:.
        let (s, e, ins) =
            super::splice_root_display_name(spec, "file.mtime", Some("Modified")).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(out.contains("  file.mtime:\n    displayName: Modified\nviews:\n"));
        // Clear one of two — the entry line and properties: survive.
        let (s, e, ins) = super::splice_root_display_name(spec, "status", None).unwrap();
        let out = format!("{}{}{}", &spec[..s], ins, &spec[e..]);
        assert!(!out.contains("displayName: Status"));
        assert!(out.contains("properties:\n  count:\n    displayName: Count\n"));
        // Clear the last — the whole properties: block leaves.
        let (s, e, ins) = super::splice_root_display_name(&out, "count", None).unwrap();
        let out2 = format!("{}{}{}", &out[..s], ins, &out[e..]);
        assert!(!out2.contains("properties:"));
        assert!(out2.contains("views:"));
        // No properties: — created before views:.
        let bare = "views:\n  - type: table\n    name: T\n";
        let (s, e, ins) = super::splice_root_display_name(bare, "status", Some("State")).unwrap();
        let out = format!("{}{}{}", &bare[..s], ins, &bare[e..]);
        assert!(out.starts_with("properties:\n  status:\n    displayName: State\nviews:\n"));
        // Nothing to delete when properties: is absent.
        assert!(super::splice_root_display_name(bare, "status", None).is_none());
    }

    #[test]
    fn drop_order() {
        let spec = r#"views:
  - type: table
    name: All notes
    order:
      - file.name
      - file.folder
      - cover
  - type: cards
    name: Gallery
    order: [file.name, cover, rating]
"#;
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        // Block list — the `- cover` line disappears.
        let out = apply(spec, super::drop_order(spec, 0, "cover", &[]).unwrap());
        assert!(out.contains("      - file.folder\n"));
        assert!(!out.contains("- cover"));
        // Flow — only `rating` leaves the list.
        let out = apply(spec, super::drop_order(spec, 1, "rating", &[]).unwrap());
        assert!(out.contains("order: [file.name, cover]"));
        // Prop not listed → nothing to do.
        assert!(super::drop_order(spec, 0, "status", &[]).is_none());
        // Single-entry list collapses to `file.name` (keeps the row key).
        let one = "views:\n  - type: table\n    order: [status]\n";
        let out = apply(one, super::drop_order(one, 0, "status", &[]).unwrap());
        assert!(out.contains("order: [file.name]"));
        // No `order:` — writes the displayed set minus the hidden prop.
        let bare = "views:\n  - type: table\n    name: Bare\n";
        let cols = vec![
            "file.name".to_string(),
            "status".to_string(),
            "tags".to_string(),
        ];
        let out = apply(bare, super::drop_order(bare, 0, "status", &cols).unwrap());
        assert!(out.contains("    order:\n      - file.name\n      - tags\n"));
    }

    #[test]
    fn reorder_order() {
        let spec = r#"views:
  - type: table
    name: All notes
    order:
      - file.name
      - file.folder
      - cover
    limit: 10
  - type: cards
    name: Gallery
    order: [file.name, cover]
"#;
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        let cols = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Block — the whole key span is rewritten in the new sequence,
        // and sibling keys (`limit:`) stay put.
        let out = apply(
            spec,
            super::reorder_order(spec, 0, &cols(&["cover", "file.name", "file.folder"])).unwrap(),
        );
        assert!(out.contains(
            "    order:\n      - cover\n      - file.name\n      - file.folder\n    limit: 10\n"
        ));
        // Flow — rewritten as a canonical block list.
        let out = apply(
            spec,
            super::reorder_order(spec, 1, &cols(&["cover", "file.name"])).unwrap(),
        );
        assert!(out.contains("    order:\n      - cover\n      - file.name\n"));
        assert!(!out.contains("order: [file.name, cover]"));
        // No `order:` — appended at the item's end.
        let bare = "views:\n  - type: table\n    name: Bare\n";
        let out = apply(
            bare,
            super::reorder_order(bare, 0, &cols(&["file.name", "status"])).unwrap(),
        );
        assert!(out.contains("    order:\n      - file.name\n      - status\n"));
    }

    #[test]
    fn splice_view() {
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        // Appends after the last item, before the next top-level key.
        let spec = "views:\n  - type: table\n    name: All\nfilters: x\n";
        let out = apply(spec, super::splice_view(spec, "Board", "board").unwrap());
        assert!(out.contains("  - type: board\n    name: Board\nfilters: x\n"));
        // Names needing quotes are quoted.
        let out = apply(
            spec,
            super::splice_view(spec, "A \"weird\" name", "list").unwrap(),
        );
        assert!(out.contains("name: \"A \\\"weird\\\" name\""));
        // No `views:` key — one is appended at the end.
        let bare = "filters: 'file.ext == \"md\"'\n";
        let out = apply(bare, super::splice_view(bare, "All", "table").unwrap());
        assert!(out.ends_with("views:\n  - type: table\n    name: All\n"));
        // Empty `views:` block still gets a well-formed item.
        let empty = "views:\n";
        let out = apply(empty, super::splice_view(empty, "Cards", "cards").unwrap());
        assert!(out.contains("  - type: cards\n    name: Cards\n"));
    }

    #[test]
    fn reorder_views() {
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        let spec = "views:\n  - type: table\n    name: A\n  - type: cards\n    name: B\n    order: [file.name]\n  - type: list\n    name: C\nfilters: x\n";
        // First → last: B and C keep their text, `filters:` untouched.
        let out = apply(spec, super::reorder_views(spec, 0, 2).unwrap());
        assert!(out.contains(
            "  - type: cards\n    name: B\n    order: [file.name]\n  - type: list\n    name: C\n  - type: table\n    name: A\nfilters: x\n"
        ));
        // Last → first.
        let out = apply(spec, super::reorder_views(spec, 2, 0).unwrap());
        assert!(out.contains("  - type: list\n    name: C\n  - type: table\n    name: A\n"));
        // Same index / out of range → nothing.
        assert!(super::reorder_views(spec, 1, 1).is_none());
        assert!(super::reorder_views(spec, 0, 5).is_none());
    }

    #[test]
    fn splice_view_key() {
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        let spec = "views:\n  - type: table\n    name: A\n    group_by: status\n  - type: cards\n    name: B\n";
        // Rewrite an existing key.
        let out = apply(
            spec,
            super::splice_view_key(spec, 0, "group_by", Some("file.folder")).unwrap(),
        );
        assert!(out.contains("    group_by: file.folder\n  - type: cards"));
        // Clear it.
        let out = apply(
            spec,
            super::splice_view_key(spec, 0, "group_by", None).unwrap(),
        );
        assert!(out.contains("    name: A\n  - type: cards"));
        assert!(!out.contains("group_by"));
        // Insert after `type:` when the key is absent.
        let out = apply(
            spec,
            super::splice_view_key(spec, 1, "group_by", Some("status")).unwrap(),
        );
        assert!(out.contains("  - type: cards\n    group_by: status\n    name: B\n"));
        // Clearing a missing key → nothing.
        assert!(super::splice_view_key(spec, 1, "group_by", None).is_none());
        // A block value leaves with its key line — no orphans.
        let spec = "views:\n  - type: table\n    name: A\n    sort:\n      - property: x\n        direction: DESC\n    order: [file.name]\n";
        let out = apply(
            spec,
            super::splice_view_key(spec, 0, "sort", Some("file.mtime")).unwrap(),
        );
        assert!(out.contains("    sort: file.mtime\n    order: [file.name]"));
        assert!(!out.contains("property: x"));
    }

    #[test]
    fn splice_view_filter() {
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        // No filters key → fresh and-block after the `- ` line.
        let spec = "views:\n  - type: table\n    name: A\n";
        let out = apply(
            spec,
            super::splice_view_filter(spec, 0, "status == \"done\"").unwrap(),
        );
        assert!(out.contains(
            "  - type: table\n    filters:\n      and:\n        - 'status == \"done\"'\n    name: A\n"
        ));
        // Existing and-block → appends at the item indent.
        let spec = "views:\n  - type: table\n    name: A\n    filters:\n      and:\n        - 'a == 1'\n    order: [file.name]\n";
        let out = apply(spec, super::splice_view_filter(spec, 0, "b == 2").unwrap());
        assert!(out.contains("        - 'a == 1'\n        - 'b == 2'\n    order:"));
        // Scalar form → converts to an and-block keeping the old expr.
        let spec = "views:\n  - type: table\n    name: A\n    filters: 'a == 1'\n";
        let out = apply(spec, super::splice_view_filter(spec, 0, "b == 2").unwrap());
        assert!(out.contains("    filters:\n      and:\n        - 'a == 1'\n        - 'b == 2'\n"));
        // A `'` inside the expression doubles per YAML quoting.
        let spec = "views:\n  - type: table\n    name: A\n";
        let out = apply(
            spec,
            super::splice_view_filter(spec, 0, "name == \"it's\"").unwrap(),
        );
        assert!(out.contains("- 'name == \"it''s\"'"));
    }

    #[test]
    fn splice_view_filter_remove() {
        let apply =
            |src: &str, r: (usize, usize, String)| format!("{}{}{}", &src[..r.0], r.2, &src[r.1..]);
        // Middle of three → only that item leaves.
        let spec = "views:\n  - type: table\n    filters:\n      and:\n        - 'a == 1'\n        - 'b == 2'\n        - 'c == 3'\n    order: [file.name]\n";
        let out = apply(spec, super::splice_view_filter_remove(spec, 0, 1).unwrap());
        assert!(out.contains("        - 'a == 1'\n        - 'c == 3'\n    order:"));
        // Last of two → the other stays, `and:`/`filters:` survive.
        let out = apply(spec, super::splice_view_filter_remove(spec, 0, 1).unwrap());
        assert!(out.contains("    filters:\n      and:\n        - 'a == 1'\n        - 'c == 3'"));
        // Single item → the whole `filters:` block drops.
        let spec = "views:\n  - type: table\n    filters:\n      and:\n        - 'a == 1'\n    order: [file.name]\n";
        let out = apply(spec, super::splice_view_filter_remove(spec, 0, 0).unwrap());
        assert!(out.contains("  - type: table\n    order: [file.name]\n"));
        // Scalar `filters: 'e'` → the key line drops.
        let spec = "views:\n  - type: table\n    filters: 'a == 1'\n    order: [file.name]\n";
        let out = apply(spec, super::splice_view_filter_remove(spec, 0, 0).unwrap());
        assert!(out.contains("  - type: table\n    order: [file.name]\n"));
        // Out of range → None.
        assert!(super::splice_view_filter_remove(spec, 0, 2).is_none());
    }
}
