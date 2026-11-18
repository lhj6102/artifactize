//! Own console modes, normalized input and restoration for both terminal screens.
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as RawEvent, EventStream, KeyEvent, KeyEventKind, MouseEvent,
};
use futures_util::{Stream, StreamExt, future};
use ratatui::DefaultTerminal;
use std::io;

pub(crate) enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize,
}

pub(crate) fn events() -> impl Stream<Item = io::Result<Event>> {
    EventStream::new().filter_map(|event| {
        // Terminals differ in whether they report release and repeat. Dispatch only presses.
        future::ready(match event {
            Ok(RawEvent::Key(key)) if key.kind == KeyEventKind::Press => Some(Ok(Event::Key(key))),
            Ok(RawEvent::Mouse(mouse)) => Some(Ok(Event::Mouse(mouse))),
            Ok(RawEvent::Paste(text)) => Some(Ok(Event::Paste(text))),
            Ok(RawEvent::Resize(_, _)) => Some(Ok(Event::Resize)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
    })
}

pub(crate) fn init() -> io::Result<DefaultTerminal> {
    // Ratatui's panic hook restores the terminal before printing a panic.
    let terminal = ratatui::try_init();
    if terminal.is_err() {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    terminal
}

pub(crate) fn restore() -> Result<(), String> {
    ratatui::try_restore().map_err(|error| error.to_string())
}

/// Restore every input protocol when leaving, including errors, signals and panic unwinding.
pub(crate) struct Input;

impl Input {
    pub(crate) fn capture(&self, mouse: bool) -> Result<(), String> {
        let mut out = io::stdout();
        if mouse {
            crossterm::execute!(out, EnableMouseCapture)
        } else {
            crossterm::execute!(out, DisableMouseCapture)
        }
        .map_err(|error| error.to_string())?;
        crossterm::execute!(out, EnableBracketedPaste).map_err(|error| error.to_string())
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = crossterm::execute!(out, DisableMouseCapture);
        let _ = crossterm::execute!(out, DisableBracketedPaste);
    }
}

pub(crate) fn repaint(terminal: &mut DefaultTerminal) -> Result<(), String> {
    crossterm::execute!(
        io::stdout(),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )
    .map_err(|error| error.to_string())?;
    terminal.swap_buffers();
    Ok(())
}
