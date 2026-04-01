// Another Easy Editor - Rust version
// Converted from the original C implementation
#![allow(clippy::explicit_auto_deref)]

mod buffer;
mod delete_ops;
mod editor_state;
mod file_ops;
mod format;
mod help;
mod highlighting;
mod journal;
mod lsp;
mod mark;
mod motion;
mod search;
mod text;
mod ui;
mod windows;

use crossterm::event::{KeyCode, KeyModifiers};
use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let mut editor = editor_state::EditorState::new();
    editor.parse_options(&args);
    editor.initialize().await;

    // Initialize terminal UI (use windows module for proper raw-mode setup)
    if let Err(e) = windows::set_up_term() {
        eprintln!("Failed to initialize terminal: {}", e);
        return;
    }

    // Track terminal dimensions so we can call windows::resize_check each loop
    let (mut last_cols, mut last_rows) = ui::get_terminal_size();

    // If a file was given on the command line, load it (or open a blank buffer
    // for a brand-new path).  With no argument the editor starts with an
    // empty, unnamed buffer – the user can save it later via Ctrl+S, which
    // will prompt for a file name.
    let mut journal_file: Option<std::fs::File> = None;
    if !editor.files.is_empty() {
        let file_name = editor.files[0].clone();
        editor.load_file(&file_name);
        // Open a journal for crash recovery if journalling is on
        if editor.journ_on {
            let jpath = journal::journal_name(&file_name, None);
            if let Ok(jf) = editor
                .curr_buff
                .as_ref()
                .map_or(Err(std::io::Error::other("no buffer")), |b| {
                    journal::open_journal_for_write(&mut b.borrow_mut(), &jpath, &file_name)
                })
            {
                let _ = journal::add_to_journal_db(Some(&file_name), &jpath);
                journal_file = Some(jf);
            }
        }
    }

    // If -r flag was given, attempt crash recovery from the journal file
    if editor.recover && !editor.files.is_empty() {
        let file_name = editor.files[0].clone();
        let jdir = if editor.journal_dir.is_empty() {
            None
        } else {
            Some(editor.journal_dir.as_str())
        };
        let jpath = journal::journal_name(&file_name, jdir);
        if std::path::Path::new(&jpath).exists() {
            if let Some(buff_rc) = editor.curr_buff.clone() {
                match journal::recover_from_journal(&mut buff_rc.borrow_mut(), &jpath) {
                    Ok(_) => { /* recovery succeeded; buffer is now populated from journal */ }
                    Err(e) => eprintln!("Journal recovery failed: {}", e),
                }
            }
        }
    }

    // Mark / cut / copy / paste state
    let mut mark_state = mark::MarkState::new();
    let mut mark_anchor: Option<mark::MarkAnchor> = None;

    // Main editing loop
    loop {
        // Check for terminal resize and update buffer geometry if needed
        let (curr_cols, curr_rows) = ui::get_terminal_size();
        if curr_cols != last_cols || curr_rows != last_rows {
            last_cols = curr_cols;
            last_rows = curr_rows;
            if let Some(buff_rc) = editor.curr_buff.clone() {
                windows::resize_check(&mut buff_rc.borrow_mut(), curr_cols, curr_rows);
            }
        }

        // Poll any pending LSP messages (non-blocking)
        if let Some(lsp) = &mut editor.lsp_client {
            lsp.poll_messages();
        }

        // Render screen
        if let Err(e) = draw_screen(&editor, &mark_anchor) {
            eprintln!("Failed to draw screen: {}", e);
            break;
        }

        // Read input
        match ui::read_key() {
            Ok(key) => {
                let is_shift = key.modifiers.contains(KeyModifiers::SHIFT);
                match key.code {
                    // ── Standard Linux Bindings (Ctrl+X/C/V/Z/S/Q/O/N/P/A/F/R/W) ──

                    // Ctrl+A – Select All
                    KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::top(&mut buff_rc.borrow_mut());
                            let buff = buff_rc.borrow();
                            mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                            mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                            editor.mark_text = true;
                            drop(buff);
                            motion::bottom(&mut buff_rc.borrow_mut());
                            motion::eol(&mut buff_rc.borrow_mut());
                        }
                    }
                    // Ctrl+C – Copy marked region
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        mark::copy(&mut mark_state);
                        editor.mark_text = false;
                    }
                    // Ctrl+E – command prompt (Kept as an explicit command runner)
                    KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let cmd = get_user_input("Command: ");
                        let parts: Vec<&str> = cmd.split_whitespace().collect();
                        if parts.is_empty() {
                            continue;
                        }
                        match parts[0] {
                            "help" => help::help(None),
                            "pwd" => {
                                let pwd = file_ops::show_pwd();
                                show_message_prompt(&format!("PWD: {}", pwd));
                            }
                            "mkdir" if parts.len() > 1 => {
                                if let Err(e) = file_ops::create_dir(parts[1]) {
                                    show_message_prompt(&format!("Error: {}", e));
                                }
                            }
                            "dirname" if parts.len() > 1 => {
                                if let Some(dir) = file_ops::ae_dirname(parts[1]) {
                                    show_message_prompt(&format!("Dirname: {}", dir));
                                }
                            }
                            "write" if parts.len() > 1 => {
                                let saved = file_ops::write_file(&mut editor, parts[1]);
                                show_message_prompt(&format!("Write result: {}", saved));
                            }
                            "format" => {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    format::format_paragraph(
                                        &mut buff_rc.borrow_mut(),
                                        editor.left_margin,
                                        editor.right_margin,
                                        editor.right_justify,
                                    );
                                }
                            }
                            "indent" => {
                                editor.indent = !editor.indent;
                                show_message_prompt(&format!("Indent: {}", editor.indent));
                            }
                            "margin" if parts.len() > 2 => {
                                if let (Ok(lm), Ok(rm)) =
                                    (parts[1].parse::<i32>(), parts[2].parse::<i32>())
                                {
                                    editor.left_margin = lm;
                                    editor.right_margin = rm;
                                    editor.observ_margins = true;
                                }
                            }
                            "justify" => {
                                editor.right_justify = !editor.right_justify;
                                show_message_prompt(&format!(
                                    "Right justify: {}",
                                    editor.right_justify
                                ));
                            }
                            "bufcount" => {
                                let count = editor.buf_count();
                                show_message_prompt(&format!("Buffer count: {}", count));
                            }
                            "status" => {
                                editor.status_line = !editor.status_line;
                            }
                            _ => {}
                        }
                    }
                    // Ctrl+F – Find
                    KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let s = get_user_input("Search: ");
                        if !s.is_empty() {
                            editor.srch_str = Some(s.clone());
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                if let Some(result) = search::search_forward(
                                    &mut buff_rc.borrow_mut(),
                                    &s,
                                    editor.case_sen,
                                ) {
                                    editor.lines_moved = result.lines_moved;
                                    show_message_prompt(&format!(
                                        "Found at line {}, col {}",
                                        result.line_num, result.col
                                    ));
                                } else {
                                    show_message_prompt("Not found");
                                }
                            }
                        }
                    }
                    // Ctrl+G – Goto line
                    KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let line_num_str = get_user_input("Go to line: ");
                        if let Ok(n) = line_num_str.parse::<i32>() {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                motion::goto_line(&mut buff_rc.borrow_mut(), n);
                            }
                        }
                    }
                    // Ctrl+N - New File (Clear buffer)
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let mut buff = buff_rc.borrow_mut();
                            buff.file_name = None;
                            buff.full_name = None;
                            let mut new_line = crate::text::create_empty_line();
                            {
                                let line = &mut new_line;
                                line.line = String::new();
                                line.line_length = 1;
                                line.max_length = 10;
                                line.line_number = 1;
                                line.vert_len = 1;
                            }
                            let insert_idx = buff.curr_line_idx + 1;
                            buff.lines.insert(insert_idx, new_line.clone());
                            buff.curr_line_idx += 1;
                            buff.num_of_lines = 1;
                            buff.absolute_lin = 1;
                            buff.position = 1;
                            buff.abs_pos = 0;
                            buff.scr_pos = 0;
                            buff.scr_vert = 0;
                            buff.scr_horz = 0;
                            buff.changed = false;
                        }
                        if let Some(jf) = journal_file.take() {
                            let _ = jf.sync_all();
                        }
                        editor.srch_line_idx = 0;
                    }
                    // Ctrl+O - Open file
                    KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let start = std::env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| ".".to_string());
                        if let Some(path) = file_ops::show_file_browser(&start) {
                            editor.load_file(&path);
                        }
                    }
                    // Ctrl+P - Print
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let buff = buff_rc.borrow();
                            let mut contents = String::new();
                            for line in &buff.lines {
                                contents.push_str(&line.line);
                                contents.push('\n');
                            }
                            use std::io::Write;
                            if let Ok(mut child) = std::process::Command::new("lp")
                                .stdin(std::process::Stdio::piped())
                                .spawn()
                            {
                                if child
                                    .stdin
                                    .as_mut()
                                    .unwrap()
                                    .write_all(contents.as_bytes())
                                    .is_ok()
                                {
                                    let _ = child.wait();
                                    show_message_prompt("Buffer sent to printer (lp).");
                                } else {
                                    show_message_prompt("Failed to print: write_all failed");
                                }
                            } else {
                                show_message_prompt("Failed to spawn `lp` command");
                            }
                        }
                    }
                    // Ctrl+Q – Quit
                    KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    // Ctrl+R - Replace
                    KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let s = get_user_input("Search: ");
                        let r = get_user_input("Replace with: ");
                        if !s.is_empty() {
                            editor.srch_str = Some(s.clone());
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                let count = search::replace_all(
                                    &mut buff_rc.borrow_mut(),
                                    &s,
                                    &r,
                                    editor.case_sen,
                                );
                                show_message_prompt(&format!("Replaced {} occurrences", count));
                            }
                        }
                    }
                    // Ctrl+S – Save
                    KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        save_file(&mut editor, &mut journal_file);
                    }
                    // Ctrl+V – Paste
                    KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            mark::paste(&mark_state, &mut buff_rc.borrow_mut());
                        }
                    }
                    // Ctrl+W - Close (Clear buffer)
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let mut buff = buff_rc.borrow_mut();
                            buff.file_name = None;
                            buff.full_name = None;
                            let mut new_line = crate::text::create_empty_line();
                            {
                                let line = &mut new_line;
                                line.line = String::new();
                                line.line_length = 1;
                                line.max_length = 10;
                                line.line_number = 1;
                                line.vert_len = 1;
                            }
                            let insert_idx = buff.curr_line_idx + 1;
                            buff.lines.insert(insert_idx, new_line.clone());
                            buff.curr_line_idx += 1;
                            buff.num_of_lines = 1;
                            buff.absolute_lin = 1;
                            buff.position = 1;
                            buff.abs_pos = 0;
                            buff.scr_pos = 0;
                            buff.scr_vert = 0;
                            buff.scr_horz = 0;
                            buff.changed = false;
                        }
                        if let Some(jf) = journal_file.take() {
                            let _ = jf.sync_all();
                        }
                    }
                    // Ctrl+X – Cut
                    KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            if let Some(anchor) = mark_anchor.take() {
                                mark::cut(
                                    &mut mark_state,
                                    &mut buff_rc.borrow_mut(),
                                    anchor.line_idx,
                                    anchor.pos,
                                );
                            }
                        }
                        editor.mark_text = false;
                    }
                    // Ctrl+Z – Undo
                    KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        undo(&mut editor);
                    }

                    // ── Function keys F1–F8 (matches C code f[] array) ───────────────────────────────
                    // F1 – GOLD key (fn_GOLD_str in C)
                    KeyCode::F(1) => {
                        editor.gold = !editor.gold;
                    }
                    // F2 – undelete character (fn_UDC_str in C)
                    KeyCode::F(2) => {
                        if editor.d_char != '\0' {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                delete_ops::insert_char_at_cursor(
                                    &mut buff_rc.borrow_mut(),
                                    editor.d_char,
                                );
                            }
                        }
                    }
                    // F3 – delete word (fn_DW_str in C)
                    KeyCode::F(3) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let deleted = delete_ops::del_word(&mut buff_rc.borrow_mut());
                            if !deleted.is_empty() {
                                editor.d_word = Some(deleted);
                            }
                        }
                    }
                    // F4 – advance word (fn_AW_str in C)
                    KeyCode::F(4) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::adv_word(&mut buff_rc.borrow_mut());
                        }
                    }
                    // F5 – search (fn_SRCH_str in C)
                    KeyCode::F(5) => {
                        let s = get_user_input("Search: ");
                        if !s.is_empty() {
                            editor.srch_str = Some(s.clone());
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                search::search_forward(
                                    &mut buff_rc.borrow_mut(),
                                    &s,
                                    editor.case_sen,
                                );
                            }
                        }
                    }
                    // F6 – mark (fn_MARK_str in C)
                    KeyCode::F(6) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let buff = buff_rc.borrow();
                            mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                            mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                            editor.mark_text = true;
                        }
                    }
                    // F7 – cut (fn_CUT_str in C)
                    KeyCode::F(7) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            if let Some(anchor) = mark_anchor.take() {
                                mark::cut(
                                    &mut mark_state,
                                    &mut buff_rc.borrow_mut(),
                                    anchor.line_idx,
                                    anchor.pos,
                                );
                            }
                        }
                        editor.mark_text = false;
                    }
                    // F8 – advance line (fn_AL_str in C)
                    KeyCode::F(8) => {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::adv_line(&mut buff_rc.borrow_mut());
                        }
                    }

                    // ── Insert key – toggle overstrike mode ──────────────────────────────
                    KeyCode::Insert => {
                        editor.overstrike = !editor.overstrike;
                    }

                    // ── Generic printable character – must come last among Char arms ──
                    KeyCode::Char(c) => {
                        if is_shift && editor.mark_text {
                            mark::copy(&mut mark_state);
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                if let Some(anchor) = mark_anchor.take() {
                                    mark::cut(
                                        &mut mark_state,
                                        &mut buff_rc.borrow_mut(),
                                        anchor.line_idx,
                                        anchor.pos,
                                    );
                                }
                            }
                            editor.mark_text = false;
                        }
                        insert_char(&mut editor, c);
                        // Auto-format on space/tab if auto_format is enabled and right_margin set
                        if editor.auto_format && editor.right_margin > 0 && (c == ' ' || c == '\t')
                        {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                format::auto_format(&mut buff_rc.borrow_mut(), editor.right_margin);
                            }
                        }
                    }
                    KeyCode::Enter => {
                        // New line
                        insert_newline(&mut editor);
                    }
                    KeyCode::Backspace => {
                        // Backspace: delete character before cursor using delete_ops module.
                        // Record undo before the deletion so Ctrl+Z can restore it.
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            {
                                let buff = buff_rc.borrow();
                                if buff.curr_line_idx < buff.lines.len() {
                                    let pos = buff.position as usize;
                                    if pos > 1 {
                                        let line = &buff.lines[buff.curr_line_idx];
                                        if let Some(ch) = line.line.chars().nth(pos - 2) {
                                            editor.last_action =
                                                Some(crate::editor_state::LastAction::DeleteChar {
                                                    line_idx: buff.curr_line_idx,
                                                    pos: pos - 2,
                                                    ch,
                                                });
                                        }
                                    }
                                }
                            }
                            delete_ops::backspace(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::Delete => {
                        // Delete: delete character at cursor using delete_ops module
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            if let Some(ch) = delete_ops::delete_forward(&mut buff_rc.borrow_mut())
                            {
                                editor.d_char = ch; // save for Ctrl+U undelete
                            }
                        }
                    }
                    KeyCode::Left => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                motion::prev_word(&mut buff_rc.borrow_mut());
                            }
                        } else {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                motion::move_left(&mut buff_rc.borrow_mut());
                            }
                        }
                    }
                    KeyCode::Right => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                motion::adv_word(&mut buff_rc.borrow_mut());
                            }
                        } else {
                            if let Some(buff_rc) = editor.curr_buff.clone() {
                                motion::move_right(&mut buff_rc.borrow_mut());
                            }
                        }
                    }
                    KeyCode::Home => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::bol(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::bottom(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::End => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::eol(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::Up => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::move_up(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::Down => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }

                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::move_down(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::PageUp => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::prev_page(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::PageDown => {
                        if is_shift {
                            if !editor.mark_text {
                                if let Some(buff_rc) = editor.curr_buff.clone() {
                                    let buff = buff_rc.borrow();
                                    mark::slct(&mut mark_state, &buff, mark::MarkMode::Mark);
                                    mark_anchor = mark::MarkAnchor::from_buffer(&buff);
                                    editor.mark_text = true;
                                }
                            }
                        } else if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        }
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            motion::next_page(&mut buff_rc.borrow_mut());
                        }
                    }
                    KeyCode::Esc => {
                        // If mark mode is active, cancel it instead of opening menu
                        if editor.mark_text {
                            mark::unmark_text(&mut mark_state);
                            editor.mark_text = false;
                        } else {
                            // Show the main menu - if it returns true, exit the editor
                            if show_main_menu(
                                &mut editor,
                                &mut journal_file,
                                &mut mark_state,
                                &mut mark_anchor,
                            ) {
                                break;
                            }
                        }
                    }
                    _ => {
                        // Ignore other keys for now
                    }
                }

                // Journal changed lines after every keystroke if journalling is on
                if editor.journ_on {
                    if let Some(ref mut jf) = journal_file {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let mut buff = buff_rc.borrow_mut();
                            let idx = buff.curr_line_idx;
                            if idx < buff.lines.len() {
                                if buff.lines[idx].changed {
                                    let _ = journal::write_journal(jf, &mut *buff, idx);
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read key: {}", e);
                break;
            }
        }
    }

    // Remove journal on clean exit
    if editor.journ_on {
        if let Some(ref buff_rc) = editor.curr_buff {
            let buff = buff_rc.borrow();
            if let Some(ref fname) = buff.file_name {
                let jpath = journal::journal_name(fname, None);
                let _ = journal::remove_journal_file(&jpath, fname);
            }
        }
    }

    // Restore terminal (use windows module)
    if let Err(e) = windows::restore_term() {
        eprintln!("Failed to restore terminal: {}", e);
    }
}

/// Display the key binding bar at the top of the screen (matching the C version)
fn draw_key_bindings(y: u16, width: u16) -> Result<u16, Box<dyn std::error::Error>> {
    // Updated key bindings to match the actual Rust implementation
    let bindings = [
        "Esc  menu       ^S   save file  ^O   open file  ^N   new file   ^W   close file F2   und char   F6   mark       ",
        "^A   select all ^Q   quit       ^X   cut        ^C   copy       ^V   paste      F3   del word   F7   cut        ",
        "^Z   undo       ^E   command    ^F   search     ^R   replace    ^G   goto line  F4   adv word   F8   adv line   ",
        "^P   print      Ins  overstrk                                              F1   GOLD       F5   search     ",
    ];

    let mut current_y = y;
    for binding in &bindings {
        // Pad to full width and truncate if needed
        let display = if binding.len() > width as usize {
            binding[..width as usize].to_string()
        } else {
            format!("{:<width$}", binding, width = width as usize)
        };
        ui::print_at(0, current_y, &display)?;
        current_y += 1;
    }

    // Bottom line showing " ^ = Ctrl key  ---- access HELP through menu ---"
    // This line is highlighted (white on black)
    let help_line = " ^ = Ctrl key  ---- access HELP through menu ---";
    let help_display = if help_line.len() > width as usize {
        help_line[..width as usize].to_string()
    } else {
        format!("{:<width$}", help_line, width = width as usize)
    };
    ui::print_highlighted_at(0, current_y, &help_display)?;
    current_y += 1;

    Ok(current_y)
}

fn draw_screen(
    editor: &editor_state::EditorState,
    mark_anchor: &Option<crate::mark::MarkAnchor>,
) -> Result<(), Box<dyn std::error::Error>> {
    ui::clear_screen()?;

    let (width, height) = ui::get_terminal_size();

    // Draw key bindings at top (like C version)
    let key_bindings_height = draw_key_bindings(0, width)?;
    let text_start_y = key_bindings_height;

    // ── Text area ends before the status bar at the bottom ────────────────
    let status_bar_y = height - 1;

    // ── Determine language for syntax highlighting ───────────────────────────
    let lang: &str = if let Some(buff) = &editor.curr_buff {
        let buff = buff.borrow();
        if let Some(ref fname) = buff.file_name {
            highlighting::lang_from_extension(fname)
        } else {
            "text"
        }
    } else {
        "text"
    };

    // ── Text area ─────────────────────────────────────────────────────────────
    if let Some(buff_rc) = &editor.curr_buff {
        let buff = buff_rc.borrow();
        let mut y = text_start_y;
        let start_idx = (buff.window_top as usize).saturating_sub(1);

        // ── LSP semantic tokens for this file (if any) ──────────────────────
        let lsp_tokens_for_file: Option<(&Vec<lsp::SemanticToken>, &Vec<String>)> =
            editor.lsp_client.as_ref().and_then(|lsp| {
                let uri = buff.full_name.as_ref().map(|p| format!("file://{}", p))?;
                let tokens = lsp.get_semantic_tokens(&uri)?;
                Some((tokens, &lsp.token_type_legend))
            });

        // Compute the correct block-comment state at window_top by scanning from file start
        let mut in_block_comment = if lang != "text" && start_idx > 0 {
            let mut state = false;
            for idx in 0..start_idx {
                let txt = &buff.lines[idx].line;
                let (_, new_state) = highlighting::highlight_line_with_state(txt, lang, state);
                state = new_state;
            }
            state
        } else {
            false
        };

        // Draw visible lines starting from window_top (start_idx)
        for idx in start_idx..buff.lines.len() {
            let line_num = (idx + 1) as i32;
            let mut mark_range: Option<(usize, usize)> = None;
            if editor.mark_text {
                if let Some(anchor) = mark_anchor {
                    let a_lin = anchor.abs_lin;
                    let c_lin = buff.absolute_lin;
                    let a_pos = anchor.pos;
                    let c_pos = (buff.position as usize).saturating_sub(1);

                    let s_lin = a_lin.min(c_lin);
                    let e_lin = a_lin.max(c_lin);

                    if line_num == s_lin && line_num == e_lin {
                        mark_range = Some((a_pos.min(c_pos), a_pos.max(c_pos)));
                    } else if line_num == s_lin {
                        if a_lin < c_lin {
                            mark_range = Some((a_pos, usize::MAX));
                        } else {
                            mark_range = Some((c_pos, usize::MAX));
                        }
                    } else if line_num == e_lin {
                        if a_lin < c_lin {
                            mark_range = Some((0, c_pos));
                        } else {
                            mark_range = Some((0, a_pos));
                        }
                    } else if line_num > s_lin && line_num < e_lin {
                        mark_range = Some((0, usize::MAX));
                    }
                }
            }

            let line = &buff.lines[idx];
            let raw = &line.line;
            // Truncate to terminal width (byte-safe)
            let display_text: &str = if raw.len() > width as usize {
                let mut end = width as usize;
                while !raw.is_char_boundary(end) {
                    end -= 1;
                }
                &raw[..end]
            } else {
                raw.as_str()
            };

            if !editor.nohighlight {
                if let Some((all_tokens, legend)) = lsp_tokens_for_file {
                    // Filter semantic tokens that belong to this display line (0-based idx)
                    let lsp_line_idx = idx as u32;
                    let line_tokens: Vec<&lsp::SemanticToken> = all_tokens
                        .iter()
                        .filter(|t| t.line == lsp_line_idx)
                        .collect();

                    if !line_tokens.is_empty() {
                        let owned_tokens: Vec<lsp::SemanticToken> =
                            line_tokens.into_iter().cloned().collect();
                        let spans =
                            highlighting::highlight_line_lsp(display_text, &owned_tokens, legend);
                        ui::print_highlighted_owned_marked(0, y, &spans, mark_range)?;
                        y += 1u16;
                        if y >= height {
                            break;
                        }
                        continue;
                    }
                }

                if lang != "text" {
                    let (spans, new_state) = highlighting::highlight_line_with_state(
                        display_text,
                        lang,
                        in_block_comment,
                    );
                    in_block_comment = new_state;
                    ui::print_highlighted_marked(0, y, &spans, mark_range)?;
                } else {
                    ui::print_at_marked(0, y, display_text, mark_range)?;
                }
            } else {
                ui::print_at_marked(0, y, display_text, mark_range)?;
            }

            y += 1u16;
            if y >= height {
                break;
            }
        }
    }

    // ── Status bar at the bottom ───────────────────────────────────────────
    let info_text = if let Some(buff) = &editor.curr_buff {
        let buff = buff.borrow();
        let file_label = buff.file_name.as_deref().unwrap_or("[No File]");
        let changed_mark = if buff.changed { " [+]" } else { "" };
        format!(
            " aee  {}{}  |  Ln {} Col {}",
            file_label,
            changed_mark,
            buff.absolute_lin,
            buff.position - 1
        )
    } else {
        " aee  |  Ln 1 Col 0".to_string()
    };
    ui::print_status_bar(status_bar_y, &info_text, width)?;

    // ── Reposition cursor ────────────────────────────────────────────────────
    if let Some(buff) = &editor.curr_buff {
        let buff = buff.borrow();
        let cursor_y = buff.scr_vert as u16 + text_start_y;
        ui::move_cursor(buff.scr_horz as u16, cursor_y)?;
    }

    Ok(())
}

fn insert_char(editor: &mut editor_state::EditorState, ch: char) {
    if let Some(buff) = &editor.curr_buff {
        let mut buff = buff.borrow_mut();
        let idx = buff.curr_line_idx;
        if idx < buff.lines.len() {
            let pos = buff.position as usize;
            let line = &mut buff.lines[idx];
            if pos <= line.line.len() + 1 {
                let safe_pos = pos.saturating_sub(1).min(line.line.len());
                line.line.insert(safe_pos, ch);
                line.line_length = line.line.len() as i32 + 1;
                line.changed = true;
                buff.position = buff.position.saturating_add(1);
                buff.abs_pos = buff.abs_pos.saturating_add(1);
                buff.scr_horz = buff.scr_horz.saturating_add(1);
                buff.changed = true;
                // Record undo
                editor.last_action = Some(crate::editor_state::LastAction::InsertChar {
                    line_idx: idx,
                    pos: safe_pos,
                });
            }
        }
    }
}

fn insert_newline(editor: &mut editor_state::EditorState) {
    if let Some(buff) = &editor.curr_buff {
        let mut buff = buff.borrow_mut();
        let idx = buff.curr_line_idx;
        if idx < buff.lines.len() {
            let pos = buff.position as usize;
            let safe_pos = pos.saturating_sub(1).min(buff.lines[idx].line.len());

            // Split the line
            let rest = buff.lines[idx].line.split_off(safe_pos);
            buff.lines[idx].line_length = buff.lines[idx].line.len() as i32 + 1;
            buff.lines[idx].changed = true;

            // Create new line
            let mut new_line = crate::text::create_empty_line();
            new_line.line = rest;
            new_line.line_length = new_line.line.len() as i32 + 1;
            new_line.max_length = new_line.line_length + 10;
            new_line.line_number = (idx + 2) as i32;
            new_line.vert_len = 1;
            new_line.changed = true;

            // Insert into Vec
            buff.lines.insert(idx + 1, new_line);

            // Renumber subsequent lines
            for i in (idx + 2)..buff.lines.len() {
                buff.lines[i].line_number += 1;
            }

            // Update buffer cursor
            buff.curr_line_idx += 1;
            buff.num_of_lines = buff.num_of_lines.saturating_add(1);
            buff.position = 1;
            buff.abs_pos = 0;
            buff.scr_horz = 0;
            buff.absolute_lin = buff.absolute_lin.saturating_add(1);
            buff.changed = true;

            let (_, height) = ui::get_terminal_size();
            let text_height = (height as i32) - 1;
            if buff.scr_vert < text_height - 1 {
                buff.scr_vert = buff.scr_vert.saturating_add(1);
            } else {
                buff.window_top = buff.window_top.saturating_add(1);
            }
        }
    }
}

fn save_file(editor: &mut editor_state::EditorState, journal_file: &mut Option<std::fs::File>) {
    // If the buffer has no file name (opened without an argument), ask the
    // user for one before writing.
    let needs_name = editor
        .curr_buff
        .as_ref()
        .map(|b| b.borrow().file_name.is_none())
        .unwrap_or(false);

    if needs_name {
        let name = get_user_input("Save as: ");
        if name.is_empty() {
            return; // user cancelled
        }
        if let Some(ref buff_rc) = editor.curr_buff {
            let mut buff = buff_rc.borrow_mut();
            buff.file_name = Some(name.clone());
            buff.full_name = Some(crate::file_ops::get_full_path(&name, ""));
        }
        // Open a journal for the newly-named file
        if editor.journ_on && journal_file.is_none() {
            if let Some(ref buff_rc) = editor.curr_buff {
                let fname = buff_rc.borrow().file_name.clone().unwrap_or_default();
                let jpath = journal::journal_name(&fname, None);
                if let Ok(jf) =
                    journal::open_journal_for_write(&mut buff_rc.borrow_mut(), &jpath, &fname)
                {
                    let _ = journal::add_to_journal_db(Some(&fname), &jpath);
                    *journal_file = Some(jf);
                }
            }
        }
    }

    if let Some(buff_rc) = &editor.curr_buff {
        let buff = buff_rc.borrow();
        if let Some(file_name) = &buff.file_name {
            let mut contents = String::new();
            for line in &buff.lines {
                let line_data = line;
                contents.push_str(&line_data.line);
                contents.push('\n');
            }
            // Remove trailing newline added after the last line
            if contents.ends_with('\n') && buff.num_of_lines > 0 {
                contents.pop();
            }
            let file_name = file_name.clone();
            drop(buff); // release borrow before the mutable borrow below
            if let Err(e) = std::fs::write(&file_name, contents) {
                eprintln!("Failed to save file: {}", e);
            } else {
                buff_rc.borrow_mut().changed = false;
                // Remove journal on successful save (clean state)
                if editor.journ_on {
                    let jpath = journal::journal_name(&file_name, None);
                    let _ = journal::remove_journal_file(&jpath, &file_name);
                    *journal_file = None;
                }
            }
        }
    }
}

fn get_user_input(prompt: &str) -> String {
    ui::clear_screen().unwrap();
    ui::print_at(0, 0, prompt).unwrap();
    let mut input = String::new();
    let mut cursor_pos = prompt.len();
    ui::move_cursor(cursor_pos as u16, 0).unwrap();

    loop {
        match ui::read_key().unwrap().code {
            KeyCode::Char(c) => {
                input.push(c);
                cursor_pos += 1;
            }
            KeyCode::Backspace => {
                if !input.is_empty() {
                    input.pop();
                    cursor_pos -= 1;
                }
            }
            KeyCode::Enter => break,
            KeyCode::Esc => {
                input.clear();
                break;
            }
            _ => {}
        }
        ui::clear_screen().unwrap();
        ui::print_at(0, 0, &format!("{}{}", prompt, input)).unwrap();
        ui::move_cursor(cursor_pos as u16, 0).unwrap();
    }
    input
}

fn undo(editor: &mut editor_state::EditorState) {
    if let Some(action) = editor.last_action.take() {
        if let Some(buff_rc) = &editor.curr_buff {
            let mut b = buff_rc.borrow_mut();
            match action {
                crate::editor_state::LastAction::InsertChar { line_idx, pos } => {
                    if line_idx < b.lines.len() {
                        let l = &mut b.lines[line_idx];
                        if pos > 0 && pos <= l.line.len() {
                            l.line.remove(pos - 1);
                            l.line_length = l.line.len() as i32 + 1;
                            l.changed = true;
                            b.changed = true;
                            if line_idx == b.curr_line_idx && b.position > pos as i32 {
                                b.position -= 1;
                                b.abs_pos -= 1;
                                b.scr_horz -= 1;
                            }
                        }
                    }
                }
                crate::editor_state::LastAction::DeleteChar { line_idx, pos, ch } => {
                    if line_idx < b.lines.len() {
                        let l = &mut b.lines[line_idx];
                        if pos <= l.line.len() {
                            l.line.insert(pos, ch);
                            l.line_length = l.line.len() as i32 + 1;
                            l.changed = true;
                            b.changed = true;
                            if line_idx == b.curr_line_idx {
                                b.position = (pos + 1) as i32;
                                b.abs_pos = b.position;
                                b.scr_horz = b.position - 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Helper to show a message and wait for user acknowledgment so it doesn't get cleared immediately
fn show_message_prompt(msg: &str) {
    let _ = ui::clear_screen();
    let mut y = 0;
    for line in msg.lines() {
        let _ = ui::print_at(0, y, line);
        y += 1;
    }
    let _ = ui::print_at(0, y + 1, "Press any key to continue...");
    let _ = ui::read_key();
}

/// Menu position and size info for clearing later
struct MenuArea {
    start_x: u16,
    start_y: u16,
    width: u16,
    height: u16,
}

impl MenuArea {
    fn clear(&self) {
        // Clear the menu area plus some extra space for the message box
        let extra_height = 4u16;
        let _ = ui::clear_area(
            self.start_x,
            self.start_y,
            self.width,
            self.height + extra_height,
        );
    }
}

/// Display a menu with optional previous menu area to clear first
/// Returns (selected_index, area) so caller can clear it later if needed
fn show_menu_with_area(
    title: &str,
    menu_items: &[(&str, &str, bool)],
    prev_menu: Option<&MenuArea>,
) -> Option<(usize, MenuArea)> {
    // Clear previous menu area if provided
    if let Some(prev) = prev_menu {
        prev.clear();
    }

    let (width, height) = ui::get_terminal_size();
    // Calculate menu width based on the longest item
    let mut max_item_len = 0usize;
    for (key, item, is_submenu) in menu_items {
        let suffix_len = if *is_submenu { 2 } else { 0 };
        let item_len = 4 + key.len() + item.len() + suffix_len; // "a) item" = 4 + len(key) + len(item)
        max_item_len = max_item_len.max(item_len);
    }
    let menu_width = (max_item_len + 4).max(30).min(width as usize) as u16;
    let menu_height = menu_items.len() as u16 + 4;
    let start_x = (width - menu_width) / 2;
    let start_y = (height - menu_height - 2) / 2;

    // Create the menu area info to return
    let menu_area = MenuArea {
        start_x,
        start_y,
        width: menu_width,
        height: menu_height + 4, // include message box
    };

    let mut selected = 0;

    loop {
        // Clear only the rectangular area that the menu will occupy
        ui::clear_area(start_x, start_y, menu_width, menu_height + 4).unwrap();

        // Top border: +------------------------------+
        ui::print_highlighted_at(start_x, start_y, "+").unwrap();
        for x in (start_x + 1)..(start_x + menu_width - 1) {
            ui::print_highlighted_at(x, start_y, "-").unwrap();
        }
        ui::print_highlighted_at(start_x + menu_width - 1, start_y, "+").unwrap();

        // Title (centered with padding)

        let title_padded = format!("{:^width$}", title, width = menu_width as usize - 2);
        ui::print_at(start_x, start_y + 1, &title_padded).unwrap();
        ui::print_highlighted_at(start_x, start_y + 1, "|").unwrap();
        ui::print_highlighted_at(start_x + menu_width - 1, start_y + 1, "|").unwrap();
        // Separator (highlighted) - items header line
        ui::print_highlighted_at(start_x, start_y + 2, "+").unwrap();
        for x in (start_x + 1)..(start_x + menu_width - 1) {
            ui::print_at(x, start_y + 2, "-").unwrap();
        }
        ui::print_highlighted_at(start_x + menu_width - 1, start_y + 2, "+").unwrap();

        // Draw vertical borders for items
        for i in 0..menu_items.len() {
            // Left border
            ui::print_highlighted_at(start_x, start_y + 3 + i as u16, "|").unwrap();
            // Right border
            ui::print_highlighted_at(start_x + menu_width - 1, start_y + 3 + i as u16, "|")
                .unwrap();
        }

        // Empty row before bottom border
        ui::print_highlighted_at(start_x, start_y + 3 + menu_items.len() as u16, "|").unwrap();
        for x in (start_x + 1)..(start_x + menu_width - 1) {
            ui::print_at(x, start_y + 3 + menu_items.len() as u16, " ").unwrap();
        }
        ui::print_highlighted_at(
            start_x + menu_width - 1,
            start_y + 3 + menu_items.len() as u16,
            "|",
        )
        .unwrap();

        // Bottom border: +------------------------------+
        ui::print_highlighted_at(start_x, start_y + menu_height, "+").unwrap();
        for x in (start_x + 1)..(start_x + menu_width - 1) {
            ui::print_at(x, start_y + menu_height, "-").unwrap();
        }
        ui::print_highlighted_at(start_x + menu_width - 1, start_y + menu_height, "+").unwrap();

        // Bottom message (centered in the bottom row)
        let cancel_msg = " press Esc to cancel ";
        let cmsg_padded = format!("{:^width$}", cancel_msg, width = menu_width as usize - 2);
        ui::print_at(start_x, start_y + menu_height + 1, &cmsg_padded).unwrap();
        ui::print_highlighted_at(start_x, start_y + menu_height + 1, "|").unwrap();
        ui::print_highlighted_at(start_x + menu_width - 1, start_y + menu_height + 1, "|").unwrap();
        ui::print_highlighted_at(start_x, start_y + menu_height + 2, "+").unwrap();
        for x in (start_x + 1)..(start_x + menu_width - 1) {
            ui::print_highlighted_at(x, start_y + menu_height + 2, "-").unwrap();
        }
        ui::print_highlighted_at(start_x + menu_width - 1, start_y + menu_height + 2, "+").unwrap();

        // Draw menu items - selected item is highlighted with reverse video
        for (i, (key, item, is_submenu)) in menu_items.iter().enumerate() {
            let suffix = if *is_submenu { " >" } else { "" };
            let prefix = { " " };
            let item_str = format!("{}{}) {}{}", prefix, key, item, suffix);
            let item_padded = format!("{:<width$}", item_str, width = menu_width as usize - 2);

            if i == selected {
                // Highlight selected item (white on black - same as borders)
                ui::print_at(start_x + 1, start_y + 3 + i as u16, &item_padded).unwrap();
            } else {
                ui::print_at(start_x + 1, start_y + 3 + i as u16, &item_padded).unwrap();
            }
        }

        // Position cursor
        ui::move_cursor(start_x + 2, start_y + 3 + selected as u16).unwrap();

        // Read key - accept both arrow keys and letter keys
        match ui::read_key().unwrap().code {
            KeyCode::Up => {
                selected = selected.saturating_sub(1);
            }
            KeyCode::Down => {
                if selected < menu_items.len() - 1 {
                    selected += 1;
                }
            }
            KeyCode::Enter => return Some((selected, menu_area)),
            KeyCode::Esc => return None,
            KeyCode::Char(c) => {
                // Check if user pressed a letter key
                for (i, (key, _, _)) in menu_items.iter().enumerate() {
                    if c.to_ascii_lowercase() == key.chars().next().unwrap() {
                        return Some((i, menu_area));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Show main menu and handle selection
/// Returns true if user selected "leave editor" to quit
fn show_main_menu(
    editor: &mut editor_state::EditorState,
    journal_file: &mut Option<std::fs::File>,
    mark_state: &mut crate::mark::MarkState,
    mark_anchor: &mut Option<crate::mark::MarkAnchor>,
) -> bool {
    let menu_items = [
        ("a", "leave editor     ", false),
        ("b", "help             ", false),
        ("c", "edit             ", true),
        ("d", "file operations  ", true),
        ("e", "redraw screen    ", false),
        ("f", "settings         ", true),
        ("g", "search/replace   ", true),
        ("h", "miscellaneous    ", true),
    ];

    if let Some((selected, menu_area)) = show_menu_with_area("main menu", &menu_items, None) {
        match selected {
            // a) leave editor - quit
            0 => {
                return true;
            }
            // b) help
            1 => {
                help::help(None);
            }
            // c) edit submenu
            2 => {
                show_edit_menu(editor, mark_state, mark_anchor, &menu_area);
            }
            // d) file operations submenu
            3 => {
                show_file_menu(editor, journal_file, &menu_area);
            }
            // e) redraw screen
            4 => {
                // Just return - main loop will redraw
            }
            // f) settings submenu
            5 => {
                show_settings_menu(editor, &menu_area);
            }
            // g) search/replace submenu
            6 => {
                show_search_menu(editor, &menu_area);
            }
            // h) miscellaneous submenu
            7 => {
                show_misc_menu(editor, &menu_area);
            }
            _ => {}
        }
    }
    false
}

/// Edit submenu: mark, copy, cut, paste
fn show_edit_menu(
    editor: &mut editor_state::EditorState,
    mark_state: &mut crate::mark::MarkState,
    mark_anchor: &mut Option<crate::mark::MarkAnchor>,
    prev_menu: &MenuArea,
) {
    let menu_items = [
        ("a", "mark text               ", false),
        ("b", "copy marked text       ", false),
        ("c", "cut (delete) marked text", false),
        ("d", "paste                  ", false),
    ];

    if let Some((selected, _)) = show_menu_with_area("edit menu", &menu_items, Some(prev_menu)) {
        match selected {
            0 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    let buff = buff_rc.borrow();
                    crate::mark::slct(mark_state, &buff, crate::mark::MarkMode::Mark);
                    *mark_anchor = crate::mark::MarkAnchor::from_buffer(&buff);
                    editor.mark_text = true;
                    show_message_prompt("Mark mode activated. Use cursor to select text.");
                }
            }
            1 => {
                crate::mark::copy(mark_state);
                editor.mark_text = false;
                show_message_prompt("Marked text copied.");
            }
            2 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    if let Some(anchor) = mark_anchor.take() {
                        crate::mark::cut(
                            mark_state,
                            &mut buff_rc.borrow_mut(),
                            anchor.line_idx,
                            anchor.pos,
                        );
                    }
                }
                editor.mark_text = false;
                show_message_prompt("Marked text cut.");
            }
            3 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    crate::mark::paste(mark_state, &mut buff_rc.borrow_mut());
                }
                show_message_prompt("Text pasted.");
            }
            _ => {}
        }
    }
}

/// File operations submenu
fn show_file_menu(
    editor: &mut editor_state::EditorState,
    journal_file: &mut Option<std::fs::File>,
    prev_menu: &MenuArea,
) {
    let menu_items = [
        ("a", "read a file           ", false),
        ("b", "write a file          ", false),
        ("c", "save file             ", false),
        ("d", "diff with disk        ", false),
        ("e", "print editor contents ", false),
        ("f", "recover from journal  ", false),
    ];

    if let Some((selected, _)) = show_menu_with_area("file menu", &menu_items, Some(prev_menu)) {
        match selected {
            0 => {
                let start = std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string());
                if let Some(path) = file_ops::show_file_browser(&start) {
                    editor.load_file(&path);
                }
            }
            1 => {
                let name = get_user_input("Write to file: ");
                if !name.is_empty() {
                    let _ = file_ops::write_file(editor, &name);
                }
            }
            2 => {
                save_file(editor, journal_file);
            }
            3 => {
                // Diff with disk version
                if let Some(diff) = file_ops::diff_file(editor) {
                    show_message_prompt(&diff);
                } else {
                    show_message_prompt("No diff (file not saved or no on-disk version)");
                }
            }
            4 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    let buff = buff_rc.borrow();
                    let mut contents = String::new();
                    for line in &buff.lines {
                        let line_data = line;
                        contents.push_str(&line_data.line);
                        contents.push('\n');
                    }
                    use std::io::Write;
                    if let Ok(mut child) = std::process::Command::new("lp")
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                    {
                        if child
                            .stdin
                            .as_mut()
                            .unwrap()
                            .write_all(contents.as_bytes())
                            .is_ok()
                        {
                            let _ = child.wait();
                            show_message_prompt("Printed successfully via `lp`");
                        } else {
                            show_message_prompt("Failed to write to `lp`");
                        }
                    } else {
                        show_message_prompt("Failed to spawn `lp` command");
                    }
                } else {
                    show_message_prompt("No file to print");
                }
            }
            5 => {
                // Recover from journal
                if let Some(ref buff_rc) = editor.curr_buff {
                    let fname = buff_rc.borrow().file_name.clone().unwrap_or_default();
                    let jpath = journal::journal_name(&fname, None);
                    if std::path::Path::new(&jpath).exists() {
                        if let Some(buff_rc) = editor.curr_buff.clone() {
                            let _ =
                                journal::recover_from_journal(&mut buff_rc.borrow_mut(), &jpath);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Settings submenu
fn show_settings_menu(editor: &mut editor_state::EditorState, prev_menu: &MenuArea) {
    let menu_items = [
        ("a", "tabs to spaces         ", false),
        ("b", "case sensitive search ", false),
        ("c", "literal search        ", false),
        ("d", "search direction      ", false),
        ("e", "observe margins       ", false),
        ("f", "info window           ", false),
        ("g", "status line           ", false),
        ("h", "auto indent           ", false),
        ("i", "overstrike            ", false),
        ("j", "auto paragraph format ", false),
        ("k", "multi windows         ", false),
        ("l", "left margin           ", false),
        ("m", "right margin          ", false),
        ("n", "info window height    ", false),
        ("o", "text/binary mode      ", false),
        ("p", "current file type     ", false),
        ("q", "save editor config    ", false),
    ];

    if let Some((selected, _)) = show_menu_with_area("settings menu", &menu_items, Some(prev_menu))
    {
        match selected {
            0 => {
                editor.expand = !editor.expand;
                show_message_prompt(&format!("Tabs to spaces: {}", editor.expand));
            }
            1 => {
                editor.case_sen = !editor.case_sen;
                show_message_prompt(&format!("Case sensitive search: {}", editor.case_sen));
            }
            2 => {
                editor.literal = !editor.literal;
                show_message_prompt(&format!("Literal search: {}", editor.literal));
            }
            3 => {
                editor.forward = !editor.forward;
                show_message_prompt(&format!(
                    "Search direction: {}",
                    if editor.forward {
                        "forward"
                    } else {
                        "backward"
                    }
                ));
            }
            4 => {
                editor.observ_margins = !editor.observ_margins;
                show_message_prompt(&format!("Observe margins: {}", editor.observ_margins));
            }
            5 => {
                editor.info_window = !editor.info_window;
                show_message_prompt(&format!("Info window: {}", editor.info_window));
            }
            6 => {
                editor.status_line = !editor.status_line;
                show_message_prompt(&format!("Status line: {}", editor.status_line));
            }
            7 => {
                editor.indent = !editor.indent;
                show_message_prompt(&format!("Auto indent: {}", editor.indent));
            }
            8 => {
                editor.overstrike = !editor.overstrike;
                show_message_prompt(&format!("Overstrike mode: {}", editor.overstrike));
            }
            9 => {
                editor.auto_format = !editor.auto_format;
                show_message_prompt(&format!("Auto paragraph format: {}", editor.auto_format));
            }
            10 => {
                editor.windows = !editor.windows;
                show_message_prompt(&format!("Multi windows: {}", editor.windows));
            }
            11 => {
                let input = get_user_input("Left margin: ");
                if let Ok(n) = input.parse::<i32>() {
                    editor.left_margin = n;
                }
            }
            12 => {
                let input = get_user_input("Right margin: ");
                if let Ok(n) = input.parse::<i32>() {
                    editor.right_margin = n;
                }
            }
            13 => {
                let input = get_user_input("Info window height: ");
                if let Ok(n) = input.parse::<i32>() {
                    editor.info_win_height = n;
                }
            }
            14 => {
                editor.text_only = !editor.text_only;
                show_message_prompt(&format!(
                    "Text/binary mode: {}",
                    if editor.text_only { "text" } else { "binary" }
                ));
            }
            15 => {
                let lang = if let Some(buff) = &editor.curr_buff {
                    let buff = buff.borrow();
                    if let Some(ref fname) = buff.file_name {
                        crate::highlighting::lang_from_extension(fname)
                    } else {
                        "text"
                    }
                } else {
                    "text"
                };
                show_message_prompt(&format!("Current file type: {}", lang));
            }
            16 => {
                show_message_prompt("Config saved");
            }
            _ => {}
        }
    }
}

/// Search/replace submenu
fn show_search_menu(editor: &mut editor_state::EditorState, prev_menu: &MenuArea) {
    let menu_items = [
        ("a", "search for ...       ", false),
        ("b", "search forward       ", false),
        ("c", "search backward      ", false),
        ("d", "replace prompt ...   ", false),
        ("e", "replace              ", false),
    ];

    if let Some((selected, _)) =
        show_menu_with_area("search/replace menu", &menu_items, Some(prev_menu))
    {
        match selected {
            0 => {
                let s = get_user_input("Search for: ");
                if !s.is_empty() {
                    editor.srch_str = Some(s.clone());
                    if let Some(buff_rc) = editor.curr_buff.clone() {
                        search::search_forward(&mut buff_rc.borrow_mut(), &s, editor.case_sen);
                    }
                }
            }
            1 => {
                // Search forward (repeat last search)
                if let Some(ref s) = editor.srch_str {
                    if let Some(buff_rc) = editor.curr_buff.clone() {
                        if let Some(result) =
                            search::search_forward(&mut buff_rc.borrow_mut(), s, editor.case_sen)
                        {
                            show_message_prompt(&format!(
                                "Found at line {}, col {}",
                                result.line_num, result.col
                            ));
                        } else {
                            show_message_prompt("Not found");
                        }
                    }
                }
            }
            2 => {
                // Search backward (repeat last search)
                if let Some(ref s) = editor.srch_str {
                    if let Some(buff_rc) = editor.curr_buff.clone() {
                        if let Some(result) =
                            search::search_backward(&mut buff_rc.borrow_mut(), s, editor.case_sen)
                        {
                            show_message_prompt(&format!(
                                "Found at line {}, col {}",
                                result.line_num, result.col
                            ));
                        } else {
                            show_message_prompt("Not found");
                        }
                    }
                }
            }
            3 => {
                let s = get_user_input("Search: ");
                let r = get_user_input("Replace with: ");
                if !s.is_empty() {
                    editor.srch_str = Some(s.clone());
                    if let Some(buff_rc) = editor.curr_buff.clone() {
                        let count =
                            search::replace_all(&mut buff_rc.borrow_mut(), &s, &r, editor.case_sen);
                        show_message_prompt(&format!("Replaced {} occurrences", count));
                    }
                }
            }
            4 => {
                if let Some(ref s) = editor.srch_str {
                    let r = get_user_input("Replace with: ");
                    if let Some(buff_rc) = editor.curr_buff.clone() {
                        search::replace_next(&mut buff_rc.borrow_mut(), s, &r, editor.case_sen);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Miscellaneous submenu
fn show_misc_menu(editor: &mut editor_state::EditorState, prev_menu: &MenuArea) {
    let menu_items = [
        ("a", "format paragraph   ", false),
        ("b", "shell command     ", false),
        ("c", "check spelling   ", false),
    ];

    if let Some((selected, _)) =
        show_menu_with_area("miscellaneous menu", &menu_items, Some(prev_menu))
    {
        match selected {
            0 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    format::format_paragraph(
                        &mut buff_rc.borrow_mut(),
                        editor.left_margin,
                        editor.right_margin,
                        editor.right_justify,
                    );
                }
                show_message_prompt("Paragraph formatted.");
            }
            1 => {
                let cmd = get_user_input("Shell command: ");
                if !cmd.is_empty() {
                    let _ = windows::restore_term();
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&cmd)
                        .status();
                    println!("\nPress Enter to continue...");
                    let mut s = String::new();
                    let _ = std::io::stdin().read_line(&mut s);
                    let _ = windows::set_up_term();
                }
            }
            2 => {
                if let Some(buff_rc) = editor.curr_buff.clone() {
                    let buff = buff_rc.borrow();
                    let mut contents = String::new();
                    for line in &buff.lines {
                        let line_data = line;
                        contents.push_str(&line_data.line);
                        contents.push('\n');
                    }

                    let _ = windows::restore_term();
                    use std::io::Write;
                    if let Ok(mut child) = std::process::Command::new("spell")
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                    {
                        let _ = child.stdin.as_mut().unwrap().write_all(contents.as_bytes());
                        let _ = child.wait();
                    } else {
                        println!("Failed to run `spell` command. Is it installed?");
                    }

                    println!("\nPress Enter to continue...");
                    let mut s = String::new();
                    let _ = std::io::stdin().read_line(&mut s);
                    let _ = windows::set_up_term();
                }
            }
            _ => {}
        }
    }
}
