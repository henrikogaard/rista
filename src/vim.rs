//! A small, application-owned Vim command engine.
//!
//! This module deliberately knows nothing about GPUI or editor state.  It
//! consumes a text snapshot and byte offsets and returns explicit effects for
//! the Document adapter to apply through its existing editor APIs.

use regex::RegexBuilder;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, LazyLock, Mutex};
use unicode_segmentation::UnicodeSegmentation;

const MAX_COUNT: usize = 1_000_000;
const MAX_EDIT_BYTES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    Command,
    Search,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Key {
    Text(String),
    Escape,
    Enter,
    Backspace,
    Delete,
    CtrlR,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operator {
    Delete,
    Change,
    Yank,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FindKind {
    Till,
    Through,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InsertKind {
    Before,
    After,
    LineStart,
    LineEnd,
    OpenAbove,
    OpenBelow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineFirstText,
    LineEnd,
    FileStart,
    FileEnd,
    Find(FindKind, char),
    MatchPair,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Object {
    Word {
        around: bool,
    },
    Quote {
        around: bool,
        quote: char,
    },
    Pair {
        around: bool,
        open: char,
        close: char,
    },
    Paragraph,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Recipe {
    Insert {
        kind: InsertKind,
        text: String,
    },
    OpenLine {
        above: bool,
        text: String,
    },
    Change {
        motion: MotionSpec,
        text: String,
    },
    ChangeObject {
        object: Object,
        count: usize,
        text: String,
    },
    DeleteObject {
        object: Object,
        count: usize,
    },
    ChangeLine {
        count: usize,
        text: String,
    },
    DeleteMotion(MotionSpec),
    DeleteLine(usize),
    DeleteChars {
        backwards: bool,
        count: usize,
    },
    Paste {
        value: RegisterValue,
        before: bool,
        count: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MotionSpec {
    motion: Motion,
    count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterValue {
    pub text: String,
    pub linewise: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RegisterBank {
    values: HashMap<char, RegisterValue>,
    unnamed: Option<RegisterValue>,
}

static GLOBAL_REGISTERS: LazyLock<Arc<Mutex<RegisterBank>>> =
    LazyLock::new(|| Arc::new(Mutex::new(RegisterBank::default())));

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StatusMessage {
    Error(&'static str, &'static str),
}

impl StatusMessage {
    pub fn text(&self, norwegian: bool) -> String {
        match self {
            Self::Error(english, norwegian_text) => {
                if norwegian {
                    (*norwegian_text).to_string()
                } else {
                    (*english).to_string()
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    pub range: Range<usize>,
    pub replacement: String,
    pub cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    Move {
        selection: Range<usize>,
        cursor: usize,
    },
    Replace(Edit),
    Undo,
    Redo,
    Save,
    ClipboardWrite(String),
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub handled: bool,
    pub effects: Vec<Effect>,
    pub status: Option<StatusMessage>,
    pub state_changed: bool,
}

impl Outcome {
    fn handled() -> Self {
        Self {
            handled: true,
            state_changed: true,
            ..Default::default()
        }
    }

    fn pass() -> Self {
        Self::default()
    }

    fn with_status(status: StatusMessage) -> Self {
        Self {
            handled: true,
            status: Some(status),
            state_changed: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct VimState {
    mode: Mode,
    pending_operator: Option<(Operator, usize)>,
    pending_text_object: Option<(Operator, bool, usize)>,
    pending_visual_text_object: Option<bool>,
    pending_change_motion: Option<MotionSpec>,
    pending_change_object: Option<(Object, usize)>,
    pending_change_line: Option<usize>,
    pending_find: Option<FindKind>,
    pending_g: bool,
    pending_register: bool,
    count: String,
    register: char,
    visual_anchor: Option<usize>,
    active_cursor: usize,
    expected_selection: Option<Range<usize>>,
    last_cursor: Option<usize>,
    desired_column: Option<usize>,
    command_line: String,
    search_line: String,
    last_search: Option<String>,
    last_search_forward: bool,
    insert: Option<InsertSession>,
    last_change: Option<Recipe>,
    status: Option<StatusMessage>,
    registers: Arc<Mutex<RegisterBank>>,
}

#[derive(Clone, Debug)]
struct InsertSession {
    before: String,
    start: usize,
    kind: InsertKind,
    change_motion: Option<MotionSpec>,
    change_object: Option<(Object, usize)>,
    change_line: Option<usize>,
    record_recipe: bool,
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

impl VimState {
    pub fn new() -> Self {
        Self::with_registers(GLOBAL_REGISTERS.clone())
    }

    pub fn with_registers(registers: Arc<Mutex<RegisterBank>>) -> Self {
        Self {
            mode: Mode::Normal,
            pending_operator: None,
            pending_text_object: None,
            pending_visual_text_object: None,
            pending_change_motion: None,
            pending_change_object: None,
            pending_change_line: None,
            pending_find: None,
            pending_g: false,
            pending_register: false,
            count: String::new(),
            register: '"',
            visual_anchor: None,
            active_cursor: 0,
            expected_selection: None,
            last_cursor: None,
            desired_column: None,
            command_line: String::new(),
            search_line: String::new(),
            last_search: None,
            last_search_forward: true,
            insert: None,
            last_change: None,
            status: None,
            registers,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_insert(&self) -> bool {
        self.mode == Mode::Insert
    }

    pub fn reset(&mut self, cursor: usize) {
        self.mode = Mode::Normal;
        self.pending_operator = None;
        self.pending_text_object = None;
        self.pending_visual_text_object = None;
        self.pending_change_motion = None;
        self.pending_change_object = None;
        self.pending_change_line = None;
        self.pending_find = None;
        self.pending_g = false;
        self.pending_register = false;
        self.count.clear();
        self.register = '"';
        self.visual_anchor = None;
        self.active_cursor = cursor;
        self.expected_selection = None;
        self.last_cursor = Some(cursor);
        self.desired_column = None;
        self.command_line.clear();
        self.search_line.clear();
        self.last_search = None;
        self.last_search_forward = true;
        self.insert = None;
        self.last_change = None;
        self.status = None;
    }

    pub fn invalidate_external_edit(&mut self) {
        if self.mode == Mode::Insert {
            self.last_change = None;
            self.insert = None;
            self.last_search = None;
            self.search_line.clear();
            return;
        }
        self.clear_pending();
        self.last_change = None;
        self.last_search = None;
        self.last_search_forward = true;
        self.command_line.clear();
        self.search_line.clear();
        self.visual_anchor = None;
        self.expected_selection = None;
        self.desired_column = None;
        self.status = None;
        self.mode = Mode::Normal;
    }

    pub fn suspend(&mut self) {
        let insert = self.mode == Mode::Insert;
        self.clear_pending();
        self.command_line.clear();
        self.search_line.clear();
        self.visual_anchor = None;
        self.expected_selection = None;
        self.desired_column = None;
        self.status = None;
        if insert {
            self.insert = None;
            self.last_change = None;
        } else {
            self.insert = None;
            self.mode = Mode::Normal;
        }
    }

    pub fn status_text(&self, norwegian: bool) -> Option<String> {
        self.status.as_ref().map(|status| status.text(norwegian))
    }

    pub fn entry_text(&self) -> Option<String> {
        match self.mode {
            Mode::Command => Some(format!(":{}", self.command_line)),
            Mode::Search => Some(format!("/{}", self.search_line)),
            _ => None,
        }
    }

    pub fn wants_clipboard(&self, key: &Key) -> bool {
        matches!(self.register, '+' | '*')
            && matches!(self.mode, Mode::Normal | Mode::Visual | Mode::VisualLine)
            && matches!(key, Key::Text(text) if text == "p" || text == "P")
    }

    pub fn handle(
        &mut self,
        text: &str,
        cursor: usize,
        selection: Range<usize>,
        key: Key,
        clipboard: Option<&str>,
    ) -> Outcome {
        self.reconcile(text, cursor, &selection);
        let mut outcome = match self.mode {
            Mode::Insert => self.handle_insert(text, cursor, key),
            Mode::Command => self.handle_command(text, cursor, key),
            Mode::Search => self.handle_search(text, cursor, key),
            Mode::Normal | Mode::Visual | Mode::VisualLine => {
                self.handle_normal(text, cursor, key, clipboard)
            }
        };
        let mut updated = text.to_string();
        for effect in &mut outcome.effects {
            if let Effect::Replace(edit) = effect {
                updated.replace_range(edit.range.clone(), &edit.replacement);
                if self.mode != Mode::Insert {
                    if !edit.replacement.is_empty()
                        && edit.cursor == edit.range.start + edit.replacement.len()
                    {
                        edit.cursor = normal_cursor_in_line(&updated, edit.cursor);
                    } else if edit.replacement.is_empty() {
                        edit.cursor = normal_cursor_after_delete(&updated, edit.cursor);
                    }
                }
                self.expected_selection = Some(edit.cursor..edit.cursor);
                self.last_cursor = Some(edit.cursor);
            }
        }
        if let Some(status) = &outcome.status {
            self.status = Some(status.clone());
        } else if outcome.handled && outcome.state_changed {
            self.status = None;
        }
        outcome
    }

    fn reconcile(&mut self, text: &str, cursor: usize, selection: &Range<usize>) {
        if self.mode == Mode::Insert || self.mode == Mode::Command || self.mode == Mode::Search {
            self.last_cursor = Some(cursor);
            return;
        }
        if let Some(expected) = &self.expected_selection {
            if expected != selection {
                self.clear_pending();
                self.desired_column = None;
                self.visual_anchor = None;
                self.mode = Mode::Normal;
                self.expected_selection = Some(selection.clone());
            }
        } else if self.last_cursor.is_some_and(|last| last != cursor) {
            self.clear_pending();
            self.desired_column = None;
            self.expected_selection = Some(selection.clone());
        }
        self.active_cursor = if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
            self.active_cursor.min(text.len())
        } else {
            cursor.min(text.len())
        };
        self.last_cursor = Some(cursor);
    }

    fn handle_insert(&mut self, text: &str, cursor: usize, key: Key) -> Outcome {
        match key {
            Key::Escape => {
                let mut outcome = Outcome::handled();
                if let Some(insert) = self.insert.take() {
                    if let Some(delta) = changed_text_at(&insert.before, text, insert.start) {
                        let is_change = insert.change_motion.is_some()
                            || insert.change_object.is_some()
                            || insert.change_line.is_some();
                        if insert.record_recipe
                            && (!delta.is_empty()
                                || is_change
                                || matches!(
                                    insert.kind,
                                    InsertKind::OpenAbove | InsertKind::OpenBelow
                                ))
                        {
                            self.last_change = Some(
                                match (
                                    insert.change_motion,
                                    insert.change_object,
                                    insert.change_line,
                                    insert.kind,
                                ) {
                                    (Some(motion), _, _, _) => Recipe::Change {
                                        motion,
                                        text: delta,
                                    },
                                    (_, Some((object, count)), _, _) => Recipe::ChangeObject {
                                        object,
                                        count,
                                        text: delta,
                                    },
                                    (_, _, Some(count), _) => {
                                        Recipe::ChangeLine { count, text: delta }
                                    }
                                    (_, _, _, InsertKind::OpenAbove) => Recipe::OpenLine {
                                        above: true,
                                        text: delta,
                                    },
                                    (_, _, _, InsertKind::OpenBelow) => Recipe::OpenLine {
                                        above: false,
                                        text: delta,
                                    },
                                    (_, _, _, kind) => Recipe::Insert { kind, text: delta },
                                },
                            );
                        }
                    } else {
                        self.last_change = None;
                    }
                }
                self.mode = Mode::Normal;
                self.clear_pending();
                let cursor = normal_cursor_in_line(text, cursor);
                self.set_move(&mut outcome, cursor, cursor..cursor);
                outcome
            }
            Key::CtrlR => Outcome::pass(),
            _ => Outcome::pass(),
        }
    }

    fn handle_command(&mut self, text: &str, cursor: usize, key: Key) -> Outcome {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                self.command_line.clear();
                Outcome::handled()
            }
            Key::Backspace => {
                self.command_line.pop();
                Outcome::handled()
            }
            Key::Enter => {
                let command = std::mem::take(&mut self.command_line);
                self.mode = Mode::Normal;
                if command == "w" {
                    let mut result = Outcome::handled();
                    result.effects.push(Effect::Save);
                    result
                } else {
                    Outcome::with_status(StatusMessage::Error(
                        "Unsupported command",
                        "Kommandoen støttes ikke",
                    ))
                }
            }
            Key::Text(value) => {
                if value.chars().count() == 1
                    && value
                        .chars()
                        .next()
                        .is_some_and(|character| !character.is_control())
                    && self.command_line.len() < 256
                {
                    self.command_line.push_str(&value);
                }
                let _ = (text, cursor);
                Outcome::handled()
            }
            Key::Delete | Key::CtrlR => Outcome::handled(),
        }
    }

    fn handle_search(&mut self, text: &str, cursor: usize, key: Key) -> Outcome {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                self.search_line.clear();
                Outcome::handled()
            }
            Key::Backspace => {
                self.search_line.pop();
                Outcome::handled()
            }
            Key::Enter => {
                let query = std::mem::take(&mut self.search_line);
                self.mode = Mode::Normal;
                if query.is_empty() {
                    return Outcome::handled();
                }
                self.last_search = Some(query.clone());
                self.last_search_forward = true;
                self.search_from(text, cursor, &query, true)
            }
            Key::Text(value) => {
                if value.chars().count() == 1
                    && !value.chars().next().unwrap().is_control()
                    && self.search_line.len() < 256
                {
                    self.search_line.push_str(&value);
                }
                Outcome::handled()
            }
            Key::Delete | Key::CtrlR => Outcome::handled(),
        }
    }

    fn handle_normal(
        &mut self,
        text: &str,
        cursor: usize,
        key: Key,
        clipboard: Option<&str>,
    ) -> Outcome {
        if matches!(key, Key::Escape) {
            let visual = matches!(self.mode, Mode::Visual | Mode::VisualLine);
            self.clear_pending();
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            return self.simple_move(if visual { self.active_cursor } else { cursor });
        }
        if self.pending_register {
            self.pending_register = false;
            if let Key::Text(value) = &key {
                if let Some(register) = value.chars().next() {
                    self.register = register;
                }
            }
            return Outcome::handled();
        }

        if self.pending_find.is_some() {
            return self.handle_find(text, cursor, key);
        }

        if let Key::Text(value) = &key {
            if value.chars().all(|char| char.is_ascii_digit())
                && (!value.starts_with('0') || !self.count.is_empty())
            {
                self.count.push_str(value);
                if self.count.len() > 7 {
                    self.count.truncate(7);
                }
                return Outcome::handled();
            }
        }

        if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
            if matches!(&key, Key::Text(value) if value == "v") && self.mode == Mode::Visual {
                self.clear_pending();
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                return self.simple_move(self.active_cursor);
            }
            if matches!(key, Key::Escape) {
                self.clear_pending();
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                return self.simple_move(self.active_cursor);
            }
            if let Some(around) = self.pending_visual_text_object.take() {
                let Some(value) = key_text(&key) else {
                    return Outcome::handled();
                };
                let Some(object) = object_for_key(value, around) else {
                    return Outcome::handled();
                };
                let Some(range) = text_object_range(text, self.active_cursor, object) else {
                    return Outcome::handled();
                };
                self.visual_anchor = Some(range.start);
                self.active_cursor = previous_boundary(text, range.end);
                let mut result = Outcome::handled();
                self.set_move(&mut result, self.active_cursor, range);
                return result;
            }
            if matches!(&key, Key::Text(value) if value == "i" || value == "a") {
                self.pending_visual_text_object =
                    Some(matches!(&key, Key::Text(value) if value == "a"));
                return Outcome::handled();
            }
            if let Some(motion) = self.motion_for_key(&key) {
                let count = self.take_count();
                return self.apply_visual_motion(text, motion, count);
            }
            if matches!(&key, Key::Text(value) if value == "G" || value == "%") {
                let explicit_count = !self.count.is_empty();
                let count = self.take_count();
                return self.apply_visual_motion(
                    text,
                    if matches!(&key, Key::Text(value) if value == "G") {
                        if explicit_count {
                            Motion::FileStart
                        } else {
                            Motion::FileEnd
                        }
                    } else {
                        Motion::MatchPair
                    },
                    count,
                );
            }
            if let Some(operator) = self.operator_for_key(&key) {
                self.take_count();
                let anchor = self.visual_anchor.unwrap_or(self.active_cursor);
                let linewise = self.mode == Mode::VisualLine;
                let range = if self.mode == Mode::VisualLine {
                    line_start(text, anchor.min(self.active_cursor))
                        ..line_after(text, anchor.max(self.active_cursor))
                } else {
                    visual_range(text, anchor, self.active_cursor)
                };
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                let outcome = self.apply_operator_range(
                    text,
                    self.active_cursor,
                    operator,
                    range,
                    linewise,
                    1,
                );
                if matches!(operator, Operator::Delete | Operator::Change) {
                    self.last_change = None;
                }
                if operator == Operator::Change {
                    if let Some(insert) = self.insert.as_mut() {
                        insert.record_recipe = false;
                    }
                }
                return outcome;
            }
        }

        if self.pending_text_object.is_some() {
            return self.handle_text_object(text, key);
        }
        if self.pending_g {
            self.pending_g = false;
            if matches!(key, Key::Text(value) if value == "g") {
                let count = self.take_count();
                if let Some((operator, operator_count)) = self.pending_operator.take() {
                    return self.apply_operator_motion(
                        text,
                        cursor,
                        operator,
                        MotionSpec {
                            motion: Motion::FileStart,
                            count: operator_count.saturating_mul(count),
                        },
                    );
                }
                if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
                    return self.apply_visual_motion(text, Motion::FileStart, count);
                }
                return self.apply_motion(
                    text,
                    cursor,
                    MotionSpec {
                        motion: Motion::FileStart,
                        count,
                    },
                );
            }
            self.clear_pending();
            return Outcome::handled();
        }

        if let Key::Text(value) = &key {
            if value == "\"" {
                self.pending_register = true;
                return Outcome::handled();
            }
        }

        if let Some((operator, operator_count)) = self.pending_operator {
            if let Key::Text(value) = &key {
                if operator_char(operator) == value {
                    self.pending_operator = None;
                    let count = operator_count.saturating_mul(self.take_count());
                    return self.apply_line_operator(text, cursor, operator, count);
                }
                if value == "i" || value == "a" {
                    self.pending_text_object = Some((operator, value == "a", operator_count));
                    self.pending_operator = None;
                    return Outcome::handled();
                }
                if value == "g" {
                    self.pending_g = true;
                    return Outcome::handled();
                }
                if value == "G" || value == "%" {
                    self.pending_operator = None;
                    let explicit_count = !self.count.is_empty();
                    let count = operator_count.saturating_mul(self.take_count());
                    return self.apply_operator_motion(
                        text,
                        cursor,
                        operator,
                        MotionSpec {
                            motion: if value == "G" {
                                if explicit_count {
                                    Motion::FileStart
                                } else {
                                    Motion::FileEnd
                                }
                            } else {
                                Motion::MatchPair
                            },
                            count,
                        },
                    );
                }
                if value == "f" || value == "t" {
                    self.pending_find = Some(if value == "f" {
                        FindKind::Through
                    } else {
                        FindKind::Till
                    });
                    return Outcome::handled();
                }
            }
            if let Some(motion) = self.motion_for_key(&key) {
                self.pending_operator = None;
                let count = operator_count.saturating_mul(self.take_count());
                return self.apply_operator_motion(
                    text,
                    cursor,
                    operator,
                    MotionSpec { motion, count },
                );
            }
            self.clear_pending();
            return Outcome::handled();
        }

        if let Some(object) = self.operator_for_key(&key) {
            let count = self.take_count();
            self.pending_operator = Some((object, count));
            return Outcome::handled();
        }

        match key {
            Key::Escape => {
                self.clear_pending();
                self.mode = Mode::Normal;
                Outcome::handled()
            }
            Key::CtrlR => {
                let mut result = Outcome::handled();
                let count = self.take_count().min(text.len().saturating_add(1));
                result
                    .effects
                    .extend(std::iter::repeat_n(Effect::Redo, count));
                result
            }
            Key::Backspace | Key::Enter | Key::Delete => Outcome::handled(),
            Key::Text(value) if value == "u" => {
                let mut result = Outcome::handled();
                let count = self.take_count().min(text.len().saturating_add(1));
                result
                    .effects
                    .extend(std::iter::repeat_n(Effect::Undo, count));
                result
            }
            Key::Text(value) if value == "." => {
                let count = self.take_count();
                self.repeat(text, cursor, count)
            }
            Key::Text(value) if value == "v" => {
                self.take_count();
                self.mode = Mode::Visual;
                self.visual_anchor = Some(cursor);
                self.active_cursor = cursor;
                let mut result = Outcome::handled();
                self.set_move(&mut result, cursor, visual_range(text, cursor, cursor));
                result
            }
            Key::Text(value) if value == "V" => {
                self.take_count();
                self.mode = Mode::VisualLine;
                self.visual_anchor = Some(cursor);
                self.active_cursor = cursor;
                let range = line_range(text, cursor);
                let mut result = Outcome::handled();
                self.set_move(&mut result, cursor, range);
                result
            }
            Key::Text(value) if value == "i" => {
                self.take_count();
                self.enter_insert(text, cursor, InsertKind::Before)
            }
            Key::Text(value) if value == "a" => {
                self.take_count();
                self.enter_append(text, cursor)
            }
            Key::Text(value) if value == "I" => {
                self.take_count();
                self.enter_insert(
                    text,
                    move_motion(text, cursor, Motion::LineFirstText, 1),
                    InsertKind::LineStart,
                )
            }
            Key::Text(value) if value == "A" => {
                self.take_count();
                self.enter_insert(text, line_end(text, cursor), InsertKind::LineEnd)
            }
            Key::Text(value) if value == "o" => {
                self.take_count();
                self.open_line(text, cursor, false)
            }
            Key::Text(value) if value == "O" => {
                self.take_count();
                self.open_line(text, cursor, true)
            }
            Key::Text(value) if value == "p" || value == "P" => {
                self.paste(text, cursor, value == "P", clipboard)
            }
            Key::Text(value) if value == "D" => {
                self.take_count();
                let range = cursor..line_end(text, cursor);
                if range.start == range.end {
                    Outcome::handled()
                } else {
                    self.pending_change_motion = Some(MotionSpec {
                        motion: Motion::LineEnd,
                        count: 1,
                    });
                    let outcome =
                        self.apply_operator_range(text, cursor, Operator::Delete, range, false, 1);
                    self.last_change = Some(Recipe::DeleteMotion(MotionSpec {
                        motion: Motion::LineEnd,
                        count: 1,
                    }));
                    outcome
                }
            }
            Key::Text(value) if value == "C" => {
                self.take_count();
                let range = cursor..line_end(text, cursor);
                self.pending_change_motion = Some(MotionSpec {
                    motion: Motion::LineEnd,
                    count: 1,
                });
                self.apply_operator_range(text, cursor, Operator::Change, range, false, 1)
            }
            Key::Text(value) if value == "x" => self.delete_char(text, cursor, false),
            Key::Text(value) if value == "X" => self.delete_char(text, cursor, true),
            Key::Text(value) if value == "g" => {
                self.pending_g = true;
                Outcome::handled()
            }
            Key::Text(value) if value == "G" => {
                let explicit_count = !self.count.is_empty();
                let count = self.take_count();
                self.apply_motion(
                    text,
                    cursor,
                    MotionSpec {
                        motion: if explicit_count {
                            Motion::FileStart
                        } else {
                            Motion::FileEnd
                        },
                        count,
                    },
                )
            }
            Key::Text(value) if value == "f" || value == "t" => {
                self.pending_find = Some(if value == "f" {
                    FindKind::Through
                } else {
                    FindKind::Till
                });
                Outcome::handled()
            }
            Key::Text(value) if value == "%" => {
                let count = self.take_count();
                self.apply_motion(
                    text,
                    cursor,
                    MotionSpec {
                        motion: Motion::MatchPair,
                        count,
                    },
                )
            }
            Key::Text(value) if value == "/" => {
                self.take_count();
                self.mode = Mode::Search;
                self.search_line.clear();
                Outcome::handled()
            }
            Key::Text(value) if value == ":" => {
                self.take_count();
                self.mode = Mode::Command;
                self.command_line.clear();
                Outcome::handled()
            }
            Key::Text(value) if value == "n" || value == "N" => {
                let Some(query) = self.last_search.clone() else {
                    self.take_count();
                    return Outcome::with_status(StatusMessage::Error(
                        "No previous search",
                        "Ingen tidligere søking",
                    ));
                };
                let forward = (value == "n") == self.last_search_forward;
                let count = self.take_count().min(text.len().saturating_add(1));
                self.search_repeated(text, cursor, &query, forward, count)
            }
            Key::Text(_) => {
                if let Some(motion) = self.motion_for_key(&key) {
                    let count = self.take_count();
                    self.apply_motion(text, cursor, MotionSpec { motion, count })
                } else {
                    Outcome::handled()
                }
            }
        }
    }

    fn operator_for_key(&self, key: &Key) -> Option<Operator> {
        match key {
            Key::Text(value) => match value.as_str() {
                "d" => Some(Operator::Delete),
                "c" => Some(Operator::Change),
                "y" => Some(Operator::Yank),
                _ => None,
            },
            _ => None,
        }
    }

    fn motion_for_key(&self, key: &Key) -> Option<Motion> {
        match key {
            Key::Text(value) => match value.as_str() {
                "h" => Some(Motion::Left),
                "j" => Some(Motion::Down),
                "k" => Some(Motion::Up),
                "l" => Some(Motion::Right),
                "w" => Some(Motion::WordForward),
                "b" => Some(Motion::WordBackward),
                "e" => Some(Motion::WordEnd),
                "0" => Some(Motion::LineStart),
                "$" => Some(Motion::LineEnd),
                "^" => Some(Motion::LineFirstText),
                _ => None,
            },
            _ => None,
        }
    }

    fn handle_find(&mut self, text: &str, cursor: usize, key: Key) -> Outcome {
        let kind = self.pending_find.take().unwrap();
        let cursor = if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
            self.active_cursor
        } else {
            cursor
        };
        let Some(value) = key_text(&key) else {
            return Outcome::handled();
        };
        let Some(target) = value.chars().next() else {
            return Outcome::handled();
        };
        let count = self.take_count();
        if let Some((operator, operator_count)) = self.pending_operator.take() {
            let spec = MotionSpec {
                motion: Motion::Find(kind, target),
                count: operator_count.saturating_mul(count),
            };
            return self.apply_operator_motion(text, cursor, operator, spec);
        }
        let Some(found) = find_forward_n(text, cursor, target, kind, count) else {
            self.register = '"';
            return Outcome::with_status(StatusMessage::Error(
                "Pattern not found",
                "Fant ikke mønsteret",
            ));
        };
        if self.mode == Mode::Visual || self.mode == Mode::VisualLine {
            self.active_cursor = found;
            let mut result = Outcome::handled();
            let range = if self.mode == Mode::VisualLine {
                let anchor = self.visual_anchor.unwrap_or(found);
                line_start(text, anchor.min(found))..line_after(text, anchor.max(found))
            } else {
                visual_range(text, self.visual_anchor.unwrap_or(found), found)
            };
            self.set_move(&mut result, found, range);
            result
        } else {
            self.move_outcome(found, found..found)
        }
    }

    fn handle_text_object(&mut self, text: &str, key: Key) -> Outcome {
        let (operator, around, operator_count) = self.pending_text_object.take().unwrap();
        let Some(value) = key_text(&key) else {
            return Outcome::handled();
        };
        let Some(object) = object_for_key(value, around) else {
            self.register = '"';
            return Outcome::handled();
        };
        let cursor = self.active_cursor;
        let count = operator_count
            .saturating_mul(self.take_count())
            .clamp(1, MAX_COUNT);
        let Some(range) = text_object_range_count(text, cursor, object, count) else {
            self.register = '"';
            return Outcome::with_status(StatusMessage::Error(
                "Text object not found",
                "Fant ikke tekstobjektet",
            ));
        };
        if operator == Operator::Change {
            self.pending_change_object = Some((object, count));
        }
        let outcome = self.apply_operator_range(text, cursor, operator, range, false, 1);
        if operator == Operator::Delete {
            self.last_change = Some(Recipe::DeleteObject { object, count });
        }
        outcome
    }

    fn apply_motion(&mut self, text: &str, cursor: usize, spec: MotionSpec) -> Outcome {
        let Some(next) = self.motion_target(text, cursor, spec.motion, spec.count) else {
            return Outcome::handled();
        };
        self.move_outcome(next, next..next)
    }

    fn apply_visual_motion(&mut self, text: &str, motion: Motion, count: usize) -> Outcome {
        let Some(next) = self.motion_target(text, self.active_cursor, motion, count) else {
            return Outcome::handled();
        };
        self.active_cursor = next;
        let range = if self.mode == Mode::VisualLine {
            let anchor = self.visual_anchor.unwrap_or(next);
            line_start(text, anchor.min(next))..line_after(text, anchor.max(next))
        } else {
            visual_range(text, self.visual_anchor.unwrap_or(next), next)
        };
        let mut result = Outcome::handled();
        self.set_move(&mut result, next, range);
        result
    }

    fn motion_target(
        &mut self,
        text: &str,
        cursor: usize,
        motion: Motion,
        count: usize,
    ) -> Option<usize> {
        match motion {
            Motion::Find(kind, target) => find_forward_n(text, cursor, target, kind, count),
            Motion::MatchPair => matching_pair(text, cursor),
            Motion::Up | Motion::Down => {
                let start = line_start(text, cursor);
                let column = *self
                    .desired_column
                    .get_or_insert_with(|| text[start..cursor].graphemes(true).count());
                Some(move_vertical_to_column(
                    text,
                    cursor,
                    count,
                    motion == Motion::Down,
                    column,
                ))
            }
            _ => Some({
                self.desired_column = None;
                move_motion(text, cursor, motion, count)
            }),
        }
    }

    fn apply_operator_motion(
        &mut self,
        text: &str,
        cursor: usize,
        operator: Operator,
        spec: MotionSpec,
    ) -> Outcome {
        let spec = if operator == Operator::Change && spec.motion == Motion::WordForward {
            MotionSpec {
                motion: Motion::WordEnd,
                count: spec.count,
            }
        } else {
            spec
        };
        let Some(next) = self.motion_target(text, cursor, spec.motion, spec.count) else {
            return Outcome::handled();
        };
        let linewise = matches!(
            spec.motion,
            Motion::Up | Motion::Down | Motion::FileStart | Motion::FileEnd
        );
        let range = if linewise {
            line_start(text, cursor.min(next))..line_after(text, cursor.max(next))
        } else {
            let inclusive = matches!(
                spec.motion,
                Motion::WordEnd
                    | Motion::LineEnd
                    | Motion::Find(_, _)
                    | Motion::MatchPair
                    | Motion::FileEnd
            );
            let (start, end) = if next >= cursor {
                (
                    cursor,
                    if spec.motion == Motion::FileEnd && spec.count == 1 {
                        text.len()
                    } else if inclusive {
                        next_boundary(text, next)
                    } else {
                        next
                    },
                )
            } else {
                (
                    next,
                    if inclusive {
                        next_boundary(text, cursor)
                    } else {
                        cursor
                    },
                )
            };
            start..end
        };
        if operator == Operator::Change {
            self.pending_change_motion = Some(spec);
        }
        let outcome =
            self.apply_operator_range(text, cursor, operator, range, linewise, spec.count);
        if operator == Operator::Delete {
            self.last_change = Some(Recipe::DeleteMotion(spec));
        }
        outcome
    }

    fn apply_line_operator(
        &mut self,
        text: &str,
        cursor: usize,
        operator: Operator,
        count: usize,
    ) -> Outcome {
        if operator == Operator::Change {
            self.pending_change_line = Some(count.max(1));
        }
        let range = if operator == Operator::Change {
            change_line_range(text, cursor, count)
        } else {
            let mut end = cursor;
            let mut repetitions = count.max(1).min(line_count(text).max(1));
            while repetitions > 0 {
                let next = line_after(text, end);
                if next == end {
                    break;
                }
                end = next;
                repetitions -= 1;
            }
            line_start(text, cursor)..end
        };
        let outcome = self.apply_operator_range(text, cursor, operator, range, true, count);
        if operator == Operator::Delete {
            self.last_change = Some(Recipe::DeleteLine(count));
        }
        outcome
    }

    fn apply_operator_range(
        &mut self,
        text: &str,
        _cursor: usize,
        operator: Operator,
        range: Range<usize>,
        linewise: bool,
        _count: usize,
    ) -> Outcome {
        let range = clamp_range(text, range);
        let range = if operator == Operator::Change && linewise {
            let end = range
                .end
                .checked_sub(1)
                .map(|offset| line_end(text, offset))
                .unwrap_or(range.end);
            range.start..end.max(range.start)
        } else {
            range
        };
        if range.start == range.end {
            if operator == Operator::Change {
                self.mode = Mode::Insert;
                self.insert = Some(InsertSession {
                    before: text.to_string(),
                    start: range.start,
                    kind: InsertKind::Before,
                    change_motion: self.pending_change_motion.take(),
                    change_object: self.pending_change_object.take(),
                    change_line: self.pending_change_line.take(),
                    record_recipe: true,
                });
                let mut result = Outcome::handled();
                self.set_move(&mut result, range.start, range.start..range.start);
                return result;
            }
            return Outcome::handled();
        }
        let value = text[range.clone()].to_string();
        match operator {
            Operator::Yank => {
                let clipboard = self.store_register(RegisterValue {
                    text: value,
                    linewise,
                });
                self.register = '"';
                let mut result = self.move_outcome(range.start, range.start..range.start);
                if let Some(text) = clipboard {
                    result.effects.push(Effect::ClipboardWrite(text));
                }
                result
            }
            Operator::Delete | Operator::Change => {
                let clipboard = self.store_register(RegisterValue {
                    text: value,
                    linewise,
                });
                self.register = '"';
                let replacement = String::new();
                let mut outcome = Outcome::handled();
                outcome.effects.push(Effect::Replace(Edit {
                    range: range.clone(),
                    replacement,
                    cursor: range.start,
                }));
                if let Some(text) = clipboard {
                    outcome.effects.push(Effect::ClipboardWrite(text));
                }
                self.expected_selection = Some(range.start..range.start);
                self.last_cursor = Some(range.start);
                if operator == Operator::Change {
                    self.mode = Mode::Insert;
                    self.insert = Some(InsertSession {
                        before: text[..range.start].to_string() + &text[range.end..],
                        start: range.start,
                        kind: InsertKind::Before,
                        change_motion: self.pending_change_motion.take(),
                        change_object: self.pending_change_object.take(),
                        change_line: self.pending_change_line.take(),
                        record_recipe: true,
                    });
                } else {
                    self.mode = Mode::Normal;
                    self.pending_change_motion = None;
                    self.pending_change_object = None;
                    self.pending_change_line = None;
                }
                outcome
            }
        }
    }

    fn enter_insert(&mut self, text: &str, cursor: usize, kind: InsertKind) -> Outcome {
        self.mode = Mode::Insert;
        self.insert = Some(InsertSession {
            before: text.to_string(),
            start: cursor,
            kind,
            change_motion: None,
            change_object: None,
            change_line: None,
            record_recipe: true,
        });
        let mut outcome = Outcome::handled();
        self.set_move(&mut outcome, cursor, cursor..cursor);
        outcome
    }

    fn enter_append(&mut self, text: &str, cursor: usize) -> Outcome {
        let at = if cursor < text.len() {
            next_boundary(text, cursor)
        } else {
            text.len()
        };
        let mut result = Outcome::handled();
        self.set_move(&mut result, at, at..at);
        self.mode = Mode::Insert;
        self.insert = Some(InsertSession {
            before: text.to_string(),
            start: at,
            kind: InsertKind::After,
            change_motion: None,
            change_object: None,
            change_line: None,
            record_recipe: true,
        });
        result
    }

    fn open_line(&mut self, text: &str, cursor: usize, above: bool) -> Outcome {
        let at = if above {
            line_start(text, cursor)
        } else {
            line_after(text, cursor)
        };
        let mut result = Outcome::handled();
        let insert_cursor = if above || at < text.len() || text.ends_with('\n') {
            at
        } else {
            at + 1
        };
        result.effects.push(Effect::Replace(Edit {
            range: at..at,
            replacement: "\n".to_string(),
            cursor: insert_cursor,
        }));
        self.expected_selection = Some(insert_cursor..insert_cursor);
        self.last_cursor = Some(insert_cursor);
        self.mode = Mode::Insert;
        let mut after = text.to_string();
        after.insert(at, '\n');
        self.insert = Some(InsertSession {
            before: after,
            start: insert_cursor,
            kind: if above {
                InsertKind::OpenAbove
            } else {
                InsertKind::OpenBelow
            },
            change_motion: None,
            change_object: None,
            change_line: None,
            record_recipe: true,
        });
        result
    }

    fn delete_char(&mut self, text: &str, cursor: usize, backwards: bool) -> Outcome {
        let count = self.take_count();
        let range = charwise_delete_range(text, cursor, backwards, count);
        if range.start == range.end {
            return Outcome::handled();
        }
        let outcome =
            self.apply_operator_range(text, cursor, Operator::Delete, range, false, count);
        self.last_change = Some(Recipe::DeleteChars { backwards, count });
        outcome
    }

    fn paste(
        &mut self,
        text: &str,
        cursor: usize,
        before: bool,
        clipboard: Option<&str>,
    ) -> Outcome {
        let value = self.register_value(clipboard);
        self.register = '"';
        let Some(value) = value else {
            return Outcome::with_status(StatusMessage::Error(
                "Register is empty",
                "Registeret er tomt",
            ));
        };
        let count = self.take_count();
        let repeated = repeat_text(&value.text, count, MAX_EDIT_BYTES);
        let at = if value.linewise {
            if before {
                line_start(text, cursor)
            } else {
                line_after(text, cursor)
            }
        } else if before {
            cursor
        } else {
            next_boundary(text, cursor)
        };
        let repeated = if value.linewise {
            linewise_paste(text, at, repeated)
        } else {
            repeated
        };
        let mut result = Outcome::handled();
        result.effects.push(Effect::Replace(Edit {
            range: at..at,
            cursor: if value.linewise {
                linewise_paste_cursor(at, &repeated)
            } else {
                at + repeated.len()
            },
            replacement: repeated,
        }));
        self.last_change = Some(Recipe::Paste {
            value,
            before,
            count,
        });
        result
    }

    fn repeat(&mut self, text: &str, cursor: usize, count: usize) -> Outcome {
        let Some(recipe) = self.last_change.clone() else {
            return Outcome::handled();
        };
        let mut simulated = text.to_string();
        let mut active_cursor = cursor;
        let budget = (16 * 1024 * 1024).max(text.len().saturating_mul(2));
        let mut work = 0usize;
        for _ in 0..count {
            let outcome = self.repeat_once(&simulated, active_cursor, recipe.clone());
            let Some(edit) = outcome.effects.into_iter().find_map(|effect| match effect {
                Effect::Replace(edit) => Some(edit),
                _ => None,
            }) else {
                break;
            };
            if simulated.get(edit.range.clone()) == Some(edit.replacement.as_str()) {
                break;
            }
            let cost = simulated.len().saturating_add(edit.replacement.len());
            if work.saturating_add(cost) > budget {
                return Outcome::with_status(StatusMessage::Error(
                    "Repeat exceeds the work limit",
                    "Gjentakelsen overskrider arbeidsgrensen",
                ));
            }
            work = work.saturating_add(cost);
            simulated.replace_range(edit.range.clone(), &edit.replacement);
            active_cursor = if !edit.replacement.is_empty()
                && edit.cursor == edit.range.start + edit.replacement.len()
            {
                normal_cursor_in_line(&simulated, edit.cursor)
            } else if edit.replacement.is_empty() {
                normal_cursor_after_delete(&simulated, edit.cursor)
            } else {
                normal_cursor(&simulated, edit.cursor)
            };
        }
        let Some((range, replacement)) = diff_range(text, &simulated) else {
            return Outcome::handled();
        };
        let mut result = Outcome::handled();
        result.effects.push(Effect::Replace(Edit {
            range,
            replacement,
            cursor: active_cursor,
        }));
        result
    }

    fn repeat_once(&mut self, text: &str, cursor: usize, recipe: Recipe) -> Outcome {
        match recipe {
            Recipe::Insert { kind, text: value } => {
                let at = insert_position(text, cursor, kind);
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    range: at..at,
                    cursor: at + value.len(),
                    replacement: value,
                }));
                result
            }
            Recipe::OpenLine { above, text: value } => {
                let at = if above {
                    line_start(text, cursor)
                } else {
                    line_after(text, cursor)
                };
                let insertion_after_break = !above && at == text.len() && !text.ends_with('\n');
                let replacement = if insertion_after_break {
                    format!("\n{value}")
                } else {
                    format!("{value}\n")
                };
                let cursor = if value.is_empty() {
                    at
                } else if insertion_after_break {
                    at + 1 + previous_boundary(&value, value.len())
                } else {
                    at + previous_boundary(&value, value.len())
                };
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    range: at..at,
                    cursor,
                    replacement,
                }));
                result
            }
            Recipe::Change {
                motion,
                text: value,
            } => {
                let Some(next) = self.motion_target(text, cursor, motion.motion, motion.count)
                else {
                    return Outcome::handled();
                };
                let linewise = matches!(
                    motion.motion,
                    Motion::Up | Motion::Down | Motion::FileStart | Motion::FileEnd
                );
                let mut range = if linewise {
                    line_start(text, cursor.min(next))..line_after(text, cursor.max(next))
                } else {
                    let Some(range) = motion_range(text, cursor, motion) else {
                        return Outcome::handled();
                    };
                    range
                };
                if linewise && range.end > range.start {
                    range.end = line_end(text, previous_boundary(text, range.end));
                }
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    range: range.clone(),
                    cursor: range.start + value.len(),
                    replacement: value,
                }));
                result
            }
            Recipe::ChangeObject {
                object,
                count,
                text: value,
            } => {
                let Some(range) = text_object_range_count(text, cursor, object, count) else {
                    return Outcome::handled();
                };
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    cursor: range.start + value.len(),
                    range,
                    replacement: value,
                }));
                result
            }
            Recipe::DeleteObject { object, count } => {
                let Some(range) = text_object_range_count(text, cursor, object, count) else {
                    return Outcome::handled();
                };
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    cursor: range.start,
                    range,
                    replacement: String::new(),
                }));
                result
            }
            Recipe::ChangeLine { count, text: value } => {
                let range = change_line_range(text, cursor, count);
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    cursor: range.start + value.len(),
                    range,
                    replacement: value,
                }));
                result
            }
            Recipe::DeleteMotion(motion) => {
                let Some(next) = self.motion_target(text, cursor, motion.motion, motion.count)
                else {
                    return Outcome::handled();
                };
                let range = if matches!(
                    motion.motion,
                    Motion::Up | Motion::Down | Motion::FileStart | Motion::FileEnd
                ) {
                    line_start(text, cursor.min(next))..line_after(text, cursor.max(next))
                } else {
                    let Some(range) = motion_range(text, cursor, motion) else {
                        return Outcome::handled();
                    };
                    range
                };
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    cursor: range.start,
                    range,
                    replacement: String::new(),
                }));
                result
            }
            Recipe::DeleteLine(count) => {
                let mut end = cursor;
                for _ in 0..count.min(line_count(text)) {
                    let next = line_after(text, end);
                    if next == end {
                        break;
                    }
                    end = next;
                }
                let range = line_start(text, cursor)..end;
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    cursor: range.start,
                    range,
                    replacement: String::new(),
                }));
                result
            }
            Recipe::DeleteChars { backwards, count } => {
                let range = charwise_delete_range(text, cursor, backwards, count);
                let mut result = Outcome::handled();
                if range.start != range.end {
                    result.effects.push(Effect::Replace(Edit {
                        cursor: range.start,
                        range,
                        replacement: String::new(),
                    }));
                }
                result
            }
            Recipe::Paste {
                value,
                before,
                count,
            } => {
                let at = if value.linewise {
                    if before {
                        line_start(text, cursor)
                    } else {
                        line_after(text, cursor)
                    }
                } else if before {
                    cursor
                } else {
                    next_boundary(text, cursor)
                };
                let pasted = if value.linewise {
                    linewise_paste(text, at, repeat_text(&value.text, count, MAX_EDIT_BYTES))
                } else {
                    repeat_text(&value.text, count, MAX_EDIT_BYTES)
                };
                let mut result = Outcome::handled();
                result.effects.push(Effect::Replace(Edit {
                    range: at..at,
                    cursor: if value.linewise {
                        linewise_paste_cursor(at, &pasted)
                    } else {
                        at + pasted.len()
                    },
                    replacement: pasted,
                }));
                result
            }
        }
    }

    fn search_from(&mut self, text: &str, cursor: usize, query: &str, forward: bool) -> Outcome {
        let Ok(regex) = RegexBuilder::new(query)
            .size_limit(1_000_000)
            .nest_limit(32)
            .build()
        else {
            return Outcome::with_status(StatusMessage::Error(
                "Invalid search pattern",
                "Ugyldig søkemønster",
            ));
        };
        let found = if forward {
            regex
                .find_iter(text)
                .find(|found| found.start() > cursor)
                .or_else(|| regex.find(text))
        } else {
            regex
                .find_iter(text)
                .filter(|found| found.start() < cursor)
                .last()
                .or_else(|| regex.find_iter(text).last())
        };
        let Some(found) = found else {
            return Outcome::with_status(StatusMessage::Error(
                "Pattern not found",
                "Fant ikke mønsteret",
            ));
        };
        self.move_outcome(found.start(), found.start()..found.start())
    }

    fn search_repeated(
        &mut self,
        text: &str,
        cursor: usize,
        query: &str,
        forward: bool,
        count: usize,
    ) -> Outcome {
        let Ok(regex) = RegexBuilder::new(query)
            .size_limit(1_000_000)
            .nest_limit(32)
            .build()
        else {
            return Outcome::with_status(StatusMessage::Error(
                "Invalid search pattern",
                "Ugyldig søkemønster",
            ));
        };
        let matches: Vec<_> = regex
            .find_iter(text)
            .map(|matched| matched.start())
            .collect();
        if matches.is_empty() {
            return Outcome::with_status(StatusMessage::Error(
                "Pattern not found",
                "Fant ikke mønsteret",
            ));
        }
        let count = count.max(1);
        let first = if forward {
            let next = matches.partition_point(|matched| *matched <= cursor);
            if next == matches.len() {
                0
            } else {
                next
            }
        } else {
            let previous = matches.partition_point(|matched| *matched < cursor);
            if previous == 0 {
                matches.len() - 1
            } else {
                previous - 1
            }
        };
        let steps = (count - 1) % matches.len();
        let target = if forward {
            (first + steps) % matches.len()
        } else {
            (first + matches.len() - steps) % matches.len()
        };
        let found = matches[target];
        self.move_outcome(found, found..found)
    }

    fn register_value(&self, clipboard: Option<&str>) -> Option<RegisterValue> {
        if self.register == '+' || self.register == '*' {
            return clipboard.map(|text| RegisterValue {
                text: text.to_string(),
                linewise: false,
            });
        }
        let bank = self.registers.lock().ok()?;
        if self.register == '"' {
            return bank.unnamed.clone();
        }
        bank.values
            .get(&self.register.to_ascii_lowercase())
            .cloned()
    }

    fn store_register(&self, value: RegisterValue) -> Option<String> {
        if self.register == '_' {
            return None;
        }
        let Ok(mut bank) = self.registers.lock() else {
            return None;
        };
        if self.register == '+' || self.register == '*' {
            bank.unnamed = Some(value.clone());
            Some(value.text)
        } else {
            let key = self.register.to_ascii_lowercase();
            if self.register.is_ascii_uppercase() {
                if let Some(existing) = bank.values.get_mut(&key) {
                    existing.text.push_str(&value.text);
                    existing.linewise = value.linewise;
                } else {
                    bank.values.insert(key, value.clone());
                }
            } else {
                bank.values.insert(key, value.clone());
            }
            bank.unnamed = Some(value);
            None
        }
    }

    fn move_outcome(&mut self, cursor: usize, selection: Range<usize>) -> Outcome {
        let mut result = Outcome::handled();
        self.set_move(&mut result, cursor, selection);
        result
    }

    fn simple_move(&mut self, cursor: usize) -> Outcome {
        self.move_outcome(cursor, cursor..cursor)
    }

    fn set_move(&mut self, result: &mut Outcome, cursor: usize, selection: Range<usize>) {
        self.active_cursor = cursor;
        self.expected_selection = Some(selection.clone());
        self.last_cursor = Some(cursor);
        result.effects.push(Effect::Move { selection, cursor });
    }

    fn clear_pending(&mut self) {
        self.pending_operator = None;
        self.pending_text_object = None;
        self.pending_visual_text_object = None;
        self.pending_change_motion = None;
        self.pending_change_object = None;
        self.pending_change_line = None;
        self.pending_find = None;
        self.pending_g = false;
        self.pending_register = false;
        self.register = '"';
        self.count.clear();
    }

    fn take_count(&mut self) -> usize {
        let value = self.count.parse::<usize>().unwrap_or(1);
        self.count.clear();
        value.clamp(1, MAX_COUNT)
    }
}

fn operator_char(operator: Operator) -> &'static str {
    match operator {
        Operator::Delete => "d",
        Operator::Change => "c",
        Operator::Yank => "y",
    }
}

fn key_text(key: &Key) -> Option<&str> {
    match key {
        Key::Text(value) => Some(value),
        _ => None,
    }
}

fn changed_text_at(before: &str, after: &str, start: usize) -> Option<String> {
    if start > before.len() || !before.is_char_boundary(start) {
        return None;
    }
    let remainder = after.strip_prefix(&before[..start])?;
    let inserted = remainder.strip_suffix(&before[start..])?;
    Some(inserted.to_string())
}

fn clamp_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text.floor_char_boundary(range.start.min(text.len()));
    let end = text
        .ceil_char_boundary(range.end.min(text.len()))
        .max(start);
    start..end
}

fn repeat_text(value: &str, count: usize, limit: usize) -> String {
    if value.is_empty() {
        return String::new();
    }
    let count = count.min((limit / value.len()).max(1));
    value.repeat(count)
}

fn diff_range(before: &str, after: &str) -> Option<(Range<usize>, String)> {
    let mut prefix = 0;
    for (left, right) in before.chars().zip(after.chars()) {
        if left != right {
            break;
        }
        prefix += left.len_utf8();
    }
    let mut suffix = 0;
    for (left, right) in before[prefix..]
        .chars()
        .rev()
        .zip(after[prefix..].chars().rev())
    {
        let left_len = left.len_utf8();
        if suffix + left_len > before.len() - prefix
            || suffix + right.len_utf8() > after.len() - prefix
        {
            break;
        }
        if left != right {
            break;
        }
        suffix += left_len;
    }
    let range = prefix..before.len() - suffix;
    let replacement = after[prefix..after.len() - suffix].to_string();
    if range.start == range.end && replacement.is_empty() {
        None
    } else {
        Some((range, replacement))
    }
}

fn normal_cursor(text: &str, cursor: usize) -> usize {
    if text.is_empty() {
        0
    } else if cursor >= text.len() {
        previous_boundary(text, text.len())
    } else {
        text.floor_char_boundary(cursor)
    }
}

fn normal_cursor_in_line(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    let start = line_start(text, cursor);
    let end = line_end(text, cursor);
    if cursor <= start {
        start
    } else {
        previous_boundary(text, cursor.min(end))
    }
}

fn next_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[offset..]
        .grapheme_indices(true)
        .nth(1)
        .map(|(index, _)| offset + index)
        .unwrap_or(text.len())
}

fn previous_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    text[..offset]
        .grapheme_indices(true)
        .next_back()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn line_start(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

fn line_end(text: &str, offset: usize) -> usize {
    let start = line_start(text, offset);
    text[start..]
        .find('\n')
        .map(|index| start + index)
        .unwrap_or(text.len())
}

fn line_after(text: &str, offset: usize) -> usize {
    let end = line_end(text, offset);
    if end < text.len() {
        end + 1
    } else {
        end
    }
}

fn line_range(text: &str, offset: usize) -> Range<usize> {
    let start = line_start(text, offset);
    let after = line_after(text, offset);
    start..after
}

fn visual_range(text: &str, anchor: usize, cursor: usize) -> Range<usize> {
    let start = anchor.min(cursor);
    let end = anchor.max(cursor);
    start..next_boundary(text, end)
}

fn change_line_range(text: &str, cursor: usize, count: usize) -> Range<usize> {
    let mut last = cursor;
    for _ in 1..count.min(line_count(text)) {
        let next = line_after(text, last);
        if next == last {
            break;
        }
        last = next;
    }
    line_start(text, cursor)..line_end(text, last)
}

fn normal_cursor_after_delete(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    let start = line_start(text, cursor);
    let end = line_end(text, cursor);
    if cursor > start && cursor >= end {
        previous_boundary(text, end)
    } else {
        cursor
    }
}

fn linewise_paste(text: &str, at: usize, mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    if at == text.len() && at > 0 && !text.ends_with('\n') {
        value.insert(0, '\n');
    }
    value
}

fn linewise_paste_cursor(at: usize, value: &str) -> usize {
    if value.ends_with('\n') {
        at + previous_boundary(value, value.len() - 1)
    } else {
        at + value.len()
    }
}

fn line_count(text: &str) -> usize {
    text.lines().count().max(1)
}

fn word_class(grapheme: &str) -> u8 {
    if grapheme.chars().all(char::is_whitespace) {
        0
    } else if grapheme
        .chars()
        .any(|char| char.is_alphanumeric() || char == '_')
    {
        1
    } else {
        2
    }
}

fn grapheme_at(text: &str, offset: usize) -> Option<&str> {
    text[offset.min(text.len())..].graphemes(true).next()
}

fn move_word_forward(text: &str, mut cursor: usize, count: usize) -> usize {
    for _ in 0..count.min(text.len().saturating_add(1)) {
        let Some(current) = grapheme_at(text, cursor) else {
            break;
        };
        let class = word_class(current);
        while cursor < text.len()
            && grapheme_at(text, cursor).is_some_and(|g| word_class(g) == class)
        {
            cursor = next_boundary(text, cursor);
        }
        while cursor < text.len() && grapheme_at(text, cursor).is_some_and(|g| word_class(g) == 0) {
            cursor = next_boundary(text, cursor);
        }
    }
    cursor
}

fn move_word_backward(text: &str, mut cursor: usize, count: usize) -> usize {
    for _ in 0..count.min(text.len().saturating_add(1)) {
        cursor = previous_boundary(text, cursor);
        while cursor > 0 && grapheme_at(text, cursor).is_some_and(|g| word_class(g) == 0) {
            cursor = previous_boundary(text, cursor);
        }
        let Some(class) = grapheme_at(text, cursor).map(word_class) else {
            break;
        };
        while cursor > 0 {
            let previous = previous_boundary(text, cursor);
            if grapheme_at(text, previous).is_some_and(|g| word_class(g) == class) {
                cursor = previous;
            } else {
                break;
            }
        }
    }
    cursor
}

fn move_word_end(text: &str, mut cursor: usize, count: usize) -> usize {
    for index in 0..count.min(text.len().saturating_add(1)) {
        let Some(current) = grapheme_at(text, cursor) else {
            return normal_cursor(text, text.len());
        };
        let class = word_class(current);
        let next = next_boundary(text, cursor);
        let next_matches =
            grapheme_at(text, next).is_some_and(|grapheme| word_class(grapheme) == class);
        if index > 0 || class == 0 || next >= text.len() || !next_matches {
            cursor = move_word_forward(text, cursor, 1);
        }
        if cursor >= text.len() {
            return normal_cursor(text, text.len());
        }
        let class = grapheme_at(text, cursor).map(word_class).unwrap_or(0);
        while next_boundary(text, cursor) < text.len()
            && grapheme_at(text, next_boundary(text, cursor))
                .is_some_and(|grapheme| word_class(grapheme) == class)
        {
            cursor = next_boundary(text, cursor);
        }
    }
    cursor
}

fn move_motion(text: &str, cursor: usize, motion: Motion, count: usize) -> usize {
    match motion {
        Motion::Left => move_horizontal(text, cursor, count, false),
        Motion::Right => move_horizontal(text, cursor, count, true),
        Motion::Up => move_vertical(text, cursor, count, false),
        Motion::Down => move_vertical(text, cursor, count, true),
        Motion::WordForward => move_word_forward(text, cursor, count),
        Motion::WordBackward => move_word_backward(text, cursor, count),
        Motion::WordEnd => move_word_end(text, cursor, count),
        Motion::LineStart => line_start(text, cursor),
        Motion::LineFirstText => {
            let start = line_start(text, cursor);
            let end = line_end(text, cursor);
            text[start..end]
                .grapheme_indices(true)
                .find(|(_, grapheme)| !grapheme.chars().all(char::is_whitespace))
                .map(|(index, _)| start + index)
                .unwrap_or(start)
        }
        Motion::LineEnd => {
            let end = line_end(text, cursor);
            if end == line_start(text, cursor) {
                end
            } else {
                previous_boundary(text, end)
            }
        }
        Motion::FileStart => {
            if count <= 1 {
                0
            } else {
                line_for_number(text, count)
            }
        }
        Motion::FileEnd => {
            if count > 1 {
                line_for_number(text, count)
            } else {
                let offset = previous_boundary(text, text.len());
                move_motion(text, offset, Motion::LineFirstText, 1)
            }
        }
        Motion::Find(kind, target) => {
            find_forward_n(text, cursor, target, kind, count).unwrap_or(cursor)
        }
        Motion::MatchPair => matching_pair(text, cursor).unwrap_or(cursor),
    }
}

fn motion_range(text: &str, cursor: usize, spec: MotionSpec) -> Option<Range<usize>> {
    let next = match spec.motion {
        Motion::Find(kind, target) => find_forward_n(text, cursor, target, kind, spec.count)?,
        Motion::MatchPair => matching_pair(text, cursor)?,
        _ => move_motion(text, cursor, spec.motion, spec.count),
    };
    let inclusive = matches!(
        spec.motion,
        Motion::WordEnd
            | Motion::LineEnd
            | Motion::Find(_, _)
            | Motion::MatchPair
            | Motion::FileEnd
    );
    if next >= cursor {
        Some(
            cursor..if spec.motion == Motion::FileEnd && spec.count == 1 {
                text.len()
            } else if inclusive {
                next_boundary(text, next)
            } else {
                next
            },
        )
    } else {
        Some(
            next..if inclusive {
                next_boundary(text, cursor)
            } else {
                cursor
            },
        )
    }
}

fn charwise_delete_range(text: &str, cursor: usize, backwards: bool, count: usize) -> Range<usize> {
    let line_start = line_start(text, cursor);
    let line_end = line_end(text, cursor);
    let mut edge = cursor;
    for _ in 0..count.min(text.len().saturating_add(1)) {
        if (backwards && edge <= line_start) || (!backwards && edge >= line_end) {
            break;
        }
        edge = if backwards {
            previous_boundary(text, edge)
        } else {
            next_boundary(text, edge)
        };
    }
    if backwards {
        edge..cursor
    } else {
        cursor..edge
    }
}

fn move_horizontal(text: &str, mut cursor: usize, count: usize, right: bool) -> usize {
    let line_start = line_start(text, cursor);
    let line_end = line_end(text, cursor);
    for _ in 0..count.min(text.len().saturating_add(1)) {
        let next = if right {
            if cursor >= line_end {
                break;
            }
            next_boundary(text, cursor)
        } else {
            if cursor <= line_start {
                break;
            }
            previous_boundary(text, cursor)
        };
        if next == cursor || (right && next >= line_end) {
            break;
        }
        cursor = next;
    }
    normal_cursor(text, cursor)
}

fn move_vertical(text: &str, cursor: usize, count: usize, down: bool) -> usize {
    let start = line_start(text, cursor);
    let column = text[start..cursor].graphemes(true).count();
    move_vertical_to_column(text, cursor, count, down, column)
}

fn move_vertical_to_column(
    text: &str,
    cursor: usize,
    count: usize,
    down: bool,
    column: usize,
) -> usize {
    let mut target = line_start(text, cursor);
    if down {
        for _ in 0..count.min(line_count(text)) {
            if line_after(text, target) == target {
                break;
            }
            target = line_after(text, target);
        }
    } else {
        for _ in 0..count.min(line_count(text)) {
            if target == 0 {
                break;
            }
            target = line_start(text, target.saturating_sub(1));
        }
    }
    let end = line_end(text, target);
    if target == end {
        return target;
    }
    target
        + text[target..end]
            .grapheme_indices(true)
            .nth(column)
            .map(|(offset, _)| offset)
            .unwrap_or_else(|| previous_boundary(text, end) - target)
}

fn find_forward_n(
    text: &str,
    cursor: usize,
    target: char,
    kind: FindKind,
    count: usize,
) -> Option<usize> {
    let mut search_cursor = cursor;
    let mut found = None;
    let end = line_end(text, cursor);
    for _ in 0..count.min(text.len().saturating_add(1)) {
        let next = find_through_target(text, search_cursor, target, end)?;
        found = Some(if kind == FindKind::Till {
            previous_boundary(text, next)
        } else {
            next
        });
        search_cursor = next;
    }
    found
}

fn find_through_target(text: &str, cursor: usize, target: char, end: usize) -> Option<usize> {
    let mut offset = next_boundary(text, cursor);
    while offset < end {
        if grapheme_at(text, offset).is_some_and(|grapheme| grapheme.starts_with(target)) {
            return Some(offset);
        }
        offset = next_boundary(text, offset);
    }
    None
}

fn line_for_number(text: &str, number: usize) -> usize {
    let mut line = 1;
    let mut offset = 0;
    while line < number && offset < text.len() {
        if text.as_bytes()[offset] == b'\n' {
            line += 1;
        }
        offset += 1;
    }
    offset.min(text.len())
}

fn matching_pair(text: &str, cursor: usize) -> Option<usize> {
    let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
    let current = grapheme_at(text, cursor)?.chars().next()?;
    if let Some((open, close)) = pairs.iter().find(|(open, _)| *open == current) {
        let mut depth = 0;
        let mut offset = cursor;
        while offset < text.len() {
            let value = grapheme_at(text, offset)?.chars().next()?;
            if value == *open {
                depth += 1;
            } else if value == *close {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            offset = next_boundary(text, offset);
        }
    }
    if let Some((open, close)) = pairs.iter().find(|(_, close)| *close == current) {
        let mut depth = 0;
        let mut offset = cursor;
        loop {
            let value = grapheme_at(text, offset)?.chars().next()?;
            if value == *close {
                depth += 1;
            } else if value == *open {
                depth -= 1;
                if depth == 0 {
                    return Some(offset);
                }
            }
            if offset == 0 {
                break;
            }
            offset = previous_boundary(text, offset);
        }
    }
    None
}

fn object_for_key(value: &str, around: bool) -> Option<Object> {
    let key = value.chars().next()?;
    Some(match key {
        'w' => Object::Word { around },
        '"' | '\'' | '`' => Object::Quote { around, quote: key },
        '(' | '[' | '{' => {
            let close = match key {
                '(' => ')',
                '[' => ']',
                _ => '}',
            };
            Object::Pair {
                around,
                open: key,
                close,
            }
        }
        'p' => Object::Paragraph,
        _ => return None,
    })
}

fn text_object_range_count(
    text: &str,
    cursor: usize,
    object: Object,
    count: usize,
) -> Option<Range<usize>> {
    let mut range = text_object_range(text, cursor, object)?;
    let Object::Word { around } = object else {
        return Some(range);
    };
    let mut at = range.end;
    for _ in 1..count.min(text.len().saturating_add(1)) {
        while at < text.len() && grapheme_at(text, at).is_some_and(|g| word_class(g) == 0) {
            at = next_boundary(text, at);
        }
        if at >= text.len() {
            break;
        }
        let next = text_object_range(text, at, Object::Word { around })?;
        range.end = next.end;
        at = next.end;
    }
    Some(range)
}

fn text_object_range(text: &str, cursor: usize, object: Object) -> Option<Range<usize>> {
    match object {
        Object::Word { around } => {
            let mut at = cursor.min(text.len());
            let mut class = grapheme_at(text, at).map(word_class).unwrap_or(0);
            if class == 0 {
                while at < text.len()
                    && grapheme_at(text, at).is_some_and(|grapheme| word_class(grapheme) == 0)
                {
                    at = next_boundary(text, at);
                }
                if at == text.len() {
                    at = move_word_backward(text, cursor, 1);
                }
                class = grapheme_at(text, at).map(word_class)?;
            }
            let mut start = at;
            while start > 0 {
                let previous = previous_boundary(text, start);
                if grapheme_at(text, previous).is_some_and(|grapheme| word_class(grapheme) == class)
                {
                    start = previous;
                } else {
                    break;
                }
            }
            let mut end = next_boundary(text, at);
            while end < text.len()
                && grapheme_at(text, end).is_some_and(|grapheme| word_class(grapheme) == class)
            {
                end = next_boundary(text, end);
            }
            if !around {
                return Some(start..end);
            }
            let mut after = end;
            while after < text.len()
                && grapheme_at(text, after).is_some_and(|grapheme| word_class(grapheme) == 0)
            {
                after = next_boundary(text, after);
            }
            if after > end {
                Some(start..after)
            } else {
                let mut before = start;
                while before > 0 {
                    let previous = previous_boundary(text, before);
                    if grapheme_at(text, previous).is_some_and(|grapheme| word_class(grapheme) == 0)
                    {
                        before = previous;
                    } else {
                        break;
                    }
                }
                Some(before..end)
            }
        }
        Object::Quote { around, quote } => {
            let mut left = None;
            let mut offset = 0;
            while offset <= cursor.min(text.len()) {
                if grapheme_at(text, offset).is_some_and(|g| g == quote.to_string()) {
                    left = Some(offset);
                }
                offset = next_boundary(text, offset);
                if offset == text.len() {
                    break;
                }
            }
            let left = left?;
            let right = text[left + quote.len_utf8()..]
                .find(quote)
                .map(|index| left + quote.len_utf8() + index)?;
            if around {
                Some(left..right + quote.len_utf8())
            } else {
                Some(left + quote.len_utf8()..right)
            }
        }
        Object::Pair {
            around,
            open,
            close,
        } => {
            let pair = find_enclosing_pair(text, cursor, open, close)?;
            if around {
                Some(pair.0..pair.1 + close.len_utf8())
            } else {
                Some(pair.0 + open.len_utf8()..pair.1)
            }
        }
        Object::Paragraph => {
            let mut start = line_start(text, cursor);
            while start > 0 {
                let previous_end = start.saturating_sub(1);
                let previous_start = line_start(text, previous_end);
                if text[previous_start..previous_end].trim().is_empty() {
                    break;
                }
                start = previous_start;
            }
            let mut end = line_after(text, cursor);
            while end < text.len() && !text[end..line_end(text, end)].trim().is_empty() {
                end = line_after(text, end);
            }
            Some(start..end)
        }
    }
}

fn find_enclosing_pair(
    text: &str,
    cursor: usize,
    open: char,
    close: char,
) -> Option<(usize, usize)> {
    let mut candidate = None;
    let mut stack = Vec::new();
    for (offset, grapheme) in text.grapheme_indices(true) {
        let value = grapheme.chars().next()?;
        if value == open {
            stack.push(offset);
        } else if value == close {
            if let Some(start) = stack.pop() {
                let is_narrower = match candidate {
                    Some((existing_start, existing_end)) => {
                        offset - start < existing_end - existing_start
                    }
                    None => true,
                };
                if start <= cursor && cursor <= offset && is_narrower {
                    candidate = Some((start, offset));
                }
            }
        }
    }
    candidate
}

fn insert_position(text: &str, cursor: usize, kind: InsertKind) -> usize {
    match kind {
        InsertKind::Before => cursor,
        InsertKind::After => next_boundary(text, cursor),
        InsertKind::LineStart => move_motion(text, cursor, Motion::LineFirstText, 1),
        InsertKind::OpenAbove => line_start(text, cursor),
        InsertKind::LineEnd => line_end(text, cursor),
        InsertKind::OpenBelow => line_after(text, cursor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> Key {
        Key::Text(value.to_string())
    }

    fn run(engine: &mut VimState, text: &mut String, cursor: &mut usize, value: &str) {
        let inputs = match value {
            "Escape" => vec![Key::Escape],
            "Enter" => vec![Key::Enter],
            "Backspace" => vec![Key::Backspace],
            _ => value
                .chars()
                .map(|character| key(&character.to_string()))
                .collect(),
        };
        let mut selection = engine
            .expected_selection
            .clone()
            .unwrap_or(*cursor..*cursor);
        for input in inputs {
            let outcome = engine.handle(text, *cursor, selection.clone(), input.clone(), None);
            if !outcome.handled {
                if let Key::Text(inserted) = input {
                    text.insert_str(*cursor, &inserted);
                    *cursor += inserted.len();
                    selection = *cursor..*cursor;
                }
            }
            for effect in outcome.effects {
                match effect {
                    Effect::Move {
                        cursor: _,
                        selection: next_selection,
                    } => {
                        *cursor = next_selection.end;
                        selection = next_selection;
                    }
                    Effect::Replace(edit) => {
                        assert!(text.is_char_boundary(edit.range.start));
                        assert!(text.is_char_boundary(edit.range.end));
                        text.replace_range(edit.range, &edit.replacement);
                        assert!(text.is_char_boundary(edit.cursor));
                        *cursor = edit.cursor;
                        selection = *cursor..*cursor;
                    }
                    Effect::Undo | Effect::Redo | Effect::Save | Effect::ClipboardWrite(_) => {}
                }
            }
        }
    }

    #[test]
    fn unicode_motions_and_counted_delete() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "nå 🙂 beta gamma".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "d");
        run(&mut engine, &mut text, &mut cursor, "2");
        run(&mut engine, &mut text, &mut cursor, "w");
        assert_eq!(text, "beta gamma");

        engine.reset(0);
        text = "e\u{301}x y".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "dw");
        assert_eq!(text, "y");
    }

    #[test]
    fn change_insert_and_dot_repeat_at_new_cursor() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one two".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "c");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "w");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        run(&mut engine, &mut text, &mut cursor, "l");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "new new");
    }

    #[test]
    fn open_line_inserts_below_and_dot_repeats_the_line_recipe() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "o");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "one\nnew\ntwo");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "one\nnew\nnew\ntwo");
    }

    #[test]
    fn visual_line_and_registers() {
        let registers = Arc::new(Mutex::new(RegisterBank::default()));
        let mut engine = VimState::with_registers(registers.clone());
        let mut text = "one\ntwo\nthree".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "V");
        run(&mut engine, &mut text, &mut cursor, "j");
        run(&mut engine, &mut text, &mut cursor, "y");
        assert_eq!(
            registers.lock().unwrap().unnamed.as_ref().unwrap().text,
            "one\ntwo\n"
        );
        run(&mut engine, &mut text, &mut cursor, "V");
        run(&mut engine, &mut text, &mut cursor, "j");
        run(&mut engine, &mut text, &mut cursor, "d");
        assert_eq!(text, "three");
    }

    #[test]
    fn backward_visual_selection_deletes_the_inclusive_graphemes() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "alpha beta".to_string();
        let mut cursor = 8;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "h");
        run(&mut engine, &mut text, &mut cursor, "d");
        assert_eq!(text, "alpha ba");
    }

    #[test]
    fn named_register_and_linewise_paste_at_unterminated_eof() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "\"");
        run(&mut engine, &mut text, &mut cursor, "a");
        run(&mut engine, &mut text, &mut cursor, "y");
        run(&mut engine, &mut text, &mut cursor, "y");
        run(&mut engine, &mut text, &mut cursor, "G");
        run(&mut engine, &mut text, &mut cursor, "\"");
        run(&mut engine, &mut text, &mut cursor, "a");
        run(&mut engine, &mut text, &mut cursor, "p");
        assert_eq!(text, "one\ntwo\none\n");
    }

    #[test]
    fn explicit_clipboard_register_is_the_only_clipboard_write() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let text = "hello".to_string();
        let mut cursor = 0;
        let mut selection = 0..0;
        let mut clipboard_written = false;
        engine.handle(&text, cursor, selection.clone(), key("y"), None);
        let ordinary_yank = engine.handle(&text, cursor, selection.clone(), key("w"), None);
        assert!(!ordinary_yank
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::ClipboardWrite(_))));
        for input in [key("\""), key("+"), key("y"), key("w")] {
            let outcome = engine.handle(&text, cursor, selection.clone(), input, None);
            for effect in outcome.effects {
                match effect {
                    Effect::Move {
                        cursor: next,
                        selection: next_selection,
                    } => {
                        cursor = next;
                        selection = next_selection;
                    }
                    Effect::ClipboardWrite(value) if value == "hello" => {
                        clipboard_written = true;
                    }
                    _ => {}
                }
            }
        }
        let _ = (cursor, selection);
        assert!(clipboard_written);
    }

    #[test]
    fn find_pairs_objects_and_missing_delimiters_are_safe() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "(nå) end".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "%");
        assert_eq!(cursor, 4);
        run(&mut engine, &mut text, &mut cursor, "d");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "\"");
        assert_eq!(text, "(nå) end");

        text = "(one) (two)".to_string();
        cursor = 1;
        engine.reset(cursor);
        run(&mut engine, &mut text, &mut cursor, "d");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "(");
        assert_eq!(text, "() (two)");

        text = "one (two".to_string();
        cursor = 0;
        engine.reset(cursor);
        run(&mut engine, &mut text, &mut cursor, "d");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "(");
        assert_eq!(text, "one (two");

        text = "(one)".to_string();
        cursor = 4;
        engine.reset(cursor);
        run(&mut engine, &mut text, &mut cursor, "%");
        assert_eq!(cursor, 0);
        engine.reset(4);
        cursor = 4;
        run(&mut engine, &mut text, &mut cursor, "d%");
        assert_eq!(text, "");
    }

    #[test]
    fn counted_find_operators_use_relative_motion_ranges() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one,two,three".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "d");
        run(&mut engine, &mut text, &mut cursor, "2");
        run(&mut engine, &mut text, &mut cursor, "f");
        run(&mut engine, &mut text, &mut cursor, ",");
        assert_eq!(text, "three");
    }

    #[test]
    fn search_wrap_and_command_cancel() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "alpha beta alpha".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "/");
        run(&mut engine, &mut text, &mut cursor, "alpha");
        run(&mut engine, &mut text, &mut cursor, "Enter");
        assert_eq!(cursor, 11);
        run(&mut engine, &mut text, &mut cursor, "n");
        assert_eq!(cursor, 0);
        run(&mut engine, &mut text, &mut cursor, "N");
        assert_eq!(cursor, 11);
        run(&mut engine, &mut text, &mut cursor, "/");
        run(&mut engine, &mut text, &mut cursor, "not found");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "alpha beta alpha");
        run(&mut engine, &mut text, &mut cursor, ":");
        run(&mut engine, &mut text, &mut cursor, "q");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(engine.mode(), Mode::Normal);
    }

    #[test]
    fn change_line_preserves_following_line_and_cw_keeps_separator() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "cc");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "new\ntwo");

        engine.reset(0);
        text = "one two".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "cw");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "new two");

        engine.reset(0);
        text = "one\ntwo\nthree".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "2cc");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "new\nthree");
    }

    #[test]
    fn failed_counted_find_and_pair_never_delete() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\nx".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "d%");
        assert_eq!(text, "one\nx");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "one\nx");
        run(&mut engine, &mut text, &mut cursor, "dfx");
        assert_eq!(text, "one\nx");
        text = "one x\nx".into();
        cursor = 0;
        engine.reset(cursor);
        run(&mut engine, &mut text, &mut cursor, "2dfx");
        assert_eq!(text, "one x\nx");
    }

    #[test]
    fn visual_grapheme_text_object_and_special_motions_keep_anchor() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "(one)\n two".to_string();
        let mut cursor = 4;
        run(&mut engine, &mut text, &mut cursor, "v");
        assert_eq!(engine.expected_selection, Some(4..5));
        run(&mut engine, &mut text, &mut cursor, "v");
        assert_eq!(engine.mode(), Mode::Normal);
        assert_eq!(cursor, 4);
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "%");
        assert_eq!(engine.mode(), Mode::Visual);
        assert_eq!(engine.expected_selection, Some(0..5));
        run(&mut engine, &mut text, &mut cursor, "d");
        assert_eq!(text, "\n two");

        engine.reset(0);
        text = "one two".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "w");
        assert_eq!(engine.mode(), Mode::Visual);
        assert_eq!(engine.expected_selection, Some(0..3));
        run(&mut engine, &mut text, &mut cursor, "d");
        assert_eq!(text, " two");
    }

    #[test]
    fn visual_find_and_file_motions_extend_the_existing_selection() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "abx\n  two".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "fx");
        assert_eq!(engine.expected_selection, Some(0..3));
        run(&mut engine, &mut text, &mut cursor, "gg");
        assert_eq!(engine.expected_selection, Some(0..1));
        run(&mut engine, &mut text, &mut cursor, "G");
        assert_eq!(engine.expected_selection, Some(0..7));

        engine.reset(0);
        text = "ab x".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "tx");
        assert_eq!(engine.expected_selection, Some(0..3));

        engine.reset(3);
        text = "abcx".into();
        cursor = 3;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "h");
        run(&mut engine, &mut text, &mut cursor, "fx");
        assert_eq!(engine.expected_selection, Some(3..4));
    }

    #[test]
    fn counted_motion_objects_search_history_and_dot_are_bounded() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one two three four five".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "2d2w");
        assert_eq!(text, "five");

        engine.reset(0);
        text = "one two three".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "d2iw");
        assert_eq!(text, " three");

        engine.reset(0);
        text = "one two three four".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "dw");
        run(&mut engine, &mut text, &mut cursor, "2.");
        assert_eq!(text, "four");

        engine.reset(0);
        text = "alpha beta alpha gamma alpha delta alpha".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "/alpha");
        run(&mut engine, &mut text, &mut cursor, "Enter");
        run(&mut engine, &mut text, &mut cursor, "2n");
        assert_eq!(cursor, 35);
        run(&mut engine, &mut text, &mut cursor, "2N");
        assert_eq!(cursor, 11);

        let undo = engine.handle(&text, cursor, cursor..cursor, key("3"), None);
        assert!(undo.handled);
        let undo = engine.handle(&text, cursor, cursor..cursor, key("u"), None);
        assert_eq!(
            undo.effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Undo))
                .count(),
            3
        );
        let redo = engine.handle(&text, cursor, cursor..cursor, Key::CtrlR, None);
        assert_eq!(
            redo.effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Redo))
                .count(),
            1
        );
    }

    #[test]
    fn linewise_paste_normalizes_unterminated_final_register_and_repeat() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo\nthree".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "G");
        run(&mut engine, &mut text, &mut cursor, "yy");
        run(&mut engine, &mut text, &mut cursor, "gg");
        run(&mut engine, &mut text, &mut cursor, "P");
        assert_eq!(text, "three\none\ntwo\nthree");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "three\nthree\none\ntwo\nthree");
    }

    #[test]
    fn insert_escape_does_not_cross_line_start() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo".to_string();
        let mut cursor = 4;
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(cursor, 4);
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(text, "one\nnewtwo");
        assert!(cursor >= 4 && cursor < text.len());
    }

    #[test]
    fn explicit_one_g_differs_from_bare_g() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "G");
        assert_eq!(&text[cursor..], "two");
        run(&mut engine, &mut text, &mut cursor, "1G");
        assert_eq!(cursor, 0);
    }

    #[test]
    fn delete_object_and_navigation_keep_relative_dot_recipe() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one two three".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "diw");
        run(&mut engine, &mut text, &mut cursor, "w");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "  three");

        engine.reset(0);
        text = "one two three".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "ciw");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        run(&mut engine, &mut text, &mut cursor, "w");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "new new three");
    }

    #[test]
    fn unicode_file_end_and_open_line_repeat_keep_boundaries() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\nå".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "G");
        assert!(text.is_char_boundary(cursor));

        text = "one\ntwo".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "o");
        run(&mut engine, &mut text, &mut cursor, "å");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        run(&mut engine, &mut text, &mut cursor, "gg");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert!(text.is_char_boundary(cursor));
    }

    #[test]
    fn visual_pending_find_counts_and_line_anchor_are_preserved() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "a h".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "vfh");
        assert_eq!(engine.expected_selection, Some(0..3));

        engine.reset(0);
        text = "a d".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "vfd");
        assert_eq!(engine.expected_selection, Some(0..3));

        engine.reset(0);
        text = "one two".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "vf");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.pending_find.is_none());
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "\"");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
        assert!(!engine.pending_register);
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "i");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert!(engine.pending_visual_text_object.is_none());

        engine.reset(0);
        text = (1..=12).map(|n| format!("line{n}\n")).collect();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "v10j");
        assert!(cursor > text.find("line10").unwrap());

        engine.reset(5);
        text = "one\ntwo x\nthree y".into();
        cursor = 4;
        run(&mut engine, &mut text, &mut cursor, "Vjf y");
        assert_eq!(engine.expected_selection.as_ref().unwrap().start, 4);
        run(&mut engine, &mut text, &mut cursor, "Escape");
        assert_eq!(engine.mode, Mode::Normal);
    }

    #[test]
    fn counted_repeat_is_one_atomic_edit_and_has_a_work_limit() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "a b c".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "x");
        run(&mut engine, &mut text, &mut cursor, "2.");
        assert_eq!(text, " c");

        engine.last_change = Some(Recipe::Insert {
            kind: InsertKind::Before,
            text: "x".into(),
        });
        let outcome = engine.repeat("a", 0, 2);
        assert_eq!(
            outcome
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Replace(_)))
                .count(),
            1
        );

        engine.last_change = Some(Recipe::Insert {
            kind: InsertKind::Before,
            text: String::new(),
        });
        let outcome = engine.repeat("a", 0, MAX_COUNT);
        assert!(outcome.effects.is_empty());

        engine.last_change = Some(Recipe::Insert {
            kind: InsertKind::Before,
            text: "x".repeat(MAX_EDIT_BYTES),
        });
        let outcome = engine.repeat("", 0, 20);
        assert!(outcome
            .effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Replace(_))));
        assert_eq!(
            outcome.status.as_ref().map(|status| status.text(false)),
            Some("Repeat exceeds the work limit".into())
        );
    }

    #[test]
    fn line_motion_change_and_counted_change_object_are_relative() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        let mut text = "one\ntwo\nthree".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "dj");
        assert_eq!(text, "three");

        engine.reset(0);
        text = "one two three four".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "c2iw");
        run(&mut engine, &mut text, &mut cursor, "new");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        run(&mut engine, &mut text, &mut cursor, "w");
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, "new new");

        engine.reset(0);
        text = "one two".into();
        cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "v");
        run(&mut engine, &mut text, &mut cursor, "l");
        run(&mut engine, &mut text, &mut cursor, "c");
        run(&mut engine, &mut text, &mut cursor, "x");
        run(&mut engine, &mut text, &mut cursor, "Escape");
        run(&mut engine, &mut text, &mut cursor, ".");
        let after_visual = text.clone();
        run(&mut engine, &mut text, &mut cursor, ".");
        assert_eq!(text, after_visual);
    }

    #[test]
    fn suspend_cancels_prefixes_but_keeps_insert_mode() {
        let mut engine = VimState::with_registers(Arc::new(Mutex::new(RegisterBank::default())));
        engine.last_change = Some(Recipe::Insert {
            kind: InsertKind::Before,
            text: "x".into(),
        });
        engine.pending_operator = Some((Operator::Delete, 1));
        engine.count = "12".into();
        engine.suspend();
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.pending_operator.is_none());
        assert!(engine.last_change.is_some());

        engine.mode = Mode::VisualLine;
        engine.visual_anchor = Some(0);
        engine.last_search = Some("two".into());
        engine.suspend();
        assert_eq!(engine.mode, Mode::Normal);
        assert!(engine.visual_anchor.is_none());
        assert_eq!(engine.last_search.as_deref(), Some("two"));

        let mut text = "one".to_string();
        let mut cursor = 0;
        run(&mut engine, &mut text, &mut cursor, "i");
        engine.suspend();
        assert_eq!(engine.mode, Mode::Insert);
        assert!(engine.insert.is_none());
        assert!(engine.last_change.is_none());
    }
}
