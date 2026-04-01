// Syntax highlighting module supporting Arborium (Tree-sitter) and basic regex state machine

use lazy_static::lazy_static;
use std::collections::HashSet;
use arborium::{AnsiHighlighter, detect_language, get_language};
use arborium::theme::builtin;

lazy_static! {
    static ref C_KEYWORDS: HashSet<&'static str> = {
        let mut set = HashSet::new();
        let keywords = [
            "auto", "break", "case", "char", "const", "continue", "default", "do", "double",
            "else", "enum", "extern", "float", "for", "goto", "if", "int", "long", "register",
            "return", "short", "signed", "sizeof", "static", "struct", "switch", "typedef",
            "union", "unsigned", "void", "volatile", "while",
        ];
        for kw in &keywords {
            set.insert(*kw);
        }
        set
    };
    static ref RUST_KEYWORDS: HashSet<&'static str> = {
        let mut set = HashSet::new();
        let keywords = [
            "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
            "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super",
            "trait", "true", "type", "union", "unsafe", "use", "where", "while",
        ];
        for kw in &keywords {
            set.insert(*kw);
        }
        set
    };
}

/// Token types produced by the local syntax highlighter
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Keyword,
    Identifier,
    Number,
    StringLiteral,
    Comment,
    Operator,
    Whitespace,
    Function,
    Type,
    /// Pre-colored ANSI string
    Ansi(String),
}

/// Determine whether `word` is a keyword for the given language.
pub fn is_keyword(word: &str, lang: &str) -> bool {
    match lang {
        "c" | "cpp" | "c++" => C_KEYWORDS.contains(word),
        "rust" => RUST_KEYWORDS.contains(word),
        _ => C_KEYWORDS.contains(word) || RUST_KEYWORDS.contains(word),
    }
}

/// Guess the language from a file extension.
pub fn lang_from_extension(path: &str) -> &'static str {
    detect_language(path).unwrap_or("text")
}

// ── Tree-sitter integration via Arborium ─────────────────────────────────────

pub struct TsHighlighter {
    pub lang_id: String,
    pub highlighter: AnsiHighlighter,
}

impl TsHighlighter {
    pub fn new(lang_name: &str) -> Option<Self> {
        if get_language(lang_name).is_some() {
            let theme = builtin::catppuccin_mocha().clone();
            Some(TsHighlighter { 
                lang_id: lang_name.to_string(),
                highlighter: AnsiHighlighter::new(theme)
            })
        } else {
            None
        }
    }

    pub fn highlight_line(&mut self, text: &str) -> Vec<(String, TokenKind)> {
        match self.highlighter.highlight(&self.lang_id, text) {
            Ok(ansi_string) => {
                // We store the original text in TokenKind::Ansi for selection logic
                vec![(ansi_string, TokenKind::Ansi(text.to_string()))]
            }
            Err(_) => vec![(text.to_string(), TokenKind::Identifier)],
        }
    }
}

// ── LSP semantic-token helpers ───────────────────────────────────────────────

pub fn lsp_token_type_to_kind(type_name: &str) -> TokenKind {
    match type_name {
        "keyword" | "modifier" => TokenKind::Keyword,
        "comment" => TokenKind::Comment,
        "string" => TokenKind::StringLiteral,
        "number" => TokenKind::Number,
        "operator" => TokenKind::Operator,
        "function" | "method" => TokenKind::Function,
        "type" | "class" | "struct" | "enum" => TokenKind::Type,
        _ => TokenKind::Identifier,
    }
}

pub fn highlight_line_lsp(
    line: &str,
    tokens: &[crate::lsp::SemanticToken],
    legend: &[String],
) -> Vec<(String, TokenKind)> {
    let mut spans: Vec<(String, TokenKind)> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let char_len = chars.len();
    let mut cursor = 0usize;

    for tok in tokens {
        let start = tok.start_char as usize;
        let end = (start + tok.length as usize).min(char_len);

        if start > cursor {
            let gap: String = chars[cursor..start].iter().collect();
            spans.push((gap, TokenKind::Identifier));
        }

        if end > start {
            let text: String = chars[start..end].iter().collect();
            let type_name = legend
                .get(tok.token_type as usize)
                .map(|s| s.as_str())
                .unwrap_or("");
            spans.push((text, lsp_token_type_to_kind(type_name)));
        }

        cursor = end;
    }

    if cursor < char_len {
        let tail: String = chars[cursor..].iter().collect();
        spans.push((tail, TokenKind::Identifier));
    }

    spans
}

// ── Legacy Regex State Machine (Fallback) ────────────────────────────────────

pub fn highlight_line_with_state<'a>(
    line: &'a str,
    lang: &str,
    mut in_block_comment: bool,
) -> (Vec<(&'a str, TokenKind)>, bool) {
    let mut spans: Vec<(&'a str, TokenKind)> = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    macro_rules! push_span {
        ($start:expr, $end:expr, $kind:expr) => {
            if $start < $end {
                spans.push((&line[$start..$end], $kind));
            }
        };
    }

    while i < len {
        if in_block_comment {
            let start = i;
            while i < len {
                if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    in_block_comment = false;
                    break;
                }
                i += 1;
            }
            push_span!(start, i, TokenKind::Comment);
            continue;
        }

        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            push_span!(i, len, TokenKind::Comment);
            i = len;
            continue;
        }

        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            push_span!(start, i, TokenKind::StringLiteral);
            continue;
        }

        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.' || bytes[i] == b'_')
            {
                i += 1;
            }
            push_span!(start, i, TokenKind::Number);
            continue;
        }

        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            let kind = if is_keyword(word, lang) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            push_span!(start, i, kind);
            continue;
        }

        if bytes[i].is_ascii_whitespace() {
            let start = i;
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            push_span!(start, i, TokenKind::Whitespace);
            continue;
        }

        push_span!(i, i + 1, TokenKind::Operator);
        i += 1;
    }

    (spans, in_block_comment)
}
