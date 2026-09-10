use crate::lang::error::{Result, err};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Number(f64),
    Duration(f64),
    Color([f32; 4]),
    Str(String),
    Symbol(String),
    Ident(String),
    Let,
    If,
    Else,
    For,
    In,
    And,
    Or,
    Not,
    True,
    False,
    Func,
    Return,
    Type,
    RecordKw,
    StructKw,
    MethodKw,
    PrivateKw,
    AliasKw,
    At,
    Import,
    Export,
    Context,
    As,
    Motion,
    Output,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    DotDot,
    DotDotEq,
    Eq,
    Bang,
    Pipe,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Lt,
    Le,
    Gt,
    Ge,
    EqEq,
    Ne,
    Newline,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub line: usize,
    /// 1 始まりの列 (文字数)
    pub col: usize,
}

pub fn lex(src: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    let mut line = 1;
    let mut line_start = 0;

    while i < chars.len() {
        let c = chars[i];
        let col = i - line_start + 1;
        let push = |tokens: &mut Vec<Token>, tok: Tok, line: usize| tokens.push(Token { tok, line, col });
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '\n' => {
                push(&mut tokens, Tok::Newline, line);
                line += 1;
                i += 1;
                line_start = i;
            }
            '#' => {
                // "# " はコメント、"##" はドキュメントコメント (docgen が読む)。それ以外は色リテラル
                if matches!(chars.get(i + 1), None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('#')) {
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                let hex: String = chars[i + 1..].iter().take_while(|c| c.is_ascii_hexdigit()).collect();
                let after = chars.get(i + 1 + hex.len()).copied();
                if !matches!(hex.len(), 3 | 4 | 6 | 8) || after.is_some_and(|c| c.is_alphanumeric()) {
                    let word: String = chars[i + 1..].iter().take_while(|c| c.is_alphanumeric()).collect();
                    return err("SyntaxError.InvalidLiteral", format!("line {line}:{col}: color literal must have 3, 4, 6 or 8 hex digits, found #{word}"));
                }
                push(&mut tokens, Tok::Color(parse_color(&hex)), line);
                i += 1 + hex.len();
            }
            '"' => {
                // エスケープは \" \\ \n \t
                let mut text = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None | Some('\n') => return err("SyntaxError.UnexpectedToken", format!("line {line}:{col}: unterminated string")),
                        Some('"') => break,
                        Some('\\') => {
                            let escaped = match chars.get(i + 1) {
                                Some('"') => '"',
                                Some('\\') => '\\',
                                Some('n') => '\n',
                                Some('t') => '\t',
                                other => return err("SyntaxError.InvalidLiteral", format!("line {line}:{col}: unknown escape \\{}", other.map(|c| c.to_string()).unwrap_or_default())),
                            };
                            text.push(escaped);
                            i += 2;
                        }
                        Some(c) => {
                            text.push(*c);
                            i += 1;
                        }
                    }
                }
                push(&mut tokens, Tok::Str(text), line);
                i += 1;
            }
            ':' if chars.get(i + 1).is_some_and(|c| c.is_alphabetic() || *c == '_') => {
                let name = take_ident(&chars, i + 1);
                i += 1 + name.len();
                push(&mut tokens, Tok::Symbol(name), line);
            }
            c if c.is_ascii_digit() => {
                let (tok, len) = lex_number(&chars, i, line, col)?;
                push(&mut tokens, tok, line);
                i += len;
            }
            c if c.is_alphabetic() || c == '_' => {
                let name = take_ident(&chars, i);
                i += name.len();
                let tok = match name.as_str() {
                    "let" => Tok::Let,
                    "if" => Tok::If,
                    "else" => Tok::Else,
                    "for" => Tok::For,
                    "in" => Tok::In,
                    "and" => Tok::And,
                    "or" => Tok::Or,
                    "not" => Tok::Not,
                    "true" => Tok::True,
                    "false" => Tok::False,
                    "func" => Tok::Func,
                    "return" => Tok::Return,
                    "type" => Tok::Type,
                    "record" => Tok::RecordKw,
                    "struct" => Tok::StructKw,
                    "method" => Tok::MethodKw,
                    "private" => Tok::PrivateKw,
                    "alias" => Tok::AliasKw,
                    "import" => Tok::Import,
                    "export" => Tok::Export,
                    "context" => Tok::Context,
                    "as" => Tok::As,
                    "motion" => Tok::Motion,
                    "output" => Tok::Output,
                    _ => Tok::Ident(name),
                };
                push(&mut tokens, tok, line);
            }
            _ => {
                let three: String = chars[i..(i + 3).min(chars.len())].iter().collect();
                let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                let (tok, len) = match (three.as_str(), two.as_str()) {
                    ("..=", _) => (Tok::DotDotEq, 3),
                    (_, "..") => (Tok::DotDot, 2),
                    (_, "->") => (Tok::Arrow, 2),
                    (_, "<=") => (Tok::Le, 2),
                    (_, ">=") => (Tok::Ge, 2),
                    (_, "==") => (Tok::EqEq, 2),
                    (_, "!=") => (Tok::Ne, 2),
                    _ => (
                        match c {
                            '(' => Tok::LParen,
                            ')' => Tok::RParen,
                            '{' => Tok::LBrace,
                            '}' => Tok::RBrace,
                            '[' => Tok::LBracket,
                            ']' => Tok::RBracket,
                            ',' => Tok::Comma,
                            ':' => Tok::Colon,
                            '.' => Tok::Dot,
                            '=' => Tok::Eq,
                            '!' => Tok::Bang,
                            '@' => Tok::At,
                            '|' => Tok::Pipe,
                            '+' => Tok::Plus,
                            '-' => Tok::Minus,
                            '*' => Tok::Star,
                            '/' => Tok::Slash,
                            '%' => Tok::Percent,
                            '^' => Tok::Caret,
                            '<' => Tok::Lt,
                            '>' => Tok::Gt,
                            _ => return err("SyntaxError.UnexpectedToken", format!("line {line}:{col}: unexpected character {c:?}")),
                        },
                        1,
                    ),
                };
                push(&mut tokens, tok, line);
                i += len;
            }
        }
    }
    tokens.push(Token { tok: Tok::Eof, line, col: chars.len() - line_start + 1 });
    Ok(tokens)
}

fn take_ident(chars: &[char], start: usize) -> String {
    chars[start..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect()
}

/// #rgb / #rgba は各桁を 2 つ並べて #rrggbb / #rrggbbaa に広げる
fn parse_color(hex: &str) -> [f32; 4] {
    let expanded: String = if hex.len() <= 4 { hex.chars().flat_map(|c| [c, c]).collect() } else { hex.to_string() };
    let ch = |i: usize| u8::from_str_radix(&expanded[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    let a = if expanded.len() == 8 { ch(6) } else { 1.0 };
    [ch(0), ch(2), ch(4), a]
}

/// 数値リテラル。単位 (ms/s/m/h) が付けば Duration (`1m23s` のように連結可)、
/// `01:23` / `01:23:45.678` はコロン形式の Duration、% が付けば 1/100 の Number
fn lex_number(chars: &[char], start: usize, line: usize, col: usize) -> Result<(Tok, usize)> {
    let invalid = |end: usize| {
        let text: String = chars[start..end].iter().collect();
        crate::lang::error::MophError::new("SyntaxError.InvalidLiteral", format!("line {line}:{col}: invalid literal {text}"))
    };
    let (value, mut i) = read_number(chars, start).ok_or_else(|| invalid(start + 1))?;

    // コロン形式
    if chars.get(i) == Some(&':') && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
        let mut parts = vec![value];
        while chars.get(i) == Some(&':') && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
            let (v, next) = read_number(chars, i + 1).ok_or_else(|| invalid(i + 1))?;
            parts.push(v);
            i = next;
        }
        if parts.len() > 3 {
            return Err(invalid(i));
        }
        let secs = parts.iter().fold(0.0, |acc, v| acc * 60.0 + v);
        return Ok((Tok::Duration(secs), i - start));
    }

    // 単位付き (連結可)
    let mut total = 0.0;
    let mut current = value;
    let mut has_unit = false;
    loop {
        let unit: String = chars[i..].iter().take_while(|c| c.is_ascii_alphabetic()).collect();
        let seconds = match unit.as_str() {
            "ms" => 0.001,
            "s" => 1.0,
            "m" => 60.0,
            "h" => 3600.0,
            "" => break,
            _ => return Err(invalid(i + unit.len())),
        };
        total += current * seconds;
        has_unit = true;
        i += unit.len();
        match read_number(chars, i) {
            Some((v, next)) => {
                current = v;
                i = next;
            }
            None => return Ok((Tok::Duration(total), i - start)),
        }
    }
    if has_unit {
        // 単位の後に単位なしの数字が続いた (例: 1m23)
        return Err(invalid(i));
    }
    if chars.get(i) == Some(&'%') {
        return Ok((Tok::Number(value / 100.0), i + 1 - start));
    }
    Ok((Tok::Number(value), i - start))
}

/// 位置 start から数値 (小数可) を読む。読めなければ None
fn read_number(chars: &[char], start: usize) -> Option<(f64, usize)> {
    let mut i = start;
    while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()))) {
        i += 1;
    }
    if i == start {
        return None;
    }
    let text: String = chars[start..i].iter().collect();
    text.parse().ok().map(|v| (v, i))
}
