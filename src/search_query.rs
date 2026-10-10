//! Query parsing and evaluation for vault search.
//!
//! The parser is deliberately small and bounded.  It keeps file-system and
//! GPUI concerns out of the query language so the same evaluator can be used
//! by the search dialog and preview query blocks.

use markdown::mdast::Node;
use regex::{Regex, RegexBuilder};
use serde_yaml::Value;
use std::path::PathBuf;
use std::time::SystemTime;

const MAX_INPUT_BYTES: usize = 4096;
const MAX_TOKENS: usize = 512;
const MAX_NESTING: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvaluationOptions {
    pub match_case: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Query {
    And(Vec<Query>),
    Or(Vec<Query>),
    Not(Box<Query>),
    Term(Term),
    Scope { kind: Scope, query: Box<Query> },
    TaskState { done: bool, query: Box<Query> },
    Property(PropertyFilter),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Content(String),
    Phrase(String),
    Regex(String),
    Tag(String),
    Path(String),
    File(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryRequirements {
    pub read_content: bool,
    pub parse_properties: bool,
    pub extract_tags: bool,
}

pub fn requirements(query: &Query) -> QueryRequirements {
    match query {
        Query::And(children) | Query::Or(children) => children
            .iter()
            .map(requirements)
            .fold(QueryRequirements::default(), merge_requirements),
        Query::Not(child)
        | Query::Scope { query: child, .. }
        | Query::TaskState { query: child, .. } => {
            let mut requirements = requirements(child);
            if matches!(query, Query::Scope { .. } | Query::TaskState { .. }) {
                requirements.read_content = true;
            }
            requirements
        }
        Query::Term(Term::Content(_) | Term::Phrase(_) | Term::Regex(_)) => QueryRequirements {
            read_content: true,
            ..QueryRequirements::default()
        },
        Query::Term(Term::Tag(_)) => QueryRequirements {
            read_content: true,
            parse_properties: true,
            extract_tags: true,
        },
        Query::Term(Term::Path(_) | Term::File(_)) => QueryRequirements::default(),
        Query::Property(property) => {
            let mut needs = QueryRequirements {
                read_content: true,
                parse_properties: true,
                ..QueryRequirements::default()
            };
            if let PropertyCondition::Query(query) = &property.condition {
                let nested = requirements(query);
                needs.extract_tags |= nested.extract_tags;
            }
            needs
        }
    }
}

fn merge_requirements(mut left: QueryRequirements, right: QueryRequirements) -> QueryRequirements {
    left.read_content |= right.read_content;
    left.parse_properties |= right.parse_properties;
    left.extract_tags |= right.extract_tags;
    left
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Line,
    Block,
    Section,
    Task,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyFilter {
    pub key: String,
    pub condition: PropertyCondition,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyCondition {
    Exists,
    Contains(String),
    Null,
    EmptyList,
    Number { operator: Comparison, value: f64 },
    Query(Box<Query>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryErrorCode {
    Empty,
    InputTooLong,
    TooManyTokens,
    TooDeep,
    UnexpectedToken,
    MissingOperand,
    UnclosedQuote,
    UnclosedRegex,
    UnclosedProperty,
    InvalidRegex,
    InvalidProperty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    pub code: QueryErrorCode,
}

impl QueryError {
    fn new(code: QueryErrorCode) -> Self {
        Self { code }
    }

    /// A localized, user-facing explanation for the typed error code.
    pub fn message(&self, norwegian: bool) -> &'static str {
        match (self.code, norwegian) {
            (QueryErrorCode::Empty, false) => "Enter a search query.",
            (QueryErrorCode::Empty, true) => "Skriv inn et søk.",
            (QueryErrorCode::InputTooLong, false) => "The query is too long.",
            (QueryErrorCode::InputTooLong, true) => "Søket er for langt.",
            (QueryErrorCode::TooManyTokens, false) => "The query has too many terms.",
            (QueryErrorCode::TooManyTokens, true) => "Søket har for mange ledd.",
            (QueryErrorCode::TooDeep, false) => "The query is nested too deeply.",
            (QueryErrorCode::TooDeep, true) => "Søket har for mange nestede grupper.",
            (QueryErrorCode::UnexpectedToken, false) => "The query has an unexpected token.",
            (QueryErrorCode::UnexpectedToken, true) => "Søket har et uventet ledd.",
            (QueryErrorCode::MissingOperand, false) => "An operator needs a value.",
            (QueryErrorCode::MissingOperand, true) => "En operator trenger en verdi.",
            (QueryErrorCode::UnclosedQuote, false) => "The quoted phrase is not closed.",
            (QueryErrorCode::UnclosedQuote, true) => "Det mangler avsluttende anførselstegn.",
            (QueryErrorCode::UnclosedRegex, false) => "The regular expression is not closed.",
            (QueryErrorCode::UnclosedRegex, true) => "Det mangler avsluttende regex-tegn.",
            (QueryErrorCode::UnclosedProperty, false) => "The property expression is not closed.",
            (QueryErrorCode::UnclosedProperty, true) => "Det mangler avsluttende hakeparentes.",
            (QueryErrorCode::InvalidRegex, false) => {
                "The regular expression is invalid or uses an unsupported feature."
            }
            (QueryErrorCode::InvalidRegex, true) => {
                "Regex-en er ugyldig eller bruker en funksjon som ikke støttes."
            }
            (QueryErrorCode::InvalidProperty, false) => "The property expression is invalid.",
            (QueryErrorCode::InvalidProperty, true) => "Egenskapsuttrykket er ugyldig.",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchDocument {
    pub path: PathBuf,
    pub relative_path: String,
    pub file_name: String,
    pub content: Option<String>,
    pub properties: Vec<(String, Value)>,
    pub tags: Vec<String>,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

impl SearchDocument {
    pub fn new(
        path: impl Into<PathBuf>,
        relative_path: impl Into<String>,
        content: String,
    ) -> Self {
        let path = path.into();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path,
            relative_path: relative_path.into(),
            file_name,
            content: Some(content),
            properties: Vec::new(),
            tags: Vec::new(),
            modified: None,
            created: None,
        }
    }

    pub fn path_only(path: impl Into<PathBuf>, relative_path: impl Into<String>) -> Self {
        let path = path.into();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            path,
            relative_path: relative_path.into(),
            file_name,
            content: None,
            properties: Vec::new(),
            tags: Vec::new(),
            modified: None,
            created: None,
        }
    }

    pub fn with_metadata(mut self, properties: Vec<(String, Value)>, tags: Vec<String>) -> Self {
        self.properties = properties;
        self.tags = tags;
        self
    }

    pub fn with_times(mut self, modified: Option<SystemTime>, created: Option<SystemTime>) -> Self {
        self.modified = modified;
        self.created = created;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Evaluation {
    pub matched: bool,
    pub occurrences: usize,
    /// The first matching line, using one-based line numbers.
    pub first_line: Option<usize>,
}

impl Evaluation {
    fn matched(occurrences: usize, first_line: Option<usize>) -> Self {
        Self {
            matched: true,
            occurrences,
            first_line,
        }
    }
}

#[derive(Clone)]
pub struct CompiledQuery {
    root: CompiledNode,
}

#[derive(Clone)]
enum CompiledNode {
    And(Vec<CompiledNode>),
    Or(Vec<CompiledNode>),
    Not(Box<CompiledNode>),
    Term(CompiledTerm),
    Scope {
        kind: Scope,
        query: Box<CompiledNode>,
    },
    TaskState {
        done: bool,
        query: Box<CompiledNode>,
    },
    Property(CompiledProperty),
}

#[derive(Clone)]
enum CompiledTerm {
    Content(String),
    Phrase(String),
    Regex(Regex),
    Tag(String),
    Path(String),
    File(String),
}

#[derive(Clone)]
struct CompiledProperty {
    key: String,
    condition: CompiledPropertyCondition,
}

#[derive(Clone)]
enum CompiledPropertyCondition {
    Exists,
    Contains(String),
    Null,
    EmptyList,
    Number { operator: Comparison, value: f64 },
    Query(Box<CompiledNode>),
}

#[derive(Clone, Copy)]
struct Region<'a> {
    text: &'a str,
    first_line: usize,
    done: Option<bool>,
}

#[derive(Clone)]
enum Token<'a> {
    Word(&'a str),
    Phrase(String),
    Regex(String),
    Property(String),
    LeftParen,
    RightParen,
    Minus,
    Or,
}

struct Lexer<'a> {
    input: &'a str,
    offset: usize,
    tokens: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            offset: 0,
            tokens: Vec::new(),
        }
    }

    fn lex(mut self) -> Result<Vec<Token<'a>>, QueryError> {
        while self.offset < self.input.len() {
            let rest = &self.input[self.offset..];
            let Some(first) = rest.chars().next() else {
                break;
            };
            if first.is_whitespace() {
                self.offset += first.len_utf8();
                continue;
            }
            if self.tokens.len() >= MAX_TOKENS {
                return Err(QueryError::new(QueryErrorCode::TooManyTokens));
            }
            match first {
                '(' => {
                    self.offset += 1;
                    self.tokens.push(Token::LeftParen);
                }
                ')' => {
                    self.offset += 1;
                    self.tokens.push(Token::RightParen);
                }
                '-' => {
                    self.offset += 1;
                    self.tokens.push(Token::Minus);
                }
                '"' => self.lex_phrase()?,
                '/' if self.is_value_start() => self.lex_regex()?,
                '[' => self.lex_property()?,
                _ => self.lex_word(),
            }
        }
        Ok(self.tokens)
    }

    fn is_value_start(&self) -> bool {
        self.tokens.is_empty()
            || matches!(
                self.tokens.last(),
                Some(Token::LeftParen | Token::Minus | Token::Or)
            )
            || matches!(
                self.tokens.last(),
                Some(Token::Word(word)) if word.ends_with(':')
            )
    }

    fn lex_phrase(&mut self) -> Result<(), QueryError> {
        self.offset += 1;
        let mut value = String::new();
        let mut escaped = false;
        while self.offset < self.input.len() {
            let ch = self.input[self.offset..]
                .chars()
                .next()
                .expect("offset is on a character boundary");
            self.offset += ch.len_utf8();
            if escaped {
                if matches!(ch, '"' | '\\') {
                    value.push(ch);
                } else {
                    value.push('\\');
                    value.push(ch);
                }
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                self.tokens.push(Token::Phrase(value));
                return Ok(());
            } else {
                value.push(ch);
            }
        }
        Err(QueryError::new(QueryErrorCode::UnclosedQuote))
    }

    fn lex_regex(&mut self) -> Result<(), QueryError> {
        self.offset += 1;
        let mut value = String::new();
        let mut escaped = false;
        while self.offset < self.input.len() {
            let ch = self.input[self.offset..]
                .chars()
                .next()
                .expect("offset is on a character boundary");
            self.offset += ch.len_utf8();
            if escaped {
                if ch == '/' {
                    value.push('/');
                } else {
                    value.push('\\');
                    value.push(ch);
                }
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '/' {
                self.tokens.push(Token::Regex(value));
                return Ok(());
            } else {
                value.push(ch);
            }
        }
        Err(QueryError::new(QueryErrorCode::UnclosedRegex))
    }

    fn lex_property(&mut self) -> Result<(), QueryError> {
        self.offset += 1;
        let start = self.offset;
        let mut escaped = false;
        let mut nested_brackets = 0;
        while self.offset < self.input.len() {
            let ch = self.input[self.offset..]
                .chars()
                .next()
                .expect("offset is on a character boundary");
            self.offset += ch.len_utf8();
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '[' {
                nested_brackets += 1;
            } else if ch == ']' {
                if nested_brackets == 0 {
                    self.tokens.push(Token::Property(
                        self.input[start..self.offset - 1].to_string(),
                    ));
                    return Ok(());
                }
                nested_brackets -= 1;
            }
        }
        Err(QueryError::new(QueryErrorCode::UnclosedProperty))
    }

    fn lex_word(&mut self) {
        let start = self.offset;
        while self.offset < self.input.len() {
            let ch = self.input[self.offset..]
                .chars()
                .next()
                .expect("offset is on a character boundary");
            if ch.is_whitespace() || matches!(ch, '(' | ')' | '"') {
                break;
            }
            if ch == '/' {
                let prefix = &self.input[start..self.offset];
                let is_regex_operator = prefix.split_once(':').is_some_and(|(operator, value)| {
                    value.is_empty()
                        && matches!(
                            operator.to_ascii_lowercase().as_str(),
                            "content"
                                | "line"
                                | "block"
                                | "section"
                                | "task"
                                | "task-todo"
                                | "task-done"
                        )
                });
                if is_regex_operator {
                    break;
                }
            }
            self.offset += ch.len_utf8();
        }
        let word = &self.input[start..self.offset];
        if word == "OR" {
            self.tokens.push(Token::Or);
        } else {
            self.tokens.push(Token::Word(word));
        }
    }
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    index: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn parse(input: &'a str, depth: usize) -> Result<Query, QueryError> {
        if input.trim().is_empty() {
            return Err(QueryError::new(QueryErrorCode::Empty));
        }
        if input.len() > MAX_INPUT_BYTES {
            return Err(QueryError::new(QueryErrorCode::InputTooLong));
        }
        if depth > MAX_NESTING {
            return Err(QueryError::new(QueryErrorCode::TooDeep));
        }
        let tokens = Lexer::new(input).lex()?;
        let mut parser = Self {
            tokens,
            index: 0,
            depth,
        };
        let query = parser.parse_or()?;
        if parser.index != parser.tokens.len() {
            return Err(QueryError::new(QueryErrorCode::UnexpectedToken));
        }
        Ok(query)
    }

    fn parse_or(&mut self) -> Result<Query, QueryError> {
        let mut children = vec![self.parse_and()?];
        while self.take_or() {
            children.push(self.parse_and()?);
        }
        if children.len() == 1 {
            Ok(children.remove(0))
        } else {
            Ok(Query::Or(children))
        }
    }

    fn parse_and(&mut self) -> Result<Query, QueryError> {
        let mut children = Vec::new();
        while self.index < self.tokens.len()
            && !matches!(self.tokens[self.index], Token::RightParen | Token::Or)
        {
            children.push(self.parse_unary()?);
        }
        if children.is_empty() {
            return Err(QueryError::new(QueryErrorCode::MissingOperand));
        }
        if children.len() == 1 {
            Ok(children.remove(0))
        } else {
            Ok(Query::And(children))
        }
    }

    fn parse_unary(&mut self) -> Result<Query, QueryError> {
        if self.take_minus() {
            if matches!(self.tokens.get(self.index), Some(Token::LeftParen)) {
                self.index += 1;
                if self.depth >= MAX_NESTING {
                    return Err(QueryError::new(QueryErrorCode::TooDeep));
                }
                let mut query = self.with_depth(|parser| parser.parse_or())?;
                if !matches!(self.tokens.get(self.index), Some(Token::RightParen)) {
                    return Err(QueryError::new(QueryErrorCode::UnexpectedToken));
                }
                self.index += 1;
                if let Query::And(children) = query {
                    query = Query::Or(children);
                }
                return Ok(Query::Not(Box::new(query)));
            }
            return Ok(Query::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Query, QueryError> {
        let token = self
            .tokens
            .get(self.index)
            .ok_or_else(|| QueryError::new(QueryErrorCode::MissingOperand))?;
        match token {
            Token::LeftParen => {
                self.index += 1;
                if self.depth >= MAX_NESTING {
                    return Err(QueryError::new(QueryErrorCode::TooDeep));
                }
                let query = self.with_depth(|parser| parser.parse_or())?;
                if !matches!(self.tokens.get(self.index), Some(Token::RightParen)) {
                    return Err(QueryError::new(QueryErrorCode::UnexpectedToken));
                }
                self.index += 1;
                Ok(query)
            }
            Token::RightParen | Token::Or => Err(QueryError::new(QueryErrorCode::UnexpectedToken)),
            Token::Minus => Err(QueryError::new(QueryErrorCode::MissingOperand)),
            Token::Property(raw) => {
                self.index += 1;
                Ok(Query::Property(parse_property(raw, self.depth)?))
            }
            Token::Phrase(value) => {
                self.index += 1;
                Ok(Query::Term(Term::Phrase(value.clone())))
            }
            Token::Regex(value) => {
                self.index += 1;
                Ok(Query::Term(Term::Regex(value.clone())))
            }
            Token::Word(word) => {
                self.index += 1;
                self.parse_word(word)
            }
        }
    }

    fn parse_word(&mut self, word: &str) -> Result<Query, QueryError> {
        if matches!(
            word.to_ascii_lowercase().as_str(),
            "task-todo" | "task-done"
        ) {
            return Ok(Query::TaskState {
                done: word.eq_ignore_ascii_case("task-done"),
                query: Box::new(Query::Term(Term::Content(String::new()))),
            });
        }
        if let Some((operator, value)) = word.split_once(':') {
            if !value.is_empty() {
                return self.operator(operator, TokenValue::Word(value.to_string()));
            }
            let value = self.take_value()?;
            if matches!(value, TokenValue::Group(_)) {
                return self.operator(operator, value);
            }
            return self.operator(operator, value);
        }
        if let Some(tag) = word.strip_prefix('#') {
            if !tag.is_empty() {
                return Ok(Query::Term(Term::Tag(tag.to_string())));
            }
        }
        Ok(Query::Term(Term::Content(word.to_string())))
    }

    fn operator(&mut self, operator: &str, value: TokenValue) -> Result<Query, QueryError> {
        let operator = operator.to_ascii_lowercase();
        match operator.as_str() {
            "tag" => Ok(Query::Term(Term::Tag(value.into_string()?))),
            "path" => Ok(Query::Term(Term::Path(value.into_string()?))),
            "file" => Ok(Query::Term(Term::File(value.into_string()?))),
            "content" => match value {
                TokenValue::Regex(pattern) => Ok(Query::Term(Term::Regex(pattern))),
                TokenValue::Phrase(value) => Ok(Query::Term(Term::Phrase(value))),
                TokenValue::Word(value) => Ok(Query::Term(Term::Content(value))),
                TokenValue::Group(query) => Ok(query),
            },
            "line" => Ok(Query::Scope {
                kind: Scope::Line,
                query: Box::new(value.into_query()?),
            }),
            "block" => Ok(Query::Scope {
                kind: Scope::Block,
                query: Box::new(value.into_query()?),
            }),
            "section" => Ok(Query::Scope {
                kind: Scope::Section,
                query: Box::new(value.into_query()?),
            }),
            "task" => Ok(Query::Scope {
                kind: Scope::Task,
                query: Box::new(value.into_query()?),
            }),
            "task-todo" | "task-done" => Ok(Query::TaskState {
                done: operator == "task-done",
                query: Box::new(value.into_query()?),
            }),
            _ => {
                let mut literal = operator;
                literal.push(':');
                literal.push_str(&value.into_string()?);
                Ok(Query::Term(Term::Content(literal)))
            }
        }
    }

    fn take_value(&mut self) -> Result<TokenValue, QueryError> {
        let token = self
            .tokens
            .get(self.index)
            .ok_or_else(|| QueryError::new(QueryErrorCode::MissingOperand))?;
        match token {
            Token::LeftParen => {
                self.index += 1;
                if self.depth >= MAX_NESTING {
                    return Err(QueryError::new(QueryErrorCode::TooDeep));
                }
                let query = self.with_depth(|parser| parser.parse_or())?;
                if !matches!(self.tokens.get(self.index), Some(Token::RightParen)) {
                    return Err(QueryError::new(QueryErrorCode::UnexpectedToken));
                }
                self.index += 1;
                Ok(TokenValue::Group(query))
            }
            Token::Word(value) => {
                self.index += 1;
                Ok(TokenValue::Word(value.to_string()))
            }
            Token::Phrase(value) => {
                self.index += 1;
                Ok(TokenValue::Phrase(value.clone()))
            }
            Token::Regex(value) => {
                self.index += 1;
                Ok(TokenValue::Regex(value.clone()))
            }
            Token::Property(_) | Token::RightParen | Token::Or | Token::Minus => {
                Err(QueryError::new(QueryErrorCode::MissingOperand))
            }
        }
    }

    fn with_depth<T>(
        &mut self,
        parser: impl FnOnce(&mut Parser<'a>) -> Result<T, QueryError>,
    ) -> Result<T, QueryError> {
        self.depth += 1;
        let result = parser(self);
        self.depth = self.depth.saturating_sub(1);
        result
    }

    fn take_minus(&mut self) -> bool {
        if matches!(self.tokens.get(self.index), Some(Token::Minus)) {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn take_or(&mut self) -> bool {
        if matches!(self.tokens.get(self.index), Some(Token::Or)) {
            self.index += 1;
            true
        } else {
            false
        }
    }
}

enum TokenValue {
    Word(String),
    Phrase(String),
    Regex(String),
    Group(Query),
}

impl TokenValue {
    fn into_string(self) -> Result<String, QueryError> {
        match self {
            Self::Word(value) | Self::Phrase(value) | Self::Regex(value) => Ok(value),
            Self::Group(_) => Err(QueryError::new(QueryErrorCode::MissingOperand)),
        }
    }

    fn into_query(self) -> Result<Query, QueryError> {
        match self {
            Self::Group(query) => Ok(query),
            Self::Word(value) => Ok(Query::Term(Term::Content(value))),
            Self::Phrase(value) => Ok(Query::Term(Term::Phrase(value))),
            Self::Regex(value) => Ok(Query::Term(Term::Regex(value))),
        }
    }
}

fn parse_property(raw: &str, depth: usize) -> Result<PropertyFilter, QueryError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(QueryError::new(QueryErrorCode::InvalidProperty));
    }
    let (key, condition) = if let Some((key, value)) = raw.split_once(':') {
        (key.trim(), parse_property_value(value.trim(), depth)?)
    } else if let Some((key, operator, value)) = split_comparison(raw) {
        let number = value
            .trim()
            .parse::<f64>()
            .map_err(|_| QueryError::new(QueryErrorCode::InvalidProperty))?;
        (
            key.trim(),
            PropertyCondition::Number {
                operator,
                value: number,
            },
        )
    } else {
        (raw, PropertyCondition::Exists)
    };
    if key.is_empty() {
        return Err(QueryError::new(QueryErrorCode::InvalidProperty));
    }
    Ok(PropertyFilter {
        key: key.to_string(),
        condition,
    })
}

fn split_comparison(raw: &str) -> Option<(&str, Comparison, &str)> {
    for (needle, operator) in [
        (">=", Comparison::GreaterOrEqual),
        ("<=", Comparison::LessOrEqual),
        (">", Comparison::Greater),
        ("<", Comparison::Less),
    ] {
        if let Some(index) = raw.find(needle) {
            return Some((&raw[..index], operator, &raw[index + needle.len()..]));
        }
    }
    None
}

fn parse_property_value(raw: &str, depth: usize) -> Result<PropertyCondition, QueryError> {
    if raw.is_empty() {
        return Err(QueryError::new(QueryErrorCode::InvalidProperty));
    }
    if raw.eq_ignore_ascii_case("null") {
        return Ok(PropertyCondition::Null);
    }
    if raw == "[]" {
        return Ok(PropertyCondition::EmptyList);
    }
    if raw.starts_with('(') && raw.ends_with(')') {
        if depth >= MAX_NESTING {
            return Err(QueryError::new(QueryErrorCode::TooDeep));
        }
        return Ok(PropertyCondition::Query(Box::new(Parser::parse(
            &raw[1..raw.len() - 1],
            depth + 1,
        )?)));
    }
    let value = if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let body = &raw[1..raw.len() - 1];
        unescape_phrase(body)
    } else {
        raw.to_string()
    };
    Ok(PropertyCondition::Contains(value))
}

fn unescape_phrase(raw: &str) -> String {
    let mut value = String::new();
    let mut escaped = false;
    for ch in raw.chars() {
        if escaped {
            if matches!(ch, '"' | '\\') {
                value.push(ch);
            } else {
                value.push('\\');
                value.push(ch);
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            value.push(ch);
        }
    }
    if escaped {
        value.push('\\');
    }
    value
}

pub fn parse_query(input: &str) -> Result<Query, QueryError> {
    let query = Parser::parse(input, 0)?;
    compile_query(&query, EvaluationOptions::default())?;
    Ok(query)
}

pub fn compile_query(
    query: &Query,
    options: EvaluationOptions,
) -> Result<CompiledQuery, QueryError> {
    Ok(CompiledQuery {
        root: compile_node(query, options)?,
    })
}

fn compile_node(query: &Query, options: EvaluationOptions) -> Result<CompiledNode, QueryError> {
    Ok(match query {
        Query::And(children) => CompiledNode::And(
            children
                .iter()
                .map(|child| compile_node(child, options))
                .collect::<Result<_, _>>()?,
        ),
        Query::Or(children) => CompiledNode::Or(
            children
                .iter()
                .map(|child| compile_node(child, options))
                .collect::<Result<_, _>>()?,
        ),
        Query::Not(child) => CompiledNode::Not(Box::new(compile_node(child, options)?)),
        Query::Term(term) => CompiledNode::Term(match term {
            Term::Content(value) => CompiledTerm::Content(value.clone()),
            Term::Phrase(value) => CompiledTerm::Phrase(value.clone()),
            Term::Regex(pattern) => {
                let regex = RegexBuilder::new(pattern)
                    .case_insensitive(!options.match_case)
                    .build()
                    .map_err(|_| QueryError::new(QueryErrorCode::InvalidRegex))?;
                CompiledTerm::Regex(regex)
            }
            Term::Tag(value) => CompiledTerm::Tag(value.clone()),
            Term::Path(value) => CompiledTerm::Path(value.clone()),
            Term::File(value) => CompiledTerm::File(value.clone()),
        }),
        Query::Scope { kind, query } => CompiledNode::Scope {
            kind: *kind,
            query: Box::new(compile_node(query, options)?),
        },
        Query::TaskState { done, query } => CompiledNode::TaskState {
            done: *done,
            query: Box::new(compile_node(query, options)?),
        },
        Query::Property(filter) => CompiledNode::Property(CompiledProperty {
            key: filter.key.clone(),
            condition: match &filter.condition {
                PropertyCondition::Exists => CompiledPropertyCondition::Exists,
                PropertyCondition::Contains(value) => {
                    CompiledPropertyCondition::Contains(value.clone())
                }
                PropertyCondition::Null => CompiledPropertyCondition::Null,
                PropertyCondition::EmptyList => CompiledPropertyCondition::EmptyList,
                PropertyCondition::Number { operator, value } => {
                    CompiledPropertyCondition::Number {
                        operator: *operator,
                        value: *value,
                    }
                }
                PropertyCondition::Query(query) => {
                    CompiledPropertyCondition::Query(Box::new(compile_node(query, options)?))
                }
            },
        }),
    })
}

pub fn evaluate(
    query: &Query,
    document: &SearchDocument,
    options: EvaluationOptions,
) -> Result<Evaluation, QueryError> {
    let compiled = compile_query(query, options)?;
    Ok(evaluate_compiled(&compiled, document, options))
}

pub fn evaluate_compiled(
    query: &CompiledQuery,
    document: &SearchDocument,
    options: EvaluationOptions,
) -> Evaluation {
    evaluate_node(
        &query.root,
        document,
        document.content.as_deref(),
        1,
        options,
    )
}

fn evaluate_node(
    query: &CompiledNode,
    document: &SearchDocument,
    text: Option<&str>,
    first_line: usize,
    options: EvaluationOptions,
) -> Evaluation {
    match query {
        CompiledNode::And(children) => {
            let mut result = Evaluation::default();
            for child in children {
                let current = evaluate_node(child, document, text, first_line, options);
                if !current.matched {
                    return Evaluation::default();
                }
                result.occurrences += current.occurrences;
                result.first_line = min_line(result.first_line, current.first_line);
            }
            result.matched = true;
            result
        }
        CompiledNode::Or(children) => {
            let mut result = Evaluation::default();
            for child in children {
                let current = evaluate_node(child, document, text, first_line, options);
                if current.matched && (!result.matched || current.occurrences > result.occurrences)
                {
                    result = current;
                }
            }
            result
        }
        CompiledNode::Not(child) => {
            let current = evaluate_node(child, document, text, first_line, options);
            Evaluation {
                matched: !current.matched,
                ..Evaluation::default()
            }
        }
        CompiledNode::Term(term) => evaluate_term(term, document, text, first_line, options),
        CompiledNode::Scope { kind, query } => {
            let regions = match kind {
                Scope::Line => line_regions(text.unwrap_or_default(), first_line),
                Scope::Block => block_regions(text.unwrap_or_default(), first_line),
                Scope::Section => section_regions(text.unwrap_or_default(), first_line),
                Scope::Task => task_regions(text.unwrap_or_default(), first_line),
            };
            let mut result = Evaluation::default();
            for region in regions {
                let current = evaluate_node(
                    query,
                    document,
                    Some(region.text),
                    region.first_line,
                    options,
                );
                if current.matched {
                    result.matched = true;
                    result.occurrences += current.occurrences.max(1);
                    result.first_line = min_line(result.first_line, current.first_line);
                }
            }
            result
        }
        CompiledNode::TaskState { done, query } => {
            let mut result = Evaluation::default();
            for region in task_regions(text.unwrap_or_default(), first_line)
                .into_iter()
                .filter(|region| region.done == Some(*done))
            {
                let current = evaluate_node(
                    query,
                    document,
                    Some(region.text),
                    region.first_line,
                    options,
                );
                if current.matched {
                    result.matched = true;
                    result.occurrences += current.occurrences.max(1);
                    result.first_line = min_line(result.first_line, current.first_line);
                }
            }
            result
        }
        CompiledNode::Property(property) => evaluate_property(property, document, options),
    }
}

fn evaluate_term(
    term: &CompiledTerm,
    document: &SearchDocument,
    text: Option<&str>,
    first_line: usize,
    options: EvaluationOptions,
) -> Evaluation {
    match term {
        CompiledTerm::Content(value) => match text {
            Some(text) => text_match(text, value, options.match_case, first_line),
            None => Evaluation::default(),
        },
        CompiledTerm::Phrase(value) => match text {
            Some(text) => text_match(text, value, options.match_case, first_line),
            None => Evaluation::default(),
        },
        CompiledTerm::Regex(regex) => {
            let Some(text) = text else {
                return Evaluation::default();
            };
            let occurrences = regex.find_iter(text).count();
            Evaluation::matched(
                occurrences,
                if occurrences > 0 {
                    regex
                        .find(text)
                        .map(|found| first_line_for_offset(text, found.start(), first_line))
                } else {
                    None
                },
            )
            .with_matched(occurrences > 0)
        }
        CompiledTerm::Tag(value) => {
            let needle = normalize(value, options.match_case);
            let matched = document.tags.iter().any(|tag| {
                let tag = normalize(tag, options.match_case);
                tag == needle || tag.starts_with(&(needle.clone() + "/"))
            });
            Evaluation::matched(1, Some(first_line)).with_matched(matched)
        }
        CompiledTerm::Path(value) => {
            let haystack = normalize(&document.relative_path, options.match_case);
            let needle = normalize(value, options.match_case);
            let matched = haystack.contains(&needle);
            Evaluation::matched(usize::from(matched), None).with_matched(matched)
        }
        CompiledTerm::File(value) => {
            let haystack = normalize(&document.file_name, options.match_case);
            let needle = normalize(value, options.match_case);
            let matched = haystack.contains(&needle);
            Evaluation::matched(usize::from(matched), None).with_matched(matched)
        }
    }
}

trait EvaluationExt {
    fn with_matched(self, matched: bool) -> Self;
}

impl EvaluationExt for Evaluation {
    fn with_matched(mut self, matched: bool) -> Self {
        self.matched = matched;
        if !matched {
            self.occurrences = 0;
            self.first_line = None;
        }
        self
    }
}

fn evaluate_property(
    property: &CompiledProperty,
    document: &SearchDocument,
    options: EvaluationOptions,
) -> Evaluation {
    let Some((_, value)) = document
        .properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(&property.key))
    else {
        return Evaluation::default();
    };
    let matched = match &property.condition {
        CompiledPropertyCondition::Exists => true,
        CompiledPropertyCondition::Null => value.is_null(),
        CompiledPropertyCondition::EmptyList => {
            matches!(value, Value::Sequence(values) if values.is_empty())
        }
        CompiledPropertyCondition::Contains(needle) => {
            property_contains(value, needle, options.match_case)
        }
        CompiledPropertyCondition::Number {
            operator,
            value: needle,
        } => property_number_matches(value, *operator, *needle),
        CompiledPropertyCondition::Query(query) => property_value_matches(value, &|scalar| {
            let mut nested = SearchDocument::new("", "", scalar.to_string());
            nested.content = Some(scalar.to_string());
            evaluate_node(query, &nested, nested.content.as_deref(), 1, options).matched
        }),
    };
    Evaluation::matched(usize::from(matched), Some(1)).with_matched(matched)
}

fn property_value_matches(value: &Value, matcher: &impl Fn(&str) -> bool) -> bool {
    match value {
        Value::Sequence(values) => values.iter().any(|value| matcher(&scalar_value(value))),
        Value::Null => false,
        value => matcher(&scalar_value(value)),
    }
}

fn property_contains(value: &Value, needle: &str, match_case: bool) -> bool {
    match value {
        Value::Sequence(values) => values
            .iter()
            .any(|value| property_contains(value, needle, match_case)),
        Value::Null => false,
        Value::String(value) if needle.is_empty() => value.is_empty(),
        _ if needle.is_empty() => false,
        value => text_match(&scalar_value(value), needle, match_case, 1).matched,
    }
}

fn property_number_matches(value: &Value, operator: Comparison, needle: f64) -> bool {
    let Value::Number(number) = value else {
        return false;
    };
    let Some(number) = number.as_f64() else {
        return false;
    };
    match operator {
        Comparison::Less => number < needle,
        Comparison::LessOrEqual => number <= needle,
        Comparison::Greater => number > needle,
        Comparison::GreaterOrEqual => number >= needle,
    }
}

fn scalar_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => serde_yaml::to_string(other)
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
    }
}

fn text_match(text: &str, needle: &str, match_case: bool, first_line: usize) -> Evaluation {
    let haystack = normalize(text, match_case);
    let needle = normalize(needle, match_case);
    if needle.is_empty() {
        return Evaluation::matched(1, Some(first_line));
    }
    let mut occurrences = 0;
    let mut offset = 0;
    let mut first = None;
    while let Some(position) = haystack[offset..].find(&needle) {
        occurrences += 1;
        let at = offset + position;
        let matching_line =
            first_line + haystack[..at].bytes().filter(|byte| *byte == b'\n').count();
        first.get_or_insert(matching_line);
        offset = at + needle.len();
        if offset >= haystack.len() {
            break;
        }
    }
    Evaluation::matched(occurrences, first).with_matched(occurrences > 0)
}

fn normalize(value: &str, match_case: bool) -> String {
    if match_case {
        value.to_string()
    } else {
        value.to_lowercase()
    }
}

fn first_line_for_offset(text: &str, offset: usize, first_line: usize) -> usize {
    first_line
        + text[..offset.min(text.len())]
            .bytes()
            .filter(|b| *b == b'\n')
            .count()
}

fn min_line(first: Option<usize>, second: Option<usize>) -> Option<usize> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (first, second) => first.or(second),
    }
}

fn line_regions(text: &str, first_line: usize) -> Vec<Region<'_>> {
    if text.is_empty() {
        return vec![Region {
            text,
            first_line,
            done: None,
        }];
    }
    text.split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| Region {
            text: line.trim_end_matches('\n'),
            first_line: first_line + index,
            done: None,
        })
        .collect()
}

fn block_regions(text: &str, first_line: usize) -> Vec<Region<'_>> {
    let Some(root) = markdown_root(text) else {
        return line_regions(text, first_line);
    };
    let mut ranges = Vec::new();
    collect_block_ranges(&root, &mut ranges);
    if ranges.is_empty() {
        return line_regions(text, first_line);
    }
    ranges
        .into_iter()
        .filter_map(|range| region_from_range(text, range, first_line))
        .collect()
}

fn collect_block_ranges(node: &Node, ranges: &mut Vec<std::ops::Range<usize>>) {
    match node {
        Node::Root(root) => {
            for child in &root.children {
                collect_block_ranges(child, ranges);
            }
        }
        Node::List(list) => {
            for child in &list.children {
                collect_block_ranges(child, ranges);
            }
        }
        Node::ListItem(item) => {
            if let Some(position) = &item.position {
                ranges.push(position.start.offset..position.end.offset);
            }
        }
        Node::Blockquote(quote) => {
            for child in &quote.children {
                collect_block_ranges(child, ranges);
            }
        }
        Node::Paragraph(_)
        | Node::Heading(_)
        | Node::Code(_)
        | Node::Table(_)
        | Node::ThematicBreak(_)
        | Node::Html(_) => {
            if let Some(position) = node.position() {
                ranges.push(position.start.offset..position.end.offset);
            }
        }
        _ => {}
    }
}

fn section_regions(text: &str, first_line: usize) -> Vec<Region<'_>> {
    let Some(root) = markdown_root(text) else {
        return vec![Region {
            text,
            first_line,
            done: None,
        }];
    };
    let Some(children) = root.children() else {
        return vec![Region {
            text,
            first_line,
            done: None,
        }];
    };
    let mut headings = Vec::new();
    for child in children {
        if let Node::Heading(heading) = child {
            if let Some(position) = &heading.position {
                headings.push((
                    position.start.offset,
                    position.end.offset,
                    heading.depth as usize,
                ));
            }
        }
    }
    if headings.is_empty() {
        return vec![Region {
            text,
            first_line,
            done: None,
        }];
    }
    let mut ranges = Vec::new();
    if headings[0].0 > 0 {
        ranges.push(0..headings[0].0);
    }
    for (index, heading) in headings.iter().enumerate() {
        let end = headings
            .iter()
            .skip(index + 1)
            .find(|next| next.2 <= heading.2)
            .map(|next| next.0)
            .unwrap_or(text.len());
        ranges.push(heading.0..end);
    }
    ranges
        .into_iter()
        .filter_map(|range| region_from_range(text, range, first_line))
        .collect()
}

fn task_regions(text: &str, first_line: usize) -> Vec<Region<'_>> {
    let Some(root) = markdown_root(text) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    collect_task_ranges(&root, &mut ranges);
    ranges
        .into_iter()
        .filter_map(|(range, done)| {
            region_from_range(text, range, first_line).map(|mut region| {
                region.done = Some(done);
                region
            })
        })
        .collect()
}

fn collect_task_ranges(node: &Node, ranges: &mut Vec<(std::ops::Range<usize>, bool)>) {
    if let Node::ListItem(item) = node {
        if item.checked.is_some() {
            if let Some(position) = &item.position {
                ranges.push((
                    position.start.offset..position.end.offset,
                    item.checked == Some(true),
                ));
            }
        }
    }
    for child in node.children().into_iter().flatten() {
        collect_task_ranges(child, ranges);
    }
}

fn region_from_range(
    text: &str,
    range: std::ops::Range<usize>,
    first_line: usize,
) -> Option<Region<'_>> {
    let start = clamp_boundary(text, range.start.min(text.len()));
    let end = clamp_boundary(text, range.end.min(text.len()));
    if start >= end {
        return None;
    }
    Some(Region {
        text: &text[start..end],
        first_line: first_line + text[..start].bytes().filter(|b| *b == b'\n').count(),
        done: None,
    })
}

fn clamp_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset;
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn markdown_root(text: &str) -> Option<Node> {
    let mut options = markdown::ParseOptions::gfm();
    options.constructs.frontmatter = true;
    markdown::to_mdast(text, &options).ok()
}

/// Extract inline and frontmatter tags while ignoring code spans and fences.
pub fn extract_tags(text: &str, properties: &[(String, Value)]) -> Vec<String> {
    let mut tags = std::collections::BTreeSet::new();
    if let Some((_, value)) = properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("tags"))
    {
        match value {
            Value::Sequence(values) => {
                for value in values {
                    let tag = scalar_value(value);
                    if !tag.is_empty() {
                        tags.insert(tag.trim_start_matches('#').to_string());
                    }
                }
            }
            value => {
                for tag in scalar_value(value).split(',') {
                    let tag = tag.trim().trim_start_matches('#');
                    if !tag.is_empty() {
                        tags.insert(tag.to_string());
                    }
                }
            }
        }
    }
    let Some(root) = markdown_root(text) else {
        return tags.into_iter().collect();
    };
    let mut fragments = Vec::new();
    collect_tag_fragments(&root, &mut fragments);
    for fragment in fragments {
        for tag in inline_tags(fragment) {
            tags.insert(tag);
        }
    }
    tags.into_iter().collect()
}

fn collect_tag_fragments<'a>(node: &'a Node, fragments: &mut Vec<&'a str>) {
    match node {
        Node::Text(text) => fragments.push(text.value.as_str()),
        Node::InlineCode(_) | Node::Code(_) => {}
        _ => {
            for child in node.children().into_iter().flatten() {
                collect_tag_fragments(child, fragments);
            }
        }
    }
}

fn inline_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut offset = 0;
    while let Some(position) = text[offset..].find('#') {
        let start = offset + position;
        let previous = text[..start].chars().next_back();
        if previous.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '\\') {
            offset = start + 1;
            continue;
        }
        let end = text[start + 1..]
            .char_indices()
            .find(|(_, ch)| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '/')))
            .map(|(index, _)| start + 1 + index)
            .unwrap_or(text.len());
        if end > start + 1 {
            tags.push(text[start + 1..end].to_string());
        }
        offset = end.max(start + 1);
    }
    tags
}

pub fn explain(query: &Query, norwegian: bool) -> String {
    fn render(query: &Query, norwegian: bool) -> String {
        match query {
            Query::And(children) => children
                .iter()
                .map(|child| render(child, norwegian))
                .collect::<Vec<_>>()
                .join(if norwegian { " og " } else { " and " }),
            Query::Or(children) => children
                .iter()
                .map(|child| render(child, norwegian))
                .collect::<Vec<_>>()
                .join(if norwegian { " eller " } else { " or " }),
            Query::Not(child) => format!(
                "{} {}",
                if norwegian { "ikke" } else { "not" },
                render(child, norwegian)
            ),
            Query::Term(term) => match term {
                Term::Content(value) => format!(
                    "{} “{}”",
                    if norwegian { "innhold" } else { "content" },
                    value
                ),
                Term::Phrase(value) => format!(
                    "{} “{}”",
                    if norwegian { "uttrykket" } else { "phrase" },
                    value
                ),
                Term::Regex(_) => if norwegian {
                    "regex-mønster"
                } else {
                    "regular-expression pattern"
                }
                .into(),
                Term::Tag(value) => format!("{} #{value}", if norwegian { "tagg" } else { "tag" }),
                Term::Path(value) => format!("{} {value}", if norwegian { "sti" } else { "path" }),
                Term::File(value) => format!("{} {value}", if norwegian { "fil" } else { "file" }),
            },
            Query::Scope { kind, query } => format!(
                "{} ({})",
                match (kind, norwegian) {
                    (Scope::Line, false) => "same line",
                    (Scope::Line, true) => "samme linje",
                    (Scope::Block, false) => "same block",
                    (Scope::Block, true) => "samme blokk",
                    (Scope::Section, false) => "same section",
                    (Scope::Section, true) => "samme seksjon",
                    (Scope::Task, false) => "same task",
                    (Scope::Task, true) => "samme oppgave",
                },
                render(query, norwegian)
            ),
            Query::TaskState { done, query } => {
                let state = if *done {
                    if norwegian {
                        "ferdig oppgave"
                    } else {
                        "completed task"
                    }
                } else if norwegian {
                    "åpen oppgave"
                } else {
                    "open task"
                };
                if matches!(query.as_ref(), Query::Term(Term::Content(value)) if value.is_empty()) {
                    state.to_string()
                } else {
                    format!("{state} ({})", render(query, norwegian))
                }
            }
            Query::Property(property) => {
                let condition = match &property.condition {
                    PropertyCondition::Exists => {
                        if norwegian {
                            "er satt".to_string()
                        } else {
                            "is set".to_string()
                        }
                    }
                    PropertyCondition::Contains(value) => {
                        if norwegian {
                            format!("inneholder “{value}”")
                        } else {
                            format!("contains “{value}”")
                        }
                    }
                    PropertyCondition::Null => {
                        if norwegian {
                            "er null".to_string()
                        } else {
                            "is null".to_string()
                        }
                    }
                    PropertyCondition::EmptyList => {
                        if norwegian {
                            "er en tom liste".to_string()
                        } else {
                            "is an empty list".to_string()
                        }
                    }
                    PropertyCondition::Number { operator, value } => {
                        let operator = match operator {
                            Comparison::Less => "<",
                            Comparison::LessOrEqual => "<=",
                            Comparison::Greater => ">",
                            Comparison::GreaterOrEqual => ">=",
                        };
                        format!("{operator} {value}")
                    }
                    PropertyCondition::Query(query) => {
                        if norwegian {
                            format!("matcher ({})", render(query, norwegian))
                        } else {
                            format!("matches ({})", render(query, norwegian))
                        }
                    }
                };
                format!(
                    "{} {} {condition}",
                    if norwegian { "egenskap" } else { "property" },
                    property.key
                )
            }
        }
    }
    render(query, norwegian)
}

pub fn query_fences(source: &str) -> Vec<(usize, String)> {
    let Some(root) = markdown_root(source) else {
        return Vec::new();
    };
    let mut fences = Vec::new();
    collect_query_fences(&root, &mut fences);
    fences
}

fn collect_query_fences(node: &Node, fences: &mut Vec<(usize, String)>) {
    if let Node::Code(code) = node {
        if code.lang.as_deref() == Some("query") {
            if let Some(position) = &code.position {
                let line = position.start.line;
                fences.push((line, code.value.clone()));
            }
        }
    }
    for child in node.children().into_iter().flatten() {
        collect_query_fences(child, fences);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> SearchDocument {
        SearchDocument::new("note.md", "note.md", text.to_string())
    }

    fn matches(query: &str, text: &str) -> bool {
        evaluate(
            &parse_query(query).expect("query parses"),
            &doc(text),
            EvaluationOptions::default(),
        )
        .expect("query compiles")
        .matched
    }

    #[test]
    fn implicit_and_binds_tighter_than_or() {
        assert!(matches(
            "meeting (work OR meetup) personal",
            "meeting work personal"
        ));
        assert!(matches(
            "meeting (work OR meetup) personal",
            "meeting meetup personal"
        ));
        assert!(!matches(
            "meeting (work OR meetup) personal",
            "meeting work"
        ));
    }

    #[test]
    fn negated_groups_are_supported() {
        assert!(matches("meeting -(work meetup)", "meeting personal"));
        assert!(!matches("meeting -(work meetup)", "meeting work"));
        assert!(!matches("meeting -(work meetup)", "meeting meetup"));
    }

    #[test]
    fn quoted_unicode_and_case_toggle() {
        assert!(matches("\"Møte på kafé\"", "Et Møte på kafé"));
        let query = parse_query("\"Møte på kafé\"").unwrap();
        let document = doc("et møte på kafé");
        assert!(
            evaluate(&query, &document, EvaluationOptions::default())
                .unwrap()
                .matched
        );
        assert!(
            !evaluate(&query, &document, EvaluationOptions { match_case: true })
                .unwrap()
                .matched
        );
        assert!(matches("é", "İé"));
    }

    #[test]
    fn scopes_do_not_scatter_terms_across_regions() {
        assert!(matches("line:(alpha beta)", "alpha beta"));
        assert!(!matches("line:(alpha beta)", "alpha\nbeta"));
        assert!(matches("section:(alpha beta)", "# One\nalpha beta"));
        assert!(!matches(
            "section:(alpha beta)",
            "# One\nalpha\n# Two\nbeta"
        ));
        assert!(matches("block:(alpha beta)", "alpha beta"));
        assert!(!matches("block:(alpha beta)", "alpha\n\nbeta"));
        assert!(matches("task:(alpha beta)", "- [ ] alpha beta"));
        assert!(!matches("task:(alpha beta)", "- [ ] alpha\n- [ ] beta"));
    }

    #[test]
    fn tags_ignore_code_and_support_nested_paths() {
        let text = "`#hidden`\n\n#visible #area/today";
        let properties = Vec::new();
        let mut document = doc(text);
        document.tags = extract_tags(text, &properties);
        assert!(matches_with_doc("tag:visible", &document));
        assert!(matches_with_doc("tag:area", &document));
        assert!(!matches_with_doc("tag:hidden", &document));
    }

    #[test]
    fn tasks_and_properties_distinguish_null_empty_and_numbers() {
        let text = "- [ ] todo\n- [x] done";
        assert!(matches("task-todo:todo", text));
        assert!(matches("task-done:done", text));
        assert!(!matches("task-todo:done", text));
        assert!(matches("task-todo", text));
        assert!(matches("task-done", text));
        assert!(!matches("task-todo", "- [x] done"));

        let mut document = doc("body");
        document.properties = vec![
            ("empty".into(), Value::String(String::new())),
            ("none".into(), Value::Null),
            ("items".into(), Value::Sequence(Vec::new())),
            ("score".into(), Value::Number(serde_yaml::Number::from(7))),
            (
                "labels".into(),
                Value::Sequence(vec![
                    Value::String("work".into()),
                    Value::String("later".into()),
                ]),
            ),
        ];
        assert!(matches_with_doc("[none]", &document));
        assert!(matches_with_doc("[none:null]", &document));
        assert!(matches_with_doc("[items:[]]", &document));
        assert!(matches_with_doc("[items]", &document));
        assert!(!matches_with_doc("[empty:[]]", &document));
        assert!(!matches_with_doc("[missing]", &document));
        assert!(!matches_with_doc("[empty:null]", &document));
        assert!(matches_with_doc("[empty:\"\"]", &document));
        assert!(!matches_with_doc("[empty:\"later\"]", &document));
        assert!(!matches_with_doc("[labels:\"\"]", &document));
        assert!(matches_with_doc("[labels:work]", &document));
        assert!(matches_with_doc("[labels:(work OR later)]", &document));
        assert!(!matches_with_doc("[items:\"\"]", &document));
        assert!(matches_with_doc("[score>=7]", &document));
        assert!(!matches_with_doc("[score<7]", &document));
    }

    #[test]
    fn regex_is_safe_and_invalid_patterns_are_errors() {
        assert!(matches("/m[eé]eting/", "meeting"));
        assert!(matches("line:/meeting/", "meeting"));
        assert!(matches("/a\\/b/", "a/b"));
        assert!(!matches("/missing/", "present"));
        assert_eq!(
            parse_query("/(unclosed/").unwrap_err().code,
            QueryErrorCode::InvalidRegex
        );
        assert_eq!(
            parse_query("/unclosed").unwrap_err().code,
            QueryErrorCode::UnclosedRegex
        );
    }

    #[test]
    fn unbalanced_nesting_is_rejected() {
        assert!(parse_query("(meeting").is_err());
        let deep = "(".repeat(MAX_NESTING + 2) + "x" + &")".repeat(MAX_NESTING + 2);
        assert_eq!(
            parse_query(&deep).unwrap_err().code,
            QueryErrorCode::TooDeep
        );
    }

    #[test]
    fn query_fence_detection_ignores_other_code_blocks() {
        let fences = query_fences("```query\nfile:agenda\n```\n\n```text\nnot a query\n```");
        assert_eq!(fences, vec![(1, "file:agenda".to_string())]);
    }

    #[test]
    fn path_and_filename_terms_do_not_match_every_document() {
        let mut document = doc("unrelated text");
        document.path = "docs/agenda.md".into();
        document.relative_path = "docs/agenda.md".into();
        document.file_name = "agenda.md".into();
        assert!(matches_with_doc("path:docs", &document));
        assert!(matches_with_doc("file:agenda", &document));
        assert!(!matches_with_doc("path:missing", &document));
        assert!(!matches_with_doc("file:missing", &document));
    }

    #[test]
    fn requirements_skip_metadata_for_plain_content_and_path_queries() {
        let content = requirements(&parse_query("meeting").unwrap());
        assert!(content.read_content);
        assert!(!content.parse_properties);
        assert!(!content.extract_tags);

        let path = requirements(&parse_query("path:meeting").unwrap());
        assert!(!path.read_content);
        assert!(!path.parse_properties);
        assert!(!path.extract_tags);

        let tag = requirements(&parse_query("tag:meeting").unwrap());
        assert!(tag.read_content);
        assert!(tag.parse_properties);
        assert!(tag.extract_tags);
    }

    fn matches_with_doc(query: &str, document: &SearchDocument) -> bool {
        evaluate(
            &parse_query(query).expect("query parses"),
            document,
            EvaluationOptions::default(),
        )
        .expect("query compiles")
        .matched
    }
}
