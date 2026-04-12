use crate::ast::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            input: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 0,
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
            Token::FloatLit(s.parse().unwrap())
        } else {
            Token::IntLit(s.parse().unwrap())
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
            "fn" | "function" => Token::Function,
            "Vault" | "vault" => Token::Vault,
            "Kill"  | "kill"  => Token::Kill,
            "Exit"  | "exit"  => Token::Exit,
            "yield"    => Token::Yield,
            "return"   => Token::Return,
            "if"       => Token::If,
            "else"     => Token::Else,
            "loop"     => Token::Loop,
            "for"      => Token::For,
            "in"       => Token::In,
            "break"    => Token::Break,
            "true"     => Token::True,
            "false"    => Token::False,
            "free"     => Token::Free,
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
                        '(' => { self.advance(); Token::LParen }
                        ')' => { self.advance(); Token::RParen }
                        '{' => { self.advance(); Token::LBrace }
                        '}' => { self.advance(); Token::RBrace }
                        '[' => { self.advance(); Token::LBracket }
                        ']' => { self.advance(); Token::RBracket }
                        ';' => { self.advance(); Token::Semicolon }
                        ',' => { self.advance(); Token::Comma }
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
                                panic!("Unexpected character '&' at line {}:{} — did you mean '&&'?", start_line, start_col);
                            }
                        }
                        '|' => {
                            self.advance();
                            if self.current() == Some('|') {
                                self.advance();
                                Token::Or
                            } else {
                                panic!("Unexpected character '|' at line {}:{} — did you mean '||'?", start_line, start_col);
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
                                panic!("Unexpected character '.' at line {}:{} — did you mean '..'?", start_line, start_col);
                            }
                        }
                        '"' => self.read_string(),
                        c if c.is_ascii_digit() => self.read_number(),
                        c if c.is_alphabetic() || c == '_' => self.read_ident(),
                        _ => {
                            let c = ch;
                            self.advance();
                            panic!("Unexpected character '{}' at line {}:{}", c, start_line, start_col);
                        }
                    };
                    tokens.push((tok, start_line, start_col));
                }
            }
        }
        tokens
    }
}