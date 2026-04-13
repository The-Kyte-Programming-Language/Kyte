use crate::ast::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    pub errors: Vec<String>,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            input: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 0,
            errors: Vec::new(),
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current();
        self.pos += 1;
        match ch {
            Some('\n') => { self.line += 1; self.col = 0; }
            Some(_)    => { self.col += 1; }
            None       => {}
        }
        ch
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // 공백 제거
            while let Some(ch) = self.current() {
                if ch.is_whitespace() { self.advance(); } else { break; }
            }
            // // 주석 처리
            if self.current() == Some('/') && self.input.get(self.pos + 1) == Some(&'/') {
                while let Some(ch) = self.current() {
                    if ch == '\n' { break; }
                    self.advance();
                }
            } else {
                break;
            }
        }
    }

    fn read_string(&mut self) -> Token {
        self.advance(); // 여는 " 건너뜀
        let mut s = String::new();
        while let Some(ch) = self.current() {
            if ch == '"' { self.advance(); break; }
            if ch == '\\' {
                self.advance();
                match self.current() {
                    Some('n')  => { s.push('\n'); self.advance(); }
                    Some('t')  => { s.push('\t'); self.advance(); }
                    Some('r')  => { s.push('\r'); self.advance(); }
                    Some('\\') => { s.push('\\'); self.advance(); }
                    Some('"')  => { s.push('"');  self.advance(); }
                    Some('0')  => { s.push('\0'); self.advance(); }
                    Some('x')  => {
                        self.advance();
                        let mut hex = String::new();
                        for _ in 0..2 {
                            if let Some(c) = self.current() {
                                if c.is_ascii_hexdigit() {
                                    hex.push(c);
                                    self.advance();
                                } else { break; }
                            }
                        }
                        if let Ok(val) = u8::from_str_radix(&hex, 16) {
                            s.push(val as char);
                        } else {
                            self.errors.push(format!(
                                "Invalid hex escape '\\x{}' at line {}:{}",
                                hex, self.line, self.col
                            ));
                            s.push('?');
                        }
                    }
                    Some('u')  => {
                        self.advance();
                        let has_brace = self.current() == Some('{');
                        if has_brace { self.advance(); }
                        let mut hex = String::new();
                        let limit = if has_brace { 6 } else { 4 };
                        for _ in 0..limit {
                            if let Some(c) = self.current() {
                                if c.is_ascii_hexdigit() {
                                    hex.push(c);
                                    self.advance();
                                } else { break; }
                            }
                        }
                        if has_brace {
                            if self.current() == Some('}') { self.advance(); }
                        }
                        if let Ok(val) = u32::from_str_radix(&hex, 16) {
                            if let Some(c) = char::from_u32(val) {
                                s.push(c);
                            } else {
                                self.errors.push(format!(
                                    "Invalid unicode codepoint '\\u{}' at line {}:{}",
                                    hex, self.line, self.col
                                ));
                                s.push('\u{FFFD}');
                            }
                        } else {
                            self.errors.push(format!(
                                "Invalid unicode escape '\\u{}' at line {}:{}",
                                hex, self.line, self.col
                            ));
                            s.push('\u{FFFD}');
                        }
                    }
                    Some(c)    => { s.push('\\'); s.push(c); self.advance(); }
                    None       => { s.push('\\'); }
                }
            } else {
                s.push(ch);
                self.advance();
            }
        }
        Token::StringLit(s)
    }

    fn read_number(&mut self) -> Token {
        let mut s = String::new();
        let mut is_float = false;
        while let Some(ch) = self.current() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else if ch == '.' && !is_float {
                // '..'(범위 연산자)이면 소수점으로 처리하지 않음
                if self.input.get(self.pos + 1) == Some(&'.') {
                    break;
                }
                is_float = true;
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        if is_float {
            match s.parse::<f64>() {
                Ok(f) => Token::FloatLit(f),
                Err(_) => {
                    self.errors.push(format!(
                        "Invalid float literal '{}' at line {}:{}",
                        s, self.line, self.col
                    ));
                    Token::FloatLit(0.0)
                }
            }
        } else {
            match s.parse::<i64>() {
                Ok(n) => Token::IntLit(n),
                Err(_) => {
                    self.errors.push(format!(
                        "Invalid integer literal '{}' (overflow or bad format) at line {}:{}",
                        s, self.line, self.col
                    ));
                    Token::IntLit(0)
                }
            }
        }
    }

    fn read_ident(&mut self) -> Token {
        let mut s = String::new();
        while let Some(ch) = self.current() {
            if ch.is_alphanumeric() || ch == '_' {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        // 키워드 판별
        match s.as_str() {
            "main"     => Token::Main,
            "fn"       => Token::Function,
            "Vault"    => Token::Vault,
            "Kill"     => Token::Kill,
            "Exit"     => Token::Exit,
            "yield"    => Token::Yield,
            "return"   => Token::Return,
            "if"       => Token::If,
            "else"     => Token::Else,
            "loop"     => Token::Loop,
            "while"    => Token::While,
            "for"      => Token::For,
            "in"       => Token::In,
            "break"    => Token::Break,
            "true"     => Token::True,
            "false"    => Token::False,
            "free"     => Token::Free,
            "print"    => Token::Print,
            "as"       => Token::As,
            "struct"   => Token::Struct,
            "auto"     => Token::Auto,
            "assert"   => Token::Assert,
            "int"      => Token::Int,
            "float"    => Token::Float,
            "string"   => Token::String,
            "bool"     => Token::Bool,
            "i8"       => Token::TyI8,
            "i16"      => Token::TyI16,
            "i32"      => Token::TyI32,
            "i64"      => Token::TyI64,
            "u8"       => Token::TyU8,
            "u16"      => Token::TyU16,
            "u32"      => Token::TyU32,
            "u64"      => Token::TyU64,
            _          => Token::Ident(s),
        }
    }

    pub fn tokenize(&mut self) -> Vec<(Token, usize, usize)> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            let start_line = self.line;
            let start_col = self.col;

            match self.current() {
                None => { tokens.push((Token::EOF, start_line, start_col)); break; }
                Some(ch) => {
                    let tok = match ch {
                        '@' => { self.advance(); Token::At }
                        '#' => { self.advance(); Token::Hash }
                        '(' => { self.advance(); Token::LParen }
                        ')' => { self.advance(); Token::RParen }
                        '{' => { self.advance(); Token::LBrace }
                        '}' => { self.advance(); Token::RBrace }
                        '[' => { self.advance(); Token::LBracket }
                        ']' => { self.advance(); Token::RBracket }
                        ';' => { self.advance(); Token::Semicolon }
                        ',' => { self.advance(); Token::Comma }
                        ':' => { self.advance(); Token::Colon }
                        '+' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::PlusEq
                            } else {
                                Token::Plus
                            }
                        }
                        '*' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::StarEq
                            } else {
                                Token::Star
                            }
                        }
                        '%' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::PercentEq
                            } else {
                                Token::Percent
                            }
                        }
                        '!' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::Neq
                            } else {
                                Token::Not
                            }
                        }
                        '<' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::Le
                            } else {
                                Token::Lt
                            }
                        }
                        '>' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::Ge
                            } else {
                                Token::Gt
                            }
                        }
                        '=' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::EqEq
                            } else {
                                Token::Eq
                            }
                        }
                        '&' => {
                            self.advance();
                            if self.current() == Some('&') {
                                self.advance();
                                Token::And
                            } else {
                                self.errors.push(format!(
                                    "Unexpected character '&' at line {}:{} — did you mean '&&'?",
                                    start_line, start_col
                                ));
                                continue;
                            }
                        }
                        '|' => {
                            self.advance();
                            if self.current() == Some('|') {
                                self.advance();
                                Token::Or
                            } else {
                                self.errors.push(format!(
                                    "Unexpected character '|' at line {}:{} — did you mean '||'?",
                                    start_line, start_col
                                ));
                                continue;
                            }
                        }
                        '-' => {
                            self.advance();
                            if self.current() == Some('>') {
                                self.advance();
                                Token::Arrow
                            } else if self.current() == Some('=') {
                                self.advance();
                                Token::MinusEq
                            } else {
                                Token::Minus
                            }
                        }
                        '/' => {
                            self.advance();
                            if self.current() == Some('=') {
                                self.advance();
                                Token::SlashEq
                            } else {
                                Token::Slash
                            }
                        }
                        '.' => {
                            self.advance();
                            if self.current() == Some('.') {
                                self.advance();
                                Token::DotDot
                            } else {
                                Token::Dot
                            }
                        }
                        '"' => self.read_string(),
                        c if c.is_ascii_digit() => self.read_number(),
                        c if c.is_alphabetic() || c == '_' => self.read_ident(),
                        _ => {
                            let c = ch;
                            self.advance();
                            self.errors.push(format!(
                                "Unexpected character '{}' at line {}:{}",
                                c, start_line, start_col
                            ));
                            continue;
                        }
                    };
                    tokens.push((tok, start_line, start_col));
                }
            }
        }
        tokens
    }
}