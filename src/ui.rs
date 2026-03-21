// UI module using crossterm for terminal control

use crossterm::{
    terminal::{self, ClearType},
    execute,
    style::{self, Color, SetForegroundColor, ResetColor, SetAttribute, Attribute},
    cursor,
    event::{self, Event, KeyEvent},
};
use std::io::{stdout, Write};

use crate::highlighting::TokenKind;
use crate::editor_state::TextLine;

// ────────────────────────────────────────────────────────────────────────────
// Terminal size helpers (mirrors C globals COLS / LINES)
// ────────────────────────────────────────────────────────────────────────────

/// Return the current terminal width (mirrors C `COLS`).
#[allow(non_snake_case)]
pub fn COLS() -> i32 {
    terminal::size().map(|(w, _)| w as i32).unwrap_or(80)
}

/// Return the current terminal height (mirrors C `LINES`).
#[allow(non_snake_case, dead_code)]
pub fn LINES() -> i32 {
    terminal::size().map(|(_, h)| h as i32).unwrap_or(24)
}

// Get terminal size
pub fn get_terminal_size() -> (u16, u16) {
    terminal::size().unwrap_or((80, 24))
}

// ────────────────────────────────────────────────────────────────────────────
// scanline (mirrors C `scanline()` in aee.c)
// ────────────────────────────────────────────────────────────────────────────

/// Compute the screen-column position of character at 1-based `position`
/// within `line`, taking tabs and control characters into account.
/// Mirrors C `scanline(line, position)`.
pub fn scanline(line: &TextLine, position: i32) -> i32 {
    scanline_raw(&line.line, position)
}

/// Raw scanline that works on a plain `&str` content.
pub fn scanline_raw(content: &str, position: i32) -> i32 {
    let mut scr_pos = 0i32;
    let chars: Vec<char> = content.chars().collect();
    let limit = (position - 1).min(chars.len() as i32) as usize;
    for ch in chars[..limit].iter().copied() {
        scr_pos += len_char_at(scr_pos, ch);
    }
    scr_pos
}

fn len_char_at(scr_pos: i32, ch: char) -> i32 {
    if ch == '\t' {
        8 - (scr_pos % 8)
    } else if (ch as u32) < 32 || ch == '\x7f' {
        2
    } else {
        1
    }
}

// Clear screen
pub fn clear_screen() -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = stdout();
    execute!(stdout, terminal::Clear(ClearType::All))?;
    Ok(())
}

/// Clear a rectangular area on the screen by filling with spaces.
/// Used for overlays like menus that should only clear their own area.
pub fn clear_area(start_x: u16, start_y: u16, width: u16, height: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = stdout();
    let spaces = " ".repeat(width as usize);
    for y in start_y..start_y + height {
        execute!(stdout, cursor::MoveTo(start_x, y), style::Print(&spaces))?;
    }
    stdout.flush()?;
    Ok(())
}


// Move cursor
pub fn move_cursor(x: u16, y: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = stdout();
    execute!(stdout, cursor::MoveTo(x, y))?;
    Ok(())
}

// Print plain text at position
pub fn print_at(x: u16, y: u16, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut stdout = stdout();
    execute!(stdout, cursor::MoveTo(x, y))?;
    execute!(stdout, style::Print(text))?;
    Ok(())
}



/// Print plain text at position, with a marked range inverted
pub fn print_at_marked(x: u16, y: u16, text: &str, mark: Option<(usize, usize)>) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(x, y))?;
    if let Some((start, end)) = mark {
        let s = start.min(text.len());
        let e = end.min(text.len());
        if s > 0 {
            execute!(out, style::Print(&text[..s]))?;
        }
        if e > s {
            execute!(out, SetAttribute(Attribute::Reverse), style::Print(&text[s..e]), SetAttribute(Attribute::Reset))?;
        }
        if e < text.len() {
            execute!(out, style::Print(&text[e..]))?;
        }
    } else {
        execute!(out, style::Print(text))?;
    }
    Ok(())
}

/// Print marked syntax-highlighted line at (x, y)
pub fn print_highlighted_marked(
    x: u16,
    y: u16,
    spans: &[(&str, TokenKind)],
    mark: Option<(usize, usize)>
) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(x, y))?;
    let mut current_offset = 0;
    
    // We iterate over the spans, but if they intersect the mark, we toggle Attribute::Reverse.
    for &(text, ref kind) in spans {
        let color = token_color(kind);
        let span_len = text.len();
        
        if let Some((start, end)) = mark {
            let s = start.saturating_sub(current_offset).min(span_len);
            let e = end.saturating_sub(current_offset).min(span_len);
            
            execute!(out, SetForegroundColor(color))?;
            if s > 0 {
                execute!(out, style::Print(&text[..s]))?;
            }
            if e > s {
                execute!(out, SetAttribute(Attribute::Reverse), style::Print(&text[s..e]), SetAttribute(Attribute::Reset), SetForegroundColor(color))?;
            }
            if e < span_len {
                execute!(out, style::Print(&text[e..]))?;
            }
        } else {
            execute!(out, SetForegroundColor(color), style::Print(text))?;
        }
        current_offset += span_len;
    }
    execute!(out, ResetColor)?;
    out.flush()?;
    Ok(())
}

/// Print marked syntax-highlighted line from owned spans
pub fn print_highlighted_owned_marked(
    x: u16,
    y: u16,
    spans: &[(String, TokenKind)],
    mark: Option<(usize, usize)>
) -> Result<(), Box<dyn std::error::Error>> {
    let borrowed: Vec<(&str, TokenKind)> = spans
        .iter()
        .map(|(s, k)| (s.as_str(), k.clone()))
        .collect();
    print_highlighted_marked(x, y, &borrowed, mark)
}

/// Print the status / info bar - plain text without highlight.
pub fn print_status_bar(y: u16, text: &str, width: u16) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout();
    // Pad to full width
    let padded = format!("{:<width$}", text, width = width as usize);
    execute!(
        out,
        cursor::MoveTo(0, y),
        style::Print(padded),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}

/// Print highlighted text (white foreground, black background) at position.
/// Used for key bindings line and menu borders.
pub fn print_highlighted_at(x: u16, y: u16, text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut out = stdout();
    execute!(
        out,
        cursor::MoveTo(x, y),
        SetForegroundColor(Color::Black),
        style::SetBackgroundColor(Color::White),
        style::Print(text),
        style::SetBackgroundColor(style::Color::Reset),
        ResetColor
    )?;
    out.flush()?;
    Ok(())
}

fn token_color(kind: &TokenKind) -> Color {
    match kind {
        TokenKind::Keyword       => Color::Yellow,
        TokenKind::Comment       => Color::DarkGrey,
        TokenKind::StringLiteral => Color::Green,
        TokenKind::Number        => Color::Cyan,
        TokenKind::Operator      => Color::Magenta,
        TokenKind::Identifier    => Color::White,
        TokenKind::Whitespace    => Color::Reset,
    }
}

// Read key input
pub fn read_key() -> Result<KeyEvent, Box<dyn std::error::Error>> {
    loop {
        if let Event::Key(key) = event::read()? {
            return Ok(key);
        }
    }
}

