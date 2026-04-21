use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
    ExecutableCommand,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, Stdout, Write},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

static TERMINAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static KEYBOARD_ENHANCEMENT_PUSHED: AtomicBool = AtomicBool::new(false);

pub struct TerminalSession {
    terminal: Tui,
    keyboard_enhancement_pushed: bool,
    restored: bool,
}

impl TerminalSession {
    pub fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }

        self.restored = true;
        TERMINAL_ACTIVE.store(false, Ordering::SeqCst);

        let pop_keyboard = self.keyboard_enhancement_pushed
            && KEYBOARD_ENHANCEMENT_PUSHED.swap(false, Ordering::SeqCst);
        self.keyboard_enhancement_pushed = false;

        let mut first_err = None;
        remember_io_result(self.terminal.show_cursor(), &mut first_err);
        remember_io_result(
            restore_with_writer(self.terminal.backend_mut(), pop_keyboard),
            &mut first_err,
        );

        match first_err {
            Some(err) => Err(err.into()),
            None => Ok(()),
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

impl Deref for TerminalSession {
    type Target = Tui;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for TerminalSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

pub fn init() -> Result<TerminalSession> {
    enable_raw_mode()?;
    TERMINAL_ACTIVE.store(true, Ordering::SeqCst);

    let mut stdout = io::stdout();
    if let Err(err) = execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    ) {
        let _ = restore();
        return Err(err.into());
    }

    // Enable keyboard enhancement when the terminal supports it (kitty protocol).
    // This lets us distinguish Shift+Enter from plain Enter, Shift alone, etc.
    let mut keyboard_enhancement_pushed = false;
    if supports_keyboard_enhancement().unwrap_or(false)
        && execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok()
    {
        keyboard_enhancement_pushed = true;
        KEYBOARD_ENHANCEMENT_PUSHED.store(true, Ordering::SeqCst);
    }

    let backend = CrosstermBackend::new(stdout);
    match Terminal::new(backend) {
        Ok(terminal) => Ok(TerminalSession {
            terminal,
            keyboard_enhancement_pushed,
            restored: false,
        }),
        Err(err) => {
            let _ = restore();
            Err(err.into())
        }
    }
}

pub fn restore() -> Result<()> {
    if !TERMINAL_ACTIVE.swap(false, Ordering::SeqCst)
        && !KEYBOARD_ENHANCEMENT_PUSHED.load(Ordering::SeqCst)
    {
        return Ok(());
    }

    let pop_keyboard = KEYBOARD_ENHANCEMENT_PUSHED.swap(false, Ordering::SeqCst);
    let mut stdout = io::stdout();
    restore_with_writer(&mut stdout, pop_keyboard)?;
    Ok(())
}

fn restore_with_writer<W: Write + ?Sized>(writer: &mut W, pop_keyboard: bool) -> io::Result<()> {
    let mut first_err = None;

    // Only pop when this process successfully pushed the kitty keyboard protocol.
    // An unpaired pop can disturb a parent application that already owned the mode.
    if pop_keyboard {
        remember_io_result(
            writer.execute(PopKeyboardEnhancementFlags).map(|_| ()),
            &mut first_err,
        );
    }
    remember_io_result(disable_raw_mode(), &mut first_err);
    remember_io_result(
        writer.execute(LeaveAlternateScreen).map(|_| ()),
        &mut first_err,
    );
    remember_io_result(
        writer.execute(DisableMouseCapture).map(|_| ()),
        &mut first_err,
    );
    remember_io_result(
        writer.execute(DisableBracketedPaste).map(|_| ()),
        &mut first_err,
    );
    remember_io_result(writer.flush(), &mut first_err);

    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn remember_io_result(result: io::Result<()>, first_err: &mut Option<io::Error>) {
    if let Err(err) = result {
        first_err.get_or_insert(err);
    }
}
