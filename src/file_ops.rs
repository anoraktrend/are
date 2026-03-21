// file_ops.rs – file I/O routines
// Mirrors src/file.c (get_full_path, ae_basename, ae_dirname, resolve_name,
// get_file, write_file, diff_file, show_pwd, etc.)

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::os::unix::fs::MetadataExt;

use crate::editor_state::EditorState;

// ────────────────────────────────────────────────────────────────────────────
// Path utilities (mirrors file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Return the basename portion of a path (mirrors C `ae_basename()`).
pub fn ae_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Return the directory portion of a path or `None` if none (mirrors C `ae_dirname()`).
pub fn ae_dirname(path: &str) -> Option<String> {
    Path::new(path)
        .parent()
        .and_then(|p| p.to_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Return the canonical full path for `path` optionally anchored at
/// `orig_path` (mirrors C `get_full_path()`).
pub fn get_full_path(path: &str, orig_path: &str) -> String {
    let base = if !orig_path.is_empty() {
        PathBuf::from(orig_path)
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    let full = if path.is_empty() {
        base
    } else if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

    fs::canonicalize(&full)
        .unwrap_or(full)
        .to_str()
        .unwrap_or("")
        .to_string()
}

/// Expand `~/`, `~user/`, and `$VAR` references in a name.
/// Mirrors C `resolve_name()` in file.c.
pub fn resolve_name(name: &str) -> String {
    let expanded = expand_tilde(name);
    expand_env_vars(&expanded)
}

fn expand_tilde(name: &str) -> String {
    if name.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            return format!("{}{}", home, &name[1..]);
        }
    } else if name.starts_with('~') {
        // ~user/... form – find the slash
        if let Some(slash_pos) = name.find('/') {
            let user_name = &name[1..slash_pos];
            // Try passwd lookup (simplified: just check $HOME if it's the current user)
            let current_user = env::var("USER").or_else(|_| env::var("LOGNAME")).unwrap_or_default();
            if user_name == current_user {
                if let Ok(home) = env::var("HOME") {
                    return format!("{}{}", home, &name[slash_pos..]);
                }
            }
            // Fall back to /home/<user>
            return format!("/home/{}{}", user_name, &name[slash_pos..]);
        }
    }
    name.to_string()
}

fn expand_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    let mut chars  = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            let mut var_name = String::new();
            let braced = chars.peek() == Some(&'{');
            if braced { chars.next(); } // consume '{'
            while let Some(&c) = chars.peek() {
                if braced && c == '}' { chars.next(); break; }
                if !braced && (c == '/' || c == '$' || c == ' ') { break; }
                var_name.push(c);
                chars.next();
            }
            if let Ok(val) = env::var(&var_name) {
                result.push_str(&val);
            } else {
                result.push('$');
                result.push_str(&var_name);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

// ────────────────────────────────────────────────────────────────────────────
// Reading files (mirrors get_file() / get_line() in file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Read the contents of `file_name` and return as a String.
/// Mirrors the high-level behaviour of `get_file()` in file.c.
pub fn get_file(file_name: &str) -> io::Result<String> {
    fs::read_to_string(file_name)
}

// ────────────────────────────────────────────────────────────────────────────
// Writing files (mirrors write_file() in file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Write the current buffer to `file_name`.
/// Returns `true` on success (mirrors C `write_file()` return value).
pub fn write_file(state: &mut EditorState, file_name: &str) -> bool {
    let buff_rc = match state.curr_buff.clone() { Some(b) => b, None => return false };

    let file = match fs::File::create(file_name) {
        Ok(f)  => f,
        Err(_) => return false,
    };
    let mut writer = io::BufWriter::new(file);

    let dos_file = buff_rc.borrow().dos_file;
    let first    = buff_rc.borrow().first_line.clone();
    let mut current = first;

    while let Some(line_rc) = current {
        let (content, next) = {
            let line = line_rc.borrow();
            (line.line.clone(), line.next_line.clone())
        };
        if writer.write_all(content.as_bytes()).is_err() { return false; }
        if dos_file {
            if writer.write_all(b"\r\n").is_err() { return false; }
        } else {
            if writer.write_all(b"\n").is_err()   { return false; }
        }
        current = next;
    }

    if writer.flush().is_err() { return false; }

    // Update cached stat info.
    if let Ok(meta) = fs::metadata(file_name) {
        let mut buff = buff_rc.borrow_mut();
        buff.fileinfo_mtime = meta.mtime() as u64;
        buff.fileinfo_size  = meta.size();
        buff.changed = false;
    }

    true
}

// ────────────────────────────────────────────────────────────────────────────
// show_pwd (mirrors show_pwd() in file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Return the current working directory as a String.
pub fn show_pwd() -> String {
    env::current_dir()
        .map(|p| p.to_str().unwrap_or("").to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

// ────────────────────────────────────────────────────────────────────────────
// diff_file (mirrors diff_file() in file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Run `diff` against the on-disk version of the current file and return
/// the output as a String. Mirrors `diff_file()` in file.c.
pub fn diff_file(state: &EditorState) -> Option<String> {
    let buff_rc  = state.curr_buff.as_ref()?;
    let full_name = buff_rc.borrow().full_name.clone()?;

    let output = std::process::Command::new("diff")
        .arg(&full_name)
        .arg("-")
        .output()
        .ok()?;

    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

// ────────────────────────────────────────────────────────────────────────────
// Directory creation (mirrors create_dir() in journal.c / file.c)
// ────────────────────────────────────────────────────────────────────────────

/// Recursively create `path` if it does not already exist.
pub fn create_dir(path: &str) -> io::Result<()> {
    fs::create_dir_all(path)
}

// ────────────────────────────────────────────────────────────────────────────
// Interactive file browser (used by Ctrl+O and the file menu)
// ────────────────────────────────────────────────────────────────────────────

/// An entry in the directory listing.
#[derive(Clone)]
struct BrowserEntry {
    /// Display name shown in the list (e.g. "src/", "main.rs")
    display: String,
    /// Whether this entry is a directory (used to decide whether to descend or return)
    is_dir: bool,
    /// Absolute path to the entry
    path: PathBuf,
}

/// Read and sort the contents of `dir`: `..` first, then dirs, then files.
fn read_dir_entries(dir: &Path) -> Vec<BrowserEntry> {
    let mut dirs: Vec<BrowserEntry> = Vec::new();
    let mut files: Vec<BrowserEntry> = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_dir = path.is_dir();
            let name = entry.file_name().to_string_lossy().to_string();
            let display = if is_dir {
                format!("{}/", name)
            } else {
                name
            };
            let browser_entry = BrowserEntry { display, is_dir, path };
            if is_dir {
                dirs.push(browser_entry);
            } else {
                files.push(browser_entry);
            }
        }
    }

    dirs.sort_by(|a, b| a.display.cmp(&b.display));
    files.sort_by(|a, b| a.display.cmp(&b.display));

    // Prepend ".." unless we are already at the filesystem root
    let mut result: Vec<BrowserEntry> = Vec::new();
    if let Some(parent) = dir.parent() {
        result.push(BrowserEntry {
            display: "../".to_string(),
            is_dir: true,
            path: parent.to_path_buf(),
        });
    }
    result.extend(dirs);
    result.extend(files);
    result
}

/// Draw the file browser, using crossterm directly (no ncurses).
/// Returns the absolute path of the chosen file, or `None` if the user cancelled.
pub fn show_file_browser(start_dir: &str) -> Option<String> {
    use crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyModifiers},
        execute,
        style::{Attribute, Print, ResetColor, SetAttribute},
        terminal::{self, ClearType},
    };
    use std::io::stdout;

    let mut stdout = stdout();

    // Enter alternate screen so we don't corrupt the editor view.
    // Raw mode is already active (owned by the main event loop).
    let _ = execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide);

    let mut current_dir = fs::canonicalize(start_dir)
        .unwrap_or_else(|_| PathBuf::from(start_dir));
    let mut entries: Vec<BrowserEntry> = read_dir_entries(&current_dir);
    let mut cursor_pos: usize = 0;
    let mut scroll_offset: usize = 0;

    let result = loop {
        // --- draw ---
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        let list_rows = rows.saturating_sub(3) as usize; // header + border + status

        let _ = execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0));

        // Header
        let header = format!(
            " File Browser  {}",
            current_dir.to_string_lossy()
        );
        let header_trunc: String = header.chars().take(cols as usize).collect();
        let _ = execute!(
            stdout,
            SetAttribute(Attribute::Reverse),
            Print(format!("{:<width$}", header_trunc, width = cols as usize)),
            ResetColor,
        );

        // File list
        for (row_idx, entry) in entries
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(list_rows)
        {
            let y = (row_idx - scroll_offset + 1) as u16;
            let _ = execute!(stdout, cursor::MoveTo(0, y));

            // Truncate display name to terminal width
            let display: String = entry.display.chars().take(cols as usize).collect();
            let line = format!("{:<width$}", display, width = cols as usize);

            if row_idx == cursor_pos {
                let _ = execute!(
                    stdout,
                    SetAttribute(Attribute::Reverse),
                    Print(&line),
                    ResetColor,
                );
            } else {
                let _ = execute!(stdout, Print(&line));
            }
        }

        // Status / help bar at the bottom
        let status = "  ↑/↓ Move  Enter Select  Esc Cancel";
        let status_trunc: String = status.chars().take(cols as usize).collect();
        let _ = execute!(
            stdout,
            cursor::MoveTo(0, rows.saturating_sub(1)),
            SetAttribute(Attribute::Reverse),
            Print(format!("{:<width$}", status_trunc, width = cols as usize)),
            ResetColor,
        );

        // --- input ---
        if let Ok(Event::Key(key)) = event::read() {
            match key.code {
                KeyCode::Up => {
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                        if cursor_pos < scroll_offset {
                            scroll_offset = cursor_pos;
                        }
                    }
                }
                KeyCode::Down => {
                    if cursor_pos + 1 < entries.len() {
                        cursor_pos += 1;
                        if cursor_pos >= scroll_offset + list_rows {
                            scroll_offset = cursor_pos + 1 - list_rows;
                        }
                    }
                }
                KeyCode::Home => {
                    cursor_pos = 0;
                    scroll_offset = 0;
                }
                KeyCode::End => {
                    cursor_pos = entries.len().saturating_sub(1);
                    if cursor_pos >= list_rows {
                        scroll_offset = cursor_pos + 1 - list_rows;
                    }
                }
                KeyCode::PageUp => {
                    cursor_pos = cursor_pos.saturating_sub(list_rows);
                    scroll_offset = scroll_offset.saturating_sub(list_rows);
                }
                KeyCode::PageDown => {
                    cursor_pos = (cursor_pos + list_rows).min(entries.len().saturating_sub(1));
                    if cursor_pos >= scroll_offset + list_rows {
                        scroll_offset = cursor_pos + 1 - list_rows;
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = entries.get(cursor_pos) {
                        if entry.is_dir {
                            // Navigate into the directory
                            current_dir = entry.path.clone();
                            entries = read_dir_entries(&current_dir);
                            cursor_pos = 0;
                            scroll_offset = 0;
                        } else {
                            // Return the selected file's path
                            let chosen = entry.path.to_string_lossy().to_string();
                            break Some(chosen);
                        }
                    }
                }
                KeyCode::Esc => {
                    break None;
                }
                // Ctrl+C / Ctrl+Q also cancel
                KeyCode::Char('c') | KeyCode::Char('q')
                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    break None;
                }
                _ => {}
            }
        }
    };

    // Restore terminal state — leave alternate screen, show cursor.
    // Raw mode remains enabled for the main editor loop.
    let _ = execute!(stdout, terminal::LeaveAlternateScreen, cursor::Show);

    result
}

