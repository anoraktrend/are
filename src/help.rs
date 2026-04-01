//! Help system – ported from src/help.c
//!
//! Reads the help file (plain text with form-feeds as section separators)
//! and displays it in the terminal, allowing the user to navigate topics.
//! Uses crossterm instead of ncurses.

use std::io::{self, BufRead, BufReader, Cursor, Seek, SeekFrom};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent},
    execute, style,
    terminal::{self, Clear, ClearType},
};

// Embed the help file directly into the binary for a self-contained executable.
const HELP_CONTENT: &str = include_str!("help.ae");

// ──────────────────────────────────────────────────────────────────────────────
// Public entry point
// ──────────────────────────────────────────────────────────────────────────────

/// Display the help system.
pub fn help(_help_file: Option<&str>) {
    let mut reader = BufReader::new(Cursor::new(HELP_CONTENT));

    let mut stdout = io::stdout();
    let _ = terminal::enable_raw_mode();
    let _ = execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0));

    loop {
        // Display one "page" (up to the next form-feed or EOF)
        let more = display_page(&mut reader, &mut stdout);
        if !more {
            break; // EOF reached
        }
        // Ask whether to continue / enter topic
        match prompt_topic(&mut stdout) {
            TopicAction::Quit => break,
            TopicAction::Next => {
                // Tab: go back to start
                let _ = reader.seek(SeekFrom::Start(0));
            }
            TopicAction::Search(topic) => {
                if !seek_to_topic(&mut reader, &topic) {
                    show_topic_error(&mut stdout, &topic);
                    // Reset to start
                    let _ = reader.seek(SeekFrom::Start(0));
                }
            }
        }
    }

    let _ = terminal::disable_raw_mode();
    let _ = execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0));
}

// ──────────────────────────────────────────────────────────────────────────────
// Display one page (until form-feed or EOF)
// ──────────────────────────────────────────────────────────────────────────────

/// Output lines from `reader` until a form-feed (0x0C) line or EOF.
/// Returns `true` if a form-feed was encountered (more pages available).
fn display_page<R: BufRead + Seek>(reader: &mut R, stdout: &mut io::Stdout) -> bool {
    use crossterm::style::Print;

    let (_cols, rows) = terminal::size().unwrap_or((80, 24));
    let _ = execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0));

    let mut row = 0u16;
    let mut found_ff = false;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }
        // Check for form-feed character
        if line.starts_with('\x0C') {
            found_ff = true;
            break;
        }
        // Strip trailing newline for display
        let display = line.trim_end_matches('\n').trim_end_matches('\r');
        let _ = execute!(stdout, cursor::MoveTo(0, row), Print(display),);
        row += 1;
        if row >= rows - 1 {
            // Show "more" prompt and wait for a key
            let _ = execute!(
                stdout,
                cursor::MoveTo(0, rows - 1),
                style::SetAttribute(style::Attribute::Reverse),
                Print("--- Press any key for more ---"),
                style::SetAttribute(style::Attribute::Reset),
            );
            wait_for_key();
            let _ = execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0));
            row = 0;
        }
    }
    found_ff
}

// ──────────────────────────────────────────────────────────────────────────────
// Topic prompt
// ──────────────────────────────────────────────────────────────────────────────

enum TopicAction {
    Quit,
    Next,
    Search(String),
}

fn prompt_topic(stdout: &mut io::Stdout) -> TopicAction {
    use crossterm::style::Print;
    let (_, rows) = terminal::size().unwrap_or((80, 24));
    let _ = execute!(
        stdout,
        cursor::MoveTo(0, rows - 1),
        Clear(ClearType::CurrentLine),
        Print("Topic (Enter=quit, Tab=restart): "),
        cursor::Show,
    );

    let mut input = String::new();
    loop {
        if let Ok(Event::Key(KeyEvent { code, .. })) = event::read() {
            match code {
                KeyCode::Enter => {
                    if input.is_empty() {
                        return TopicAction::Quit;
                    } else {
                        return TopicAction::Search(input);
                    }
                }
                KeyCode::Tab => return TopicAction::Next,
                KeyCode::Esc | KeyCode::Char('q') => return TopicAction::Quit,
                KeyCode::Backspace => {
                    if !input.is_empty() {
                        input.pop();
                        let _ =
                            execute!(stdout, cursor::MoveLeft(1), Print(' '), cursor::MoveLeft(1));
                    }
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    let _ = execute!(stdout, Print(c));
                }
                _ => {}
            }
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Seek to a topic within the help file
// ──────────────────────────────────────────────────────────────────────────────

/// Seek `reader` to the line *after* the first line that starts with `topic`.
/// Returns `true` on success.
fn seek_to_topic<R: BufRead + Seek>(reader: &mut R, topic: &str) -> bool {
    let _ = reader.seek(SeekFrom::Start(0));
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return false,
            Err(_) => return false,
            Ok(_) => {}
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.starts_with(topic) {
            // Consume one more line (the section header itself was the match)
            let mut _skip = String::new();
            let _ = reader.read_line(&mut _skip);
            return true;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Error display
// ──────────────────────────────────────────────────────────────────────────────

fn show_topic_error(stdout: &mut io::Stdout, topic: &str) {
    use crossterm::style::Print;
    let (_, rows) = terminal::size().unwrap_or((80, 24));
    let _ = execute!(
        stdout,
        cursor::MoveTo(0, rows - 2),
        Clear(ClearType::CurrentLine),
        style::SetAttribute(style::Attribute::Reverse),
        Print(format!("Topic '{}' not found.", topic)),
        style::SetAttribute(style::Attribute::Reset),
    );
    wait_for_key();
}

fn wait_for_key() {
    loop {
        if let Ok(Event::Key(_)) = event::read() {
            break;
        }
    }
}
