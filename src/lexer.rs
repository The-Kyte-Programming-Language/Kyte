use crate::ast::Token;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer {
            input: source.chars().collect(),
            pos: 0,
        }
    }

    fn current(&self) -> Option<char> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.current();
        self.pos += 1;
        ch
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.current() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        // // 주석 처리
        if self.current() == Some('/') && self.input.get(self.pos + 1) == Some(&'/') {
            while let Some(ch) = self.current() {
                if ch == '\n' { break; }
                self.advance();
            }
        }
    }

    fn read_string(&mut self) -> Token {
        self.advance(); // 여는 " 건너뜀
        let mut s = String::new();
        while let Some(ch) = self.current() {
            if ch == '"' { self.advance(); break; }
            s.push(ch);
            self.advance();
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
            "function" => Token::Function,
            "Vault"    => Token::Vault,
            "Kill"     => Token::Kill,
            "Exit"     => Token::Exit,
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
            "alloc"    => Token::Alloc,
            "free"     => Token::Free,
            "int"      => Token::Int,
            "float"    => Token::Float,
            "string"   => Token::String,
            "bool"     => Token::Bool,
            _          => Token::Ident(s),
        }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            self.skip_comment();
            self.skip_whitespace();

            match self.current() {
                None => { tokens.push(Token::EOF); break; }
                Some(ch) => {
                    let tok = match ch {
                        '@' => { self.advance(); Token::At }
                        '#' => { self.advance(); Token::Hash }
                        '(' => { self.advance(); Token::LParen }
                        ')' => { self.advance(); Token::RParen }
                        '{' => { self.advance(); Token::LBrace }
                        '}' => { self.advance(); Token::RBrace }
                        ';' => { self.advance(); Token::Semicolon }
                        ',' => { self.advance(); Token::Comma }
                        '+' => { self.advance(); Token::Plus }
                        '*' => { self.advance(); Token::Star }
                        '<' => { self.advance(); Token::Lt }
                        '>' => { self.advance(); Token::Gt }
                        '=' => { self.advance(); Token::Eq }
                        '-' => {
                            self.advance();
                            if self.current() == Some('>') {
                                self.advance();
                                Token::Arrow
                            } else {
                                Token::Minus
                            }
                        }
                        '/' => {
                            self.advance();
                            Token::Slash
                        }
                        '.' => {
                            self.advance();
                            if self.current() == Some('.') {
                                self.advance();
                                Token::DotDot
                            } else {
                                // 단독 . 은 일단 스킵
                                continue;
                            }
                        }
                        '"' => self.read_string(),
                        c if c.is_ascii_digit() => self.read_number(),
                        c if c.is_alphabetic() || c == '_' => self.read_ident(),
                        _ => { self.advance(); continue; }
                    };
                    tokens.push(tok);
                }
            }
        }
        tokens
    }
}