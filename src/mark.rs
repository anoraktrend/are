//! Mark / cut / copy / paste – ported from src/mark.c
//!
//! The paste buffer is a separate list of `TextLine` nodes that
//! mirrors the editor's main text list.  Copy/cut populate it; paste
//! inserts it at the current cursor position.

use crate::delete_ops;
use crate::editor_state::{Buffer, TextLine};
use crate::text::create_empty_line;

// ──────────────────────────────────────────────────────────────────────────────
// Mark-mode flags (mirrors C enum values)
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkMode {
    Inactive,
    Mark,
}

// ──────────────────────────────────────────────────────────────────────────────
// Mark state (lives on the EditorState or passed around)
// ──────────────────────────────────────────────────────────────────────────────

/// All mutable state associated with mark/paste operations.
pub struct MarkState {
    /// Current mark mode.
    pub mode: MarkMode,
    /// The paste buffer (result of copy/cut).
    pub paste_buff: Vec<TextLine>,
    /// The select buffer (being built during mark).
    pub select_buff: Vec<TextLine>,
    /// Position within `cpste_line`.
    pub pst_pos: i32,
}

impl MarkState {
    pub fn new() -> Self {
        MarkState {
            mode: MarkMode::Inactive,
            paste_buff: Vec::new(),
            select_buff: Vec::new(),
            pst_pos: 1,
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// slct – initiate mark mode
// ──────────────────────────────────────────────────────────────────────────────

/// Begin marking text.  If mark is already active, cancel it.
pub fn slct(ms: &mut MarkState, buff: &Buffer, mode: MarkMode) {
    if ms.mode != MarkMode::Inactive {
        unmark_text(ms);
        return;
    }
    ms.mode = mode;
    let max_len = if buff.curr_line_idx < buff.lines.len() {
        buff.lines[buff.curr_line_idx].max_length as usize
    } else {
        64
    };

    let mut new_line = create_empty_line();
    new_line.line = String::with_capacity(max_len);
    new_line.line_length = 1;
    new_line.max_length = max_len as i32 + 10;

    ms.select_buff = vec![new_line];
    ms.pst_pos = 1;
}

// ──────────────────────────────────────────────────────────────────────────────
// unmark_text – deactivate mark mode and discard select buffer
// ──────────────────────────────────────────────────────────────────────────────

pub fn unmark_text(ms: &mut MarkState) {
    ms.mode = MarkMode::Inactive;
    ms.select_buff.clear();
    ms.pst_pos = 1;
}

// ──────────────────────────────────────────────────────────────────────────────
// copy – move the select buffer into paste_buff
// ──────────────────────────────────────────────────────────────────────────────

/// Copy the marked region into the paste buffer.
pub fn copy(ms: &mut MarkState) -> bool {
    if ms.mode == MarkMode::Inactive {
        return false;
    }
    if ms.select_buff.is_empty() {
        unmark_text(ms);
        return false;
    }

    match ms.mode {
        MarkMode::Mark => {
            ms.paste_buff = ms.select_buff.clone();
        }
        MarkMode::Inactive => {} // Should not be reached due to check above
    }

    ms.mode = MarkMode::Inactive;
    ms.select_buff.clear();
    ms.pst_pos = 1;
    true
}

// ──────────────────────────────────────────────────────────────────────────────
// Collect marked text from buffer
// ──────────────────────────────────────────────────────────────────────────────

/// Collect the text of the current line from `start_pos` (0-based) to `end_pos`
/// into a new paste line node.
fn collect_partial_line(line: &TextLine, start: usize, end: usize) -> TextLine {
    let s = start.min(line.line.len());
    let e = end.min(line.line.len());
    let text = line.line[s..e].to_string();

    let mut n = create_empty_line();
    n.line = text;
    n.line_length = n.line.len() as i32 + 1;
    n.max_length = n.line_length + 10;
    n.highlight_spans = Vec::new();
    n
}

// ──────────────────────────────────────────────────────────────────────────────
// mark_collect – build select buffer from anchor to cursor
// ──────────────────────────────────────────────────────────────────────────────

/// Build the select buffer from `anchor_line_idx/anchor_pos` to the current
/// cursor position.  Returns the newly built select buffer.
pub fn mark_collect(buff: &Buffer, anchor_line_idx: usize, anchor_pos: usize) -> Vec<TextLine> {
    if buff.curr_line_idx >= buff.lines.len() || anchor_line_idx >= buff.lines.len() {
        return Vec::new();
    }
    let cursor_pos = (buff.position as usize).saturating_sub(1);

    // Determine direction: anchor is start, cursor is end (or vice-versa).
    // Walk from anchor_line toward cursor_line collecting text.
    let mut collected = Vec::new();

    // Are they the same line?
    if anchor_line_idx == buff.curr_line_idx {
        let line = &buff.lines[anchor_line_idx];
        let s = anchor_pos.min(line.line.len());
        let e = cursor_pos.min(line.line.len());
        let text = if s <= e {
            line.line[s..e].to_string()
        } else {
            line.line[e..s].to_string()
        };
        let mut n = create_empty_line();
        n.line = text;
        n.line_length = n.line.len() as i32 + 1;
        n.max_length = n.line_length + 10;
        collected.push(n);
        return collected;
    }

    // Walk forward from anchor_line to cursor_line
    // Assuming anchor comes before cursor for now
    let first_node = collect_partial_line(
        &buff.lines[anchor_line_idx],
        anchor_pos,
        buff.lines[anchor_line_idx].line.len(),
    );
    collected.push(first_node);

    let mut cur_idx = anchor_line_idx + 1;
    while cur_idx <= buff.curr_line_idx && cur_idx < buff.lines.len() {
        let is_last = cur_idx == buff.curr_line_idx;
        let end_pos = if is_last {
            cursor_pos
        } else {
            buff.lines[cur_idx].line.len()
        };
        let node = collect_partial_line(&buff.lines[cur_idx], 0, end_pos);
        collected.push(node);
        cur_idx += 1;
    }

    collected
}

// ──────────────────────────────────────────────────────────────────────────────
// cut – cut marked text out of the buffer
// ──────────────────────────────────────────────────────────────────────────────

/// Cut the marked region: collect it into paste_buff and delete from buffer.
pub fn cut(
    ms: &mut MarkState,
    buff: &mut Buffer,
    anchor_line_idx: usize,
    anchor_pos: usize,
) -> bool {
    if ms.mode == MarkMode::Inactive {
        return false;
    }
    if buff.curr_line_idx >= buff.lines.len() || anchor_line_idx >= buff.lines.len() {
        return false;
    }

    // Collect into select buffer
    let collected = mark_collect(buff, anchor_line_idx, anchor_pos);
    if !collected.is_empty() {
        ms.select_buff = collected;
    }

    // Delete the region from the buffer
    if anchor_line_idx == buff.curr_line_idx {
        // Single line: just delete the range
        let start = anchor_pos;
        let end = (buff.position as usize).saturating_sub(1);
        let (s, e) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        {
            let line = &mut buff.lines[buff.curr_line_idx];
            if e <= line.line.len() {
                line.line.replace_range(s..e, "");
                line.line_length = line.line.len() as i32 + 1;
                line.changed = true;
            }
        }
        buff.position = (s + 1) as i32;
        buff.scr_horz = s as i32;
        buff.scr_pos = buff.scr_horz;
        buff.abs_pos = buff.scr_pos;
        buff.changed = true;
    } else {
        // Multiple lines: truncate anchor line, remove intermediate lines,
        // truncate cursor line, then join anchor and cursor lines.
        {
            let al = &mut buff.lines[anchor_line_idx];
            al.line.truncate(anchor_pos);
            al.line_length = al.line.len() as i32 + 1;
            al.changed = true;
        }

        let cursor_line_idx = buff.curr_line_idx;

        // Truncate cursor line from start to cursor_pos
        {
            let cursor_pos = (buff.position as usize).saturating_sub(1);
            let cl = &mut buff.lines[cursor_line_idx];
            let rest = if cursor_pos <= cl.line.len() {
                cl.line[cursor_pos..].to_string()
            } else {
                String::new()
            };
            cl.line = rest;
            cl.line_length = cl.line.len() as i32 + 1;
            cl.changed = true;
        }

        // Join cursor line onto anchor line
        let rest = buff.lines[cursor_line_idx].line.clone();
        {
            let al = &mut buff.lines[anchor_line_idx];
            al.line.push_str(&rest);
            al.line_length = al.line.len() as i32 + 1;
        }

        // Remove lines between anchor and cursor (inclusive of cursor line)
        for _ in anchor_line_idx + 1..=cursor_line_idx {
            if anchor_line_idx + 1 < buff.lines.len() {
                buff.lines.remove(anchor_line_idx + 1);
                buff.num_of_lines -= 1;
            }
        }

        buff.curr_line_idx = anchor_line_idx;
        buff.position = (anchor_pos + 1) as i32;
        buff.scr_horz = anchor_pos as i32;
        buff.scr_pos = buff.scr_horz;
        buff.abs_pos = buff.scr_pos;
        buff.changed = true;
    }

    // Move select buffer to paste buffer
    copy(ms);
    true
}

// ──────────────────────────────────────────────────────────────────────────────
// paste – insert paste buffer at current cursor position
// ──────────────────────────────────────────────────────────────────────────────

/// Insert the paste buffer at the current cursor position.
pub fn paste(ms: &MarkState, buff: &mut Buffer) -> bool {
    if ms.paste_buff.is_empty() {
        return false;
    }
    if ms.mode != MarkMode::Inactive {
        return false;
    }

    // Walk the paste buffer array, inserting each line
    for (i, pline) in ms.paste_buff.iter().enumerate() {
        let text = pline.line.clone();
        let has_next = i + 1 < ms.paste_buff.len();

        // Insert characters from this paste line at the cursor
        delete_ops::insert_string(buff, &text);

        if has_next {
            // Split at cursor and move to next line
            split_line_at_cursor(buff);
        }
    }
    true
}

/// Split the current line at the cursor, moving the rest to a new next-line.
fn split_line_at_cursor(buff: &mut Buffer) {
    if buff.curr_line_idx >= buff.lines.len() {
        return;
    }
    let pos = (buff.position as usize).saturating_sub(1);
    let rest = {
        let line = &mut buff.lines[buff.curr_line_idx];
        let rest = line.line[pos..].to_string();
        line.line.truncate(pos);
        line.line_length = line.line.len() as i32 + 1;
        line.changed = true;
        rest
    };

    let mut nl = create_empty_line();
    nl.line = rest;
    nl.line_length = nl.line.len() as i32 + 1;
    nl.max_length = nl.line_length + 10;
    nl.line_number = buff.lines[buff.curr_line_idx].line_number + 1;

    buff.lines.insert(buff.curr_line_idx + 1, nl);
    buff.curr_line_idx += 1;

    buff.num_of_lines = buff.num_of_lines.saturating_add(1);
    buff.absolute_lin = buff.absolute_lin.saturating_add(1);
    buff.position = 1;
    buff.scr_horz = 0;
    buff.scr_pos = 0;
    buff.abs_pos = 0;
    let (_, height) = crate::ui::get_terminal_size();
    let text_height = (height as i32) - 1;
    if buff.scr_vert < text_height - 1 {
        buff.scr_vert = buff.scr_vert.saturating_add(1);
    } else {
        buff.window_top = buff.window_top.saturating_add(1);
    }
    buff.changed = true;
}

// ──────────────────────────────────────────────────────────────────────────────
// Convenience: get anchor info for integration with main.rs
// ──────────────────────────────────────────────────────────────────────────────

/// Snapshot of where marking started (stored by main.rs when slct is called).
pub struct MarkAnchor {
    pub line_idx: usize,
    pub pos: usize, // 0-based byte offset
    pub abs_lin: i32,
}

impl MarkAnchor {
    pub fn from_buffer(buff: &Buffer) -> Option<Self> {
        if buff.curr_line_idx >= buff.lines.len() {
            return None;
        }
        let line_idx = buff.curr_line_idx;
        let pos = (buff.position as usize).saturating_sub(1);
        let abs_lin = buff.absolute_lin;
        Some(MarkAnchor {
            line_idx,
            pos,
            abs_lin,
        })
    }
}
