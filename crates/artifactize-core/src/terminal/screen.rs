//! VT state, not an output replay. avt restores both buffers, cursor, margins, modes and
//! unfinished escape sequences; retained history and incremental UTF-8 complete the snapshot.
use super::TerminalFrame;
use std::{collections::BTreeSet, fmt::Write, path::PathBuf};

// DEC private modes that change what the client sends rather than what the screen shows.
const X10_MOUSE: u16 = 9;
const MOUSE_PRESSES: u16 = 1000;
const MOUSE_DRAGS: u16 = 1002;
const MOUSE_MOTION: u16 = 1003;
const FOCUS_EVENTS: u16 = 1004;
const MOUSE_UTF8: u16 = 1005;
const MOUSE_SGR: u16 = 1006;
const MOUSE_URXVT: u16 = 1015;
const BRACKETED_PASTE: u16 = 2004;
/// A reattached client restores these input modes with the screen; avt keeps the rest.
const INPUT_MODES: [u16; 9] = [
    X10_MOUSE,
    MOUSE_PRESSES,
    MOUSE_DRAGS,
    MOUSE_MOTION,
    FOCUS_EVENTS,
    MOUSE_UTF8,
    MOUSE_SGR,
    MOUSE_URXVT,
    BRACKETED_PASTE,
];

/// A terminal retains ten thousand lines, independently of how long its host lives.
const SCROLLBACK_LINES: usize = 10_000;

#[derive(Default)]
struct Integration {
    cwd: Option<PathBuf>,
    input_modes: BTreeSet<u16>,
    keypad: bool,
}
impl vte::Perform for Integration {
    fn osc_dispatch(&mut self, params: &[&[u8]], _: bool) {
        if params.first() == Some(&b"7".as_slice())
            && let Some(value) = params.get(1).and_then(|v| std::str::from_utf8(v).ok())
            && let Ok(url) = url::Url::parse(value)
            && let Ok(path) = url.to_file_path()
        {
            self.cwd = Some(path);
        }
    }
    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _: bool, action: char) {
        if intermediates == b"?" && matches!(action, 'h' | 'l') {
            for param in params {
                // avt handles the rendering modes. These additional modes control client
                // keyboard, focus, mouse and paste input rather than screen cells.
                if let [mode] = param
                    && INPUT_MODES.contains(mode)
                {
                    if action == 'h' {
                        self.input_modes.insert(*mode);
                    } else {
                        self.input_modes.remove(mode);
                    }
                }
            }
        }
    }
    fn esc_dispatch(&mut self, _: &[u8], _: bool, byte: u8) {
        match byte {
            b'=' => self.keypad = true,
            b'>' => self.keypad = false,
            b'c' => {
                self.input_modes.clear();
                self.keypad = false;
            }
            _ => {}
        }
    }
}

pub(super) struct Screen {
    terminal: avt::Terminal,
    integration: Integration,
    parser: vte::Parser,
    utf8: Vec<u8>,
}
impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            terminal: avt::Terminal::with_scrollback(
                cols.into(),
                rows.into(),
                Some(SCROLLBACK_LINES),
            )
            .expect("dimensions parsed at the protocol boundary"),
            integration: Integration::default(),
            parser: vte::Parser::new(),
            utf8: Vec::new(),
        }
    }
    pub fn process(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.integration, data);
        self.utf8.extend_from_slice(data);
        let mut consumed = 0;
        loop {
            match std::str::from_utf8(&self.utf8[consumed..]) {
                Ok(text) => {
                    self.terminal.feed(text);
                    consumed = self.utf8.len();
                    break;
                }
                Err(error) => {
                    let end = consumed + error.valid_up_to();
                    self.terminal.feed(
                        std::str::from_utf8(&self.utf8[consumed..end]).expect("valid UTF-8 prefix"),
                    );
                    consumed = end;
                    if let Some(length) = error.error_len() {
                        self.terminal.feed('\u{fffd}');
                        consumed += length;
                    } else {
                        break;
                    }
                }
            }
        }
        self.utf8.drain(..consumed);
    }
    pub fn take_cwd(&mut self) -> Option<PathBuf> {
        self.integration.cwd.take()
    }
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.terminal.resize(cols.into(), rows.into());
    }
    pub fn snapshot(&self) -> TerminalFrame {
        let (cols, rows) = self.terminal.size();
        let primary = self.terminal.primary_screen();
        let history = primary.all_lines().count().saturating_sub(rows);
        let mut text = String::from("\x1bc");
        let mut wrapped = false;
        for line in primary.all_lines().take(history) {
            wrapped = line.is_wrapped();
            for cells in line.chunks(|left, right| left.pen() != right.pen()) {
                if let Some(first) = cells.first() {
                    pen(&mut text, first.pen());
                }
                for cell in cells {
                    text.push(cell.char());
                }
            }
            // A soft wrap is produced by the first character of the following row.
            if !line.is_wrapped() {
                text.push_str("\r\n");
            }
        }
        if history > 0 {
            if wrapped {
                text.push_str(" \r");
            }
            for _ in 1..rows {
                text.push_str("\r\n");
            }
        }
        text.push_str("\x1b[0m\x1b[H\x1b[2J");
        for mode in &self.integration.input_modes {
            let _ = write!(text, "\x1b[?{mode}h");
        }
        if self.integration.keypad {
            text.push_str("\x1b=");
        }
        text.push_str(&self.terminal.dump());
        let mut data = text.into_bytes();
        data.extend_from_slice(&self.utf8);
        TerminalFrame::Snapshot {
            data,
            cols: cols as u16,
            rows: rows as u16,
        }
    }
}

fn pen(text: &mut String, pen: &avt::Pen) {
    text.push_str("\x1b[0");
    for (enabled, code) in [
        (pen.is_bold(), 1),
        (pen.is_faint(), 2),
        (pen.is_italic(), 3),
        (pen.is_underline(), 4),
        (pen.is_blink(), 5),
        (pen.is_inverse(), 7),
        (pen.is_strikethrough(), 9),
    ] {
        if enabled {
            let _ = write!(text, ";{code}");
        }
    }
    for (color, code) in [(pen.foreground(), 38), (pen.background(), 48)] {
        match color {
            Some(avt::Color::Indexed(index)) => {
                let _ = write!(text, ";{code};5;{index}");
            }
            Some(avt::Color::RGB(rgb)) => {
                let _ = write!(text, ";{code};2;{};{};{}", rgb.r, rgb.g, rgb.b);
            }
            None => {}
        }
    }
    text.push('m');
}
