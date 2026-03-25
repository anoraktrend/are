// Text-line allocation – mirrors txtalloc() in aee.c

use crate::editor_state::{AeFileInfo, TextLine};

/// Allocate a new, empty TextLine (mirrors C `txtalloc()`).
///
/// `line_length` is initialised to **1** to match the C convention that
/// `line_length` includes the terminating null slot.
pub fn create_empty_line() -> TextLine {
    TextLine {
        line: String::new(),
        line_number: 0,
        max_length: 10,
        vert_len: 1,
        file_info: AeFileInfo::default(),
        changed: false,
        line_length: 1, // empty line: 0 chars + 1 null slot = 1
    }
}
