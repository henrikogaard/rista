use crate::{settings::Language, terminal_session::Session};
use gpui_kit::component::{v_flex, ActiveTheme};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{cell::Cell, ops::Range, rc::Rc};

pub struct Terminal {
    pub visible: bool,
    screen: Rc<vt100::Screen>,
    screen_generation: u64,
    font_family: String,
    font_size: f32,
    line_height: f32,
    session: Session,
    focus: FocusHandle,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    cell_width: f32,
    marked: String,
    error: Option<String>,
    language: Language,
    _poll: Task<()>,
}

impl Terminal {
    pub fn new(
        session: Session,
        settings: &crate::settings::Settings,
        cx: &mut Context<Self>,
    ) -> Self {
        let screen = Rc::new(session.snapshot());
        let poll = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let mut generation = 0;
            let mut delay = 16;
            loop {
                smol::Timer::after(std::time::Duration::from_millis(delay)).await;
                match this.update(&mut *cx, |this, cx| {
                    let bounds = this.bounds.get();
                    let rows = (f32::from(bounds.size.height) / this.line_height)
                        .floor()
                        .clamp(2., 200.) as u16;
                    let cols = (f32::from(bounds.size.width) / this.cell_width)
                        .floor()
                        .clamp(10., 400.) as u16;
                    if this.visible
                        && bounds.size.width > px(0.)
                        && this.session.size() != (rows, cols)
                        && this.session.resize(rows, cols).is_err()
                    {
                        this.error = Some(
                            this.language
                                .text(
                                    "Could not resize terminal",
                                    "Kunne ikke endre terminalstørrelsen",
                                )
                                .into(),
                        );
                    }
                    let next = this.session.generation();
                    if next != generation {
                        generation = next;
                        if this.visible {
                            cx.notify();
                        }
                    }
                    this.visible
                }) {
                    Ok(true) => delay = 16,
                    Ok(false) => delay = 200,
                    Err(_) => break,
                }
            }
        });
        Self {
            visible: true,
            screen,
            screen_generation: u64::MAX,
            font_family: settings.terminal_font.clone(),
            font_size: settings.terminal_font_size.clamp(10., 24.),
            line_height: settings.terminal_font_size.clamp(10., 24.) * 1.45,
            session,
            focus: cx.focus_handle(),
            bounds: Rc::new(Cell::new(Bounds::default())),
            cell_width: 7.8,
            marked: String::new(),
            error: None,
            language: settings.language,
            _poll: poll,
        }
    }

    pub fn apply_settings(&mut self, settings: &crate::settings::Settings, cx: &mut Context<Self>) {
        self.font_family = settings.terminal_font.clone();
        self.font_size = settings.terminal_font_size.clamp(10., 24.);
        self.line_height = self.font_size * 1.45;
        self.language = settings.language;
        cx.notify();
    }

    fn send(&mut self, bytes: Vec<u8>, cx: &mut Context<Self>) {
        if self.session.send(bytes).is_err() {
            self.error = Some(
                self.language
                    .text(
                        "Terminal input unavailable",
                        "Terminalinndata er utilgjengelig",
                    )
                    .into(),
            );
        }
        cx.notify();
    }

    pub fn interrupt(&mut self, cx: &mut Context<Self>) {
        self.send(vec![3], cx);
    }
    pub fn copy_output(&self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.session.snapshot().contents(),
        ));
    }

    fn paste_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.replace('\u{1b}', "").replace("\r\n", "\n");
        let bytes = if self.session.input_modes().1 {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace('\n', "\r").into_bytes()
        };
        self.send(bytes, cx);
    }

    fn key(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &ev.keystroke;
        if key.modifiers.platform {
            match key.key.as_str() {
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                        self.paste_text(&text, cx);
                    }
                }
                "c" => self.copy_output(cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = key_bytes(key, self.session.input_modes().0) {
            self.marked.clear();
            self.send(bytes, cx);
            cx.stop_propagation();
        }
    }
}

fn key_bytes(key: &Keystroke, application_cursor: bool) -> Option<Vec<u8>> {
    let name = key.key.as_str();
    if key.modifiers.control && name.len() == 1 {
        let byte = name.as_bytes()[0];
        if byte.is_ascii_alphabetic() || (b'@'..=b'_').contains(&byte) {
            return Some(vec![byte.to_ascii_uppercase() & 0x1f]);
        }
    }
    let sequence = match name {
        "enter" => "\r",
        "backspace" => "\x7f",
        "escape" => "\x1b",
        "tab" if key.modifiers.shift => "\x1b[Z",
        "tab" => "\t",
        "up" | "down" | "right" | "left" | "home" | "end" => {
            let code = match name {
                "up" => 'A',
                "down" => 'B',
                "right" => 'C',
                "left" => 'D',
                "home" => 'H',
                _ => 'F',
            };
            let modifier = 1
                + u8::from(key.modifiers.shift)
                + 2 * u8::from(key.modifiers.alt)
                + 4 * u8::from(key.modifiers.control);
            return Some(
                if modifier > 1 {
                    format!("\x1b[1;{modifier}{code}")
                } else if application_cursor {
                    format!("\x1bO{code}")
                } else {
                    format!("\x1b[{code}")
                }
                .into_bytes(),
            );
        }
        "delete" => "\x1b[3~",
        "pageup" => "\x1b[5~",
        "pagedown" => "\x1b[6~",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        "f3" => "\x1bOR",
        "f4" => "\x1bOS",
        "f5" => "\x1b[15~",
        "f6" => "\x1b[17~",
        "f7" => "\x1b[18~",
        "f8" => "\x1b[19~",
        "f9" => "\x1b[20~",
        "f10" => "\x1b[21~",
        "f11" => "\x1b[23~",
        "f12" => "\x1b[24~",
        _ => return None,
    };
    let mut bytes = Vec::new();
    if key.modifiers.alt {
        bytes.push(0x1b);
    }
    bytes.extend_from_slice(sequence.as_bytes());
    Some(bytes)
}

impl Focusable for Terminal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for Terminal {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let text: Vec<_> = self.marked.encode_utf16().collect();
        let range = range.start.min(text.len())..range.end.min(text.len());
        actual.replace(range.clone());
        Some(String::from_utf16_lossy(&text[range]))
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let end = self.marked.encode_utf16().count();
        Some(UTF16Selection {
            range: end..end,
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        (!self.marked.is_empty()).then_some(0..self.marked.encode_utf16().count())
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked.clear();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked.clear();
        self.send(text.as_bytes().to_vec(), cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = text.to_string();
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (row, col) = self.screen.cursor_position();
        Some(Bounds::new(
            bounds.origin
                + point(
                    px(col as f32 * self.cell_width),
                    px(row as f32 * self.line_height),
                ),
            size(px(self.cell_width), px(self.line_height)),
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
    fn paste(&mut self, item: ClipboardItem, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = item.text() {
            self.paste_text(&text, cx);
        }
    }
}

fn terminal_color(color: vt100::Color, default: Hsla, palette: &[Hsla; 16]) -> Hsla {
    // Extended ANSI colors are document content supplied by the process, not UI chrome.
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => {
            rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)).into()
        }
        vt100::Color::Idx(index) if index < 16 => palette[index as usize],
        vt100::Color::Idx(index) if index >= 232 => {
            let c = 8 + 10 * u32::from(index - 232);
            rgb((c << 16) | (c << 8) | c).into()
        }
        vt100::Color::Idx(index) => {
            let i = index - 16;
            let c = |v: u8| if v == 0 { 0 } else { 55 + 40 * u32::from(v) };
            rgb((c(i / 36) << 16) | (c((i / 6) % 6) << 8) | c(i % 6)).into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::key_bytes;
    use gpui_kit::Keystroke;
    #[test]
    fn keys_follow_terminal_modes_and_control_sequences() {
        assert_eq!(
            key_bytes(&Keystroke::parse("ctrl-c").unwrap(), false),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes(&Keystroke::parse("up").unwrap(), false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(&Keystroke::parse("up").unwrap(), true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_bytes(&Keystroke::parse("shift-tab").unwrap(), false),
            Some(b"\x1b[Z".to_vec())
        );
        assert!(key_bytes(&Keystroke::parse("a").unwrap(), false).is_none());
    }
}

impl Render for Terminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let foreground = theme.foreground;
        let background = theme.group_box;
        let palette = [
            background,
            theme.danger,
            theme.success,
            theme.warning,
            theme.blue,
            theme.magenta,
            theme.cyan,
            foreground,
            theme.muted_foreground,
            theme.danger,
            theme.success,
            theme.warning,
            theme.blue,
            theme.magenta,
            theme.cyan,
            foreground,
        ];
        let font = font(self.font_family.clone());
        let font_size = self.font_size;
        let line_height = self.line_height;
        let run = TextRun {
            len: 1,
            font: font.clone(),
            color: foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.cell_width = f32::from(
            window
                .text_system()
                .shape_line("M".into(), px(font_size), &[run], None)
                .width,
        )
        .max(1.);
        let cell_width = self.cell_width;
        let generation = self.session.generation();
        if self.screen_generation != generation {
            self.screen = Rc::new(self.session.snapshot());
            self.screen_generation = generation;
        }
        let screen = self.screen.clone();
        let bounds_slot = self.bounds.clone();
        let focus = self.focus.clone();
        let input = cx.entity();
        let focused = self.focus.is_focused(window);
        let marked = self.marked.clone();
        let canvas = canvas(
            move |bounds, _, _| {
                bounds_slot.set(bounds);
            },
            move |bounds, _, window, cx| {
                window.handle_input(&focus, ElementInputHandler::new(bounds, input), cx);
                let (rows, cols) = screen.size();
                let cursor = screen.cursor_position();
                for row in 0..rows {
                    for col in 0..cols {
                        let Some(cell) = screen.cell(row, col) else {
                            continue;
                        };
                        if cell.is_wide_continuation() {
                            continue;
                        }
                        let origin = bounds.origin
                            + point(px(col as f32 * cell_width), px(row as f32 * line_height));
                        let is_cursor = focused
                            && !screen.hide_cursor()
                            && screen.scrollback() == 0
                            && cursor == (row, col);
                        let mut fg = terminal_color(cell.fgcolor(), foreground, &palette);
                        let mut bg = terminal_color(cell.bgcolor(), background, &palette);
                        if cell.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        if is_cursor {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        let width = cell_width * if cell.is_wide() { 2. } else { 1. };
                        if bg != background {
                            window.paint_quad(fill(
                                Bounds::new(origin, size(px(width), px(line_height))),
                                bg,
                            ));
                        }
                        let text = if is_cursor && !marked.is_empty() {
                            marked.clone()
                        } else {
                            cell.contents()
                        };
                        if text.is_empty() {
                            continue;
                        }
                        let mut font = font.clone();
                        if cell.bold() {
                            font.weight = FontWeight::BOLD;
                        }
                        if cell.italic() {
                            font.style = FontStyle::Italic;
                        }
                        let run = TextRun {
                            len: text.len(),
                            font,
                            color: fg,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(
                            text.into(),
                            px(font_size),
                            &[run],
                            None,
                        );
                        let _ =
                            line.paint(origin, px(line_height), TextAlign::Left, None, window, cx);
                        if cell.underline() {
                            window.paint_quad(fill(
                                Bounds::new(
                                    origin + point(px(0.), px(line_height - 2.)),
                                    size(px(width), px(1.)),
                                ),
                                fg,
                            ));
                        }
                    }
                }
            },
        )
        .size_full();
        let status = self
            .error
            .clone()
            .or_else(|| {
                self.session.error().map(|_| {
                    self.language
                        .text("Terminal process failed", "Terminalprosessen mislyktes")
                        .into()
                })
            })
            .or_else(|| {
                self.session.exited().then(|| {
                    self.language
                        .text(
                            "Process exited · launch a tool to start again",
                            "Prosessen er avsluttet · start et verktøy på nytt",
                        )
                        .into()
                })
            });
        v_flex()
            .size_full()
            .bg(background)
            .child(
                div()
                    .id("terminal-grid")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .track_focus(&self.focus)
                    .key_context("RistaTerminal")
                    .on_key_down(cx.listener(Self::key))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.focus.focus(window, cx)),
                    )
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        let y = f32::from(ev.delta.pixel_delta(px(this.line_height)).y);
                        this.session.scroll((y / this.line_height).round() as i32);
                        cx.notify();
                        cx.stop_propagation();
                    }))
                    .child(canvas),
            )
            .when_some(status, |d, status| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(status),
                )
            })
    }
}
