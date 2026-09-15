use crate::diagnostic::Diagnostic;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Number(String),
    String(String),
    Color(String),
    LBrace,
    RBrace,
    Colon,
    Semi,
    Equal,
    Arrow,
    Comma,
    At,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
}

pub struct LexResult {
    pub tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn lex(source: &str) -> LexResult {
    let chars: Vec<char> = source.chars().collect();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut column = 1usize;
    let mut tokens = Vec::new();
    let mut diagnostics = Vec::new();

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => { i += 1; column += 1; }
            '\n' => { i += 1; line += 1; column = 1; }
            '/' if i + 1 < chars.len() && chars[i + 1] == '/' => {
                i += 2; column += 2;
                while i < chars.len() && chars[i] != '\n' { i += 1; column += 1; }
            }
            '{' => push_simple(&mut tokens, TokenKind::LBrace, line, column, &mut i, &mut column),
            '}' => push_simple(&mut tokens, TokenKind::RBrace, line, column, &mut i, &mut column),
            ':' => push_simple(&mut tokens, TokenKind::Colon, line, column, &mut i, &mut column),
            ';' => push_simple(&mut tokens, TokenKind::Semi, line, column, &mut i, &mut column),
            '=' => push_simple(&mut tokens, TokenKind::Equal, line, column, &mut i, &mut column),
            ',' => push_simple(&mut tokens, TokenKind::Comma, line, column, &mut i, &mut column),
            '@' => push_simple(&mut tokens, TokenKind::At, line, column, &mut i, &mut column),
            '-' if i + 1 < chars.len() && chars[i + 1] == '>' => {
                tokens.push(Token { kind: TokenKind::Arrow, line, column });
                i += 2; column += 2;
            }
            '"' => {
                let start_line = line;
                let start_col = column;
                i += 1; column += 1;
                let mut value = String::new();
                let mut terminated = false;
                while i < chars.len() {
                    match chars[i] {
                        '"' => { i += 1; column += 1; terminated = true; break; }
                        '\\' if i + 1 < chars.len() => {
                            let escaped = match chars[i + 1] {
                                'n' => '\n', 't' => '\t', 'r' => '\r', '"' => '"', '\\' => '\\', other => other,
                            };
                            value.push(escaped); i += 2; column += 2;
                        }
                        '\n' => { value.push('\n'); i += 1; line += 1; column = 1; }
                        ch => { value.push(ch); i += 1; column += 1; }
                    }
                }
                if !terminated { diagnostics.push(Diagnostic::error("E001", "unterminated string literal", start_line, start_col)); }
                tokens.push(Token { kind: TokenKind::String(value), line: start_line, column: start_col });
            }
            '#' => {
                let start_col = column;
                let mut value = String::from("#");
                i += 1; column += 1;
                while i < chars.len() && chars[i].is_ascii_hexdigit() { value.push(chars[i]); i += 1; column += 1; }
                if value.len() == 1 { diagnostics.push(Diagnostic::error("E002", "expected hexadecimal color", line, start_col)); }
                tokens.push(Token { kind: TokenKind::Color(value), line, column: start_col });
            }
            ch if ch.is_ascii_digit() || (ch == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) || (ch == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) => {
                let start_col = column;
                let mut value = String::new();
                if chars[i] == '-' { value.push('-'); i += 1; column += 1; }
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') { value.push(chars[i]); i += 1; column += 1; }
                tokens.push(Token { kind: TokenKind::Number(value), line, column: start_col });
            }
            ch if is_ident_start(ch) => {
                let start_col = column;
                let mut value = String::new();
                while i < chars.len() && is_ident_continue(chars[i]) { value.push(chars[i]); i += 1; column += 1; }
                tokens.push(Token { kind: TokenKind::Ident(value), line, column: start_col });
            }
            other => {
                diagnostics.push(Diagnostic::error("E003", format!("unexpected character `{other}`"), line, column));
                i += 1; column += 1;
            }
        }
    }

    tokens.push(Token { kind: TokenKind::Eof, line, column });
    LexResult { tokens, diagnostics }
}

fn push_simple(tokens: &mut Vec<Token>, kind: TokenKind, line: usize, column: usize, i: &mut usize, current_column: &mut usize) {
    tokens.push(Token { kind, line, column });
    *i += 1;
    *current_column += 1;
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '%')
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '%')
}
