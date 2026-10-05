//! Obsidian Bases — `.base` files are YAML queries over vault
//! frontmatter. We adopt Obsidian's own spec (filters / formulas /
//! views / properties) so a vault reads identically in both apps.
//!
//! Supported subset:
//! - `filters:` — `and`/`or`/`not` trees of expressions:
//!   `prop == "x"` `!=` `>` `>=` `<` `<=`, arithmetic `+ - * / %`,
//!   `prop.contains("x")`, `startsWith`, `endsWith`, `isEmpty`,
//!   `file.name/path/ext/folder/mtime/ctime/size`, `note.prop`,
//!   `formula.x`, functions `contains(a,b)`/`startsWith`/`endsWith`/
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
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------
// Literal value space — frontmatter YAML + file metadata + formulas.
// ------------------------------------------------------------------

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
) -> (RowData, Vec<String>) {
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
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let targets = link_targets(&text);
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
    )
}

struct Env<'a> {
    row: &'a RowData,
    /// Every note in the vault — rollup + link resolution reach past
    /// the filtered row set.
    rows: &'a [RowData],
    formulas: &'a BTreeMap<String, Expr>,
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

fn eval(expr: &Expr, env: &mut Env) -> Result<Lit, String> {
    if env.depth > 32 {
        return Err("formula recursion".into());
    }
    match expr {
        Expr::Lit(l) => Ok(l.clone()),
        Expr::Ref(ns, name) => match ns.as_deref() {
            None => Ok(env
                .row
                .props
                .get(name)
                .cloned()
                // Bare `file.*`/`note.*`-less refs to file metadata work too.
                .or_else(|| env.row.file_meta.get(name).cloned())
                .unwrap_or(Lit::Null)),
            Some("file") => Ok(env.row.file_meta.get(name).cloned().unwrap_or(Lit::Null)),
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
            let value = eval(target, env)?;
            let arg = match args.len() {
                0 => None,
                1 => Some(eval(&args[0], env)?),
                _ => return Err(format!("{name} takes one argument")),
            };
            apply_method(&value, name, arg.as_ref())
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

fn apply_method(value: &Lit, name: &str, arg: Option<&Lit>) -> Result<Lit, String> {
    match name {
        "contains" => match (value, arg) {
            (Lit::Str(s), Some(needle)) => Ok(Lit::Bool(s.contains(&needle.display()))),
            (Lit::List(items), Some(needle)) => {
                Ok(Lit::Bool(items.iter().any(|i| lit_eq(i, needle))))
            }
            _ => Ok(Lit::Bool(false)),
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
        _ => Err(format!("unknown method '{name}'")),
    }
}

fn apply_fn(name: &str, args: &[Lit]) -> Result<Lit, String> {
    match name {
        "contains" | "startsWith" | "endsWith" | "isEmpty" | "lower" | "upper" => {
            let (value, arg) = match args {
                [v] => (v, None),
                [v, a] => (v, Some(a)),
                _ => return Err(format!("{name} wants 1–2 args")),
            };
            apply_method(value, name, arg)
        }
        "sum" | "avg" | "mean" | "min" | "max" | "count" | "len" | "first" => match args {
            [Lit::List(items)] => aggregate(name, items.clone()),
            vals => aggregate(name, vals.to_vec()),
        },
        "now" => Ok(Lit::Num(chrono::Local::now().timestamp() as f64)),
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
        _ => Err(format!("unknown function '{name}'")),
    }
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
    /// Calendar: property the month grid buckets on
    /// (`date:`/`dateProperty:`/`date_property:`/`property:`).
    date_prop: Option<String>,
    columns: Vec<String>,
    sort: Vec<SortKey>,
    limit: Option<usize>,
    filters: Option<Value>,
}

struct BaseSpec {
    filters: Option<Value>,
    formulas: BTreeMap<String, Expr>,
    properties: BTreeMap<String, String>,
    views: Vec<ViewSpec>,
    error: Option<String>,
}

fn parse_spec(yaml: &str) -> BaseSpec {
    let parsed = serde_yaml::from_str::<Value>(yaml);
    let mut spec = BaseSpec {
        filters: None,
        formulas: BTreeMap::new(),
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
            let group_by = ["group_by", "groupBy", "group"]
                .iter()
                .find_map(|k| getv(k).and_then(|g| g.as_str()).map(str::to_string));
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
            spec.views.push(ViewSpec {
                name,
                kind,
                group_by,
                date_prop,
                columns,
                sort,
                limit,
                filters: getv("filters").cloned(),
            });
        }
    }

    // A base with no views gets a default table.
    if spec.views.is_empty() {
        spec.views.push(ViewSpec {
            name: "Table".into(),
            kind: "table".into(),
            group_by: None,
            date_prop: None,
            columns: Vec::new(),
            sort: Vec::new(),
            limit: None,
            filters: None,
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

/// Stem of a resolved path, or the string itself when it isn't one.
fn stem_of(s: &str) -> String {
    let p = Path::new(s);
    match (p.file_stem(), p.extension()) {
        (Some(stem), Some(_)) => stem.to_string_lossy().to_string(),
        _ => s.to_string(),
    }
}

struct Computed {
    headers: Vec<String>,
    rows: Vec<Row>,
    view_names: Vec<String>,
    /// `table`, `cards`, or `kanban` — picked per selected view.
    kind: String,
    /// Kanban grouping column index into `headers`/`cells`, when resolved.
    group_ix: Option<usize>,
    /// Kanban: frontmatter key the board groups on — card drops write to
    /// it, so `formula.`/`file.` columns are excluded.
    group_prop: Option<String>,
    /// Frontmatter pairs a new note needs to satisfy the base + view
    /// filters — `prop == literal` under conjunctions only.
    prefill: Vec<(String, String)>,
    error: Option<String>,
}

/// First image-ish property a note declares — `cover`, `banner`, `image`.
/// `![[name]]`/`[[name]]`/`![](url)` wrappers are unwrapped; vault basenames
/// resolve through the image index, relative paths against the vault root.
fn cover_of(
    row: &RowData,
    root: &Path,
    images: &std::collections::HashMap<String, PathBuf>,
) -> Option<String> {
    let raw = ["cover", "banner", "image", "cover_image"]
        .iter()
        .find_map(|key| match row.props.get(*key) {
            Some(Lit::Str(s)) => Some(s.clone()),
            Some(Lit::List(items)) => items.iter().find_map(|i| match i {
                Lit::Str(s) => Some(s.clone()),
                _ => None,
            }),
            _ => None,
        })?;
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
    matches!(name, "file.mtime" | "file.ctime" | "file.date" | "date")
        || name.strip_prefix("note.").is_some_and(|p| p == "date")
}

fn compute(
    spec: &BaseSpec,
    view: &ViewSpec,
    notes: &[PathBuf],
    root: &Path,
    images: &std::collections::HashMap<String, PathBuf>,
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
    for note in notes {
        let (row, targets) = row_data(root, note, &resolve);
        resolved_targets.push(targets.iter().filter_map(|t| resolve(t)).collect());
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

    let mut rows_ix = Vec::new();
    for (i, row) in all_rows.iter().enumerate() {
        let mut env = Env {
            row,
            rows: &all_rows,
            formulas: &spec.formulas,
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

    // Kanban boards may group on a property not listed in `order` —
    // Obsidian groups by any property, so it joins the columns silently.
    if matches!(view.kind.as_str(), "kanban" | "board") {
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
        let cover = cover_of(row, root, images);
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
                                "file.links" | "file.backlinks" | "links" | "backlinks"
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

    let headers = columns
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
                    "file.mtime" => "Modified".into(),
                    "file.ctime" => "Created".into(),
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

    Computed {
        headers,
        rows,
        view_names: spec.views.iter().map(|v| v.name.clone()).collect(),
        kind: view.kind.clone(),
        group_ix,
        group_prop: view
            .group_by
            .as_ref()
            .map(|g| g.strip_prefix("note.").unwrap_or(g).to_string())
            .filter(|g| !g.is_empty() && !g.starts_with("formula.") && !g.starts_with("file.")),
        prefill: prefill_pairs(spec, view),
        error,
    }
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
    /// Calendar view: months offset from the current month.
    cal_offset: i32,
    /// Interactive header sort: (column index, descending).
    sort: Option<(usize, bool)>,
    notes_epoch: u64,
    doc_epoch: u64,
    cache_key: Option<(u64, u64, usize)>,
    cached: Option<std::rc::Rc<Computed>>,
    _subscriptions: Vec<Subscription>,
}

impl BaseView {
    pub fn new(
        doc: Entity<Document>,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
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
        view._subscriptions.push(doc_sub);
        view
    }

    /// Inline embed — spec text fixed at creation; the parent Document
    /// swaps in a fresh view when the fence's contents change.
    pub fn for_inline(
        spec: String,
        vault: Entity<Vault>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::init(SpecSrc::Inline(spec), vault, workspace, window, cx)
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
        Self {
            spec_src,
            workspace,
            vault,
            view_ix: 0,
            cal_offset: 0,
            sort: None,
            notes_epoch: 0,
            doc_epoch: 0,
            cache_key: None,
            cached: None,
            _subscriptions: vec![vault_sub],
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
        let view = &spec.views[self.view_ix.min(spec.views.len() - 1)];
        let (notes, root, images) = {
            let vault = self.vault.read(cx);
            (
                vault.notes.clone(),
                vault.root.clone().unwrap_or_default(),
                vault.images.clone(),
            )
        };
        // The base file itself never belongs in its own result set.
        let notes: Vec<_> = notes
            .into_iter()
            .filter(|n| Some(n) != doc_path.as_ref())
            .collect();
        let computed = std::rc::Rc::new(compute(&spec, view, &notes, &root, &images.borrow()));
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
        for row in &computed.rows {
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
                let mut cell = div().flex_1().min_w_0().h(px(84.)).p_1().v_flex().gap_0p5();
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
                            .on_click(move |_, window, cx| {
                                let path = path.clone();
                                let _ = workspace
                                    .update(cx, |ws, cx| ws.open_document_pub(path, window, cx));
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

        // Toolbar row: view switcher on the left, "new note" on the
        // right. Views only appear when the spec declares >1.
        let mut tabs = h_flex().gap_1();
        if computed.view_names.len() > 1 {
            for (ix, name) in computed.view_names.iter().enumerate() {
                let selected = ix == self.view_ix;
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
                        .child(name.clone())
                        .on_click({
                            let this = this.clone();
                            move |_, _window, cx| {
                                this.update(cx, |view, cx| {
                                    view.view_ix = ix;
                                    view.sort = None;
                                    cx.notify();
                                });
                            }
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
            .child(tabs)
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
            );

        let cards = matches!(computed.kind.as_str(), "cards" | "gallery");
        let kanban = matches!(computed.kind.as_str(), "kanban" | "board");
        let calendar = computed.kind == "calendar";
        let header = h_flex()
            .w_full()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .children(computed.headers.iter().enumerate().map(|(ix, h)| {
                let this = this.clone();
                let sorted = self.sort.filter(|(c, _)| *c == ix);
                div()
                    .id(("base-h", ix))
                    .when(ix == 0, |d| d.flex_1())
                    .when(ix > 0, |d| d.w(px(140.)).flex_none())
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
                    .on_click(move |_, _window, cx| {
                        // none → asc → desc → none
                        this.update(cx, |view, cx| {
                            view.sort = match view.sort {
                                Some((c, false)) if c == ix => Some((ix, true)),
                                Some((c, true)) if c == ix => None,
                                _ => Some((ix, false)),
                            };
                            cx.notify();
                        });
                    })
            }));

        let mut rows = v_flex().w_full();
        if calendar {
            rows = rows.child(self.render_calendar(&this, &computed, cx));
        } else if kanban {
            // Group rows on the resolved column's display value.
            let gix = computed.group_ix.unwrap_or(usize::MAX);
            let mut groups: Vec<(String, Vec<&Row>)> = Vec::new();
            for row in &computed.rows {
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
                        .on_click(move |_, window, cx| {
                            let path = path.clone();
                            let _ = workspace
                                .update(cx, |ws, cx| ws.open_document_pub(path, window, cx));
                        });
                    if let Some(cover) = &row.cover {
                        let source: gpui_kit::ImageSource = cover
                            .strip_prefix("file://")
                            .map(|p| std::path::PathBuf::from(p).into())
                            .unwrap_or_else(|| cover.clone().into());
                        card = card.child(
                            img(source)
                                .w_full()
                                .h(px(64.))
                                .rounded(theme.radius)
                                .object_fit(ObjectFit::Cover),
                        );
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
            let mut grid = div().flex().flex_wrap().gap_3().p_3();
            for (ix, row) in computed.rows.iter().enumerate() {
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
                    .on_click(move |_, window, cx| {
                        let path = path.clone();
                        let _ =
                            workspace.update(cx, |ws, cx| ws.open_document_pub(path, window, cx));
                    });
                if let Some(cover) = &row.cover {
                    let source: gpui_kit::ImageSource = cover
                        .strip_prefix("file://")
                        .map(|p| std::path::PathBuf::from(p).into())
                        .unwrap_or_else(|| cover.clone().into());
                    card = card.child(
                        img(source)
                            .w_full()
                            .h(px(110.))
                            .object_fit(ObjectFit::Cover),
                    );
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
                        body = body.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .truncate()
                                .child(format!(
                                    "{}: {}",
                                    computed.headers.get(cix).cloned().unwrap_or_default(),
                                    cell.text
                                )),
                        );
                    }
                }
                grid = grid.child(card.child(body));
            }
            rows = rows.child(grid);
        } else {
            // Interactive header sort orders rows at render time —
            // `Computed` stays cached; comparisons use the raw `Lit`.
            let mut order: Vec<usize> = (0..computed.rows.len()).collect();
            if let Some((cix, desc)) = self.sort {
                order.sort_by(|a, b| {
                    let ord = lit_cmp(
                        &computed.rows[*a].cells[cix].lit,
                        &computed.rows[*b].cells[cix].lit,
                    );
                    if desc {
                        ord.reverse()
                    } else {
                        ord
                    }
                });
            }
            for (ix, &rix) in order.iter().enumerate() {
                let row = &computed.rows[rix];
                let path = row.path.clone();
                let workspace = self.workspace.clone();
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
                                    div()
                                        .id(("base-cell-link", ix * 4096 + cix))
                                        .when(cix == 0, |d| d.flex_1())
                                        .when(cix > 0, |d| d.w(px(140.)).flex_none())
                                        .text_sm()
                                        .truncate()
                                        .text_color(theme.accent)
                                        .cursor_pointer()
                                        .on_click(move |_, window, cx| {
                                            cx.stop_propagation();
                                            let target = target.clone();
                                            let _ = workspace.update(cx, |ws, cx| {
                                                ws.open_document_pub(target, window, cx)
                                            });
                                        })
                                        .child(cell.text.clone())
                                        .into_any_element()
                                } else {
                                    div()
                                        .when(cix == 0, |d| d.flex_1())
                                        .when(cix > 0, |d| d.w(px(140.)).flex_none())
                                        .text_sm()
                                        .truncate()
                                        .text_color(theme.foreground)
                                        .child(cell.text.clone())
                                        .into_any_element()
                                }
                            },
                        )))
                        .on_click(move |_, window, cx| {
                            let path = path.clone();
                            let _ = workspace
                                .update(cx, |ws, cx| ws.open_document_pub(path, window, cx));
                        }),
                );
            }
        }
        if computed.rows.is_empty() && computed.error.is_none() {
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
            .child(toolbar)
            .when(!cards && !kanban && !calendar, |v| v.child(header))
            .children(computed.error.iter().map(|e| {
                div()
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(e.clone())
            }))
            .child(
                gpui_kit::component::scroll::ScrollableElement::overflow_y_scrollbar(
                    rows.flex_1().min_h_0(),
                ),
            )
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
                            computed.rows.len(),
                            if computed.rows.len() == 1 {
                                "note"
                            } else {
                                "notes"
                            },
                            undated
                        )
                    } else {
                        format!(
                            "{} {}",
                            computed.rows.len(),
                            if computed.rows.len() == 1 {
                                "note"
                            } else {
                                "notes"
                            }
                        )
                    }),
            )
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
        let mut env = Env {
            row: &rows[0],
            rows: &rows,
            formulas: &formulas,
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
    fn filter_tree() {
        let (rows, formulas) = env_with(&[("status", Lit::Str("draft".into()))]);
        let no_resolve = |_: &str| None;
        let mut env = Env {
            row: &rows[0],
            rows: &rows,
            formulas: &formulas,
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
}
