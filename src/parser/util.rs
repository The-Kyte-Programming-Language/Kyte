use crate::analyzer::{CompileError, Severity};
use crate::ast::*;
use crate::parser::Parser;

pub(super) fn token_name(tok: &Token) -> &'static str {
    match tok {
        Token::Semicolon => "';'",
        Token::Colon => "':'",
        Token::Comma => "','",
        Token::Dot => "'.'",
        Token::LParen => "'('",
        Token::RParen => "')'",
        Token::LBrace => "'{'",
        Token::RBrace => "'}'",
        Token::LBracket => "'['",
        Token::RBracket => "']'",
        Token::Eq => "'='",
        Token::Arrow => "'->'",
        Token::FatArrow => "'=>'",
        Token::Function => "'fn'",
        Token::Return => "'return'",
        Token::If => "'if'",
        Token::Else => "'else'",
        Token::For => "'for'",
        Token::While => "'while'",
        Token::Loop => "'loop'",
        Token::Break => "'break'",
        Token::Continue => "'continue'",
        Token::Match => "'match'",
        Token::Struct => "'struct'",
        Token::Enum => "'enum'",
        Token::Import => "'import'",
        Token::EOF => "end of file",
        Token::Ident(_) => "identifier",
        Token::IntLit(_) => "integer literal",
        Token::FloatLit(_) => "float literal",
        Token::StringLit(_) => "string literal",
        Token::FStringLit(_) => "f-string",
        Token::True => "'true'",
        Token::False => "'false'",
        Token::Hash => "'#'",
        _ => "token",
    }
}

impl Parser {
    pub(super) fn make_error(
        &self,
        code: &'static str,
        message: String,
        hint: String,
    ) -> CompileError {
        let span = self.current_span();
        let source_line = self
            .source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        CompileError {
            code,
            severity: Severity::Error,
            message,
            hint,
            span,
            source_line,
        }
    }

    pub(super) fn is_keyword(name: &str) -> bool {
        matches!(
            name,
            "main"
                | "fn"
                | "Vault"
                | "Kill"
                | "Exit"
                | "yield"
                | "return"
                | "if"
                | "else"
                | "loop"
                | "while"
                | "for"
                | "in"
                | "break"
                | "true"
                | "false"
                | "free"
                | "print"
                | "as"
                | "struct"
                | "int"
                | "float"
                | "string"
                | "bool"
                | "auto"
                | "assert"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "enum"
                | "match"
                | "trait"
                | "impl"
                | "mod"
                | "const"
                | "import"
        )
    }

    /// 일반 식별자 컨텍스트에서 키워드는 변수명으로 사용 불가 (M02)
    pub(super) fn eat_var_ident(&mut self) -> String {
        let name = self.eat_ident();
        if Self::is_keyword(&name) {
            let e = self.make_error(
                "P001",
                format!("'{}' is a reserved keyword and cannot be used as a name", name),
                format!("Rename this to something other than '{}'", name),
            );
            self.errors.push(e);
        }
        name
    }

    pub(super) fn current(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::EOF)
    }

    pub(super) fn current_col(&self) -> usize {
        self.cols.get(self.pos).copied().unwrap_or(0)
    }

    pub(super) fn current_line(&self) -> usize {
        self.lines.get(self.pos).copied().unwrap_or(0)
    }

    pub(super) fn current_len(&self) -> usize {
        self.lens.get(self.pos).copied().unwrap_or(1)
    }

    pub(super) fn current_span(&self) -> Span {
        Span {
            line: self.current_line(),
            col: self.current_col(),
            len: self.current_len(),
        }
    }

    pub(super) fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    pub(super) fn peek(&self, offset: usize) -> &Token {
        self.tokens.get(self.pos + offset).unwrap_or(&Token::EOF)
    }

    pub(super) fn skip_decorator(&mut self) {
        self.parse_decorator();
    }

    /// #[name] 또는 #name 또는 #name(...) 형태의 decorator를 파싱하여 이름을 반환합니다.
    pub(super) fn parse_decorator(&mut self) -> String {
        self.expect(&Token::Hash);

        // #[name] 형태 지원
        let has_bracket = self.current() == &Token::LBracket;
        if has_bracket {
            self.advance();
        }

        let mut name = self.eat_ident();

        if self.current() == &Token::LParen {
            self.advance();
            let mut depth = 1usize;
            let mut arg_text = String::new();
            while depth > 0 {
                match self.current() {
                    Token::LParen => {
                        if depth >= 1 {
                            arg_text.push('(');
                        }
                        depth += 1;
                        self.advance();
                    }
                    Token::RParen => {
                        depth -= 1;
                        if depth > 0 {
                            arg_text.push(')');
                        }
                        self.advance();
                    }
                    Token::EOF => {
                        let e = self.make_error(
                            "P002",
                            "Unclosed decorator arguments — reached end of file".to_string(),
                            "Add a closing ')' to complete the decorator".to_string(),
                        );
                        self.errors.push(e);
                        break;
                    }
                    Token::Comma => {
                        arg_text.push(',');
                        self.advance();
                    }
                    Token::Ident(s) => {
                        arg_text.push_str(s);
                        self.advance();
                    }
                    Token::IntLit(n) => {
                        arg_text.push_str(&n.to_string());
                        self.advance();
                    }
                    Token::FloatLit(f) => {
                        arg_text.push_str(&f.to_string());
                        self.advance();
                    }
                    Token::StringLit(s) => {
                        arg_text.push('"');
                        arg_text.push_str(s);
                        arg_text.push('"');
                        self.advance();
                    }
                    Token::True => {
                        arg_text.push_str("true");
                        self.advance();
                    }
                    Token::False => {
                        arg_text.push_str("false");
                        self.advance();
                    }
                    Token::Minus => {
                        arg_text.push('-');
                        self.advance();
                    }
                    Token::Dot => {
                        arg_text.push('.');
                        self.advance();
                    }
                    _ => {
                        self.advance();
                    }
                }
            }
            name = format!("{}({})", name, arg_text.trim());
        }

        if has_bracket && self.current() == &Token::RBracket {
            self.advance();
        }

        name
    }

    pub(super) fn expect(&mut self, expected: &Token) {
        if self.current() == expected {
            self.advance();
        } else {
            let e = self.make_error(
                "P003",
                format!(
                    "Expected {} but found {}",
                    token_name(expected),
                    token_name(self.current())
                ),
                format!("Add or correct the {} here", token_name(expected)),
            );
            self.errors.push(e);
            // 에러 복구: 동기화 토큰까지 스킵 (세미콜론, 중괄호 등)
            if !matches!(
                self.current(),
                Token::Semicolon | Token::RBrace | Token::RParen | Token::RBracket | Token::EOF
            ) {
                self.advance();
            }
        }
    }

    pub(super) fn eat_ident(&mut self) -> String {
        match self.current().clone() {
            Token::Ident(s) => {
                self.advance();
                s
            }
            // 키워드도 앵커 이름/종류로 쓸 수 있게 허용
            Token::Main => {
                self.advance();
                "main".to_string()
            }
            Token::Function => {
                self.advance();
                "function".to_string()
            }
            Token::Vault => {
                self.advance();
                "vault".to_string()
            }
            Token::Kill => {
                self.advance();
                "kill".to_string()
            }
            Token::Exit => {
                self.advance();
                "exit".to_string()
            }
            Token::Yield => {
                self.advance();
                "yield".to_string()
            }
            Token::Return => {
                self.advance();
                "return".to_string()
            }
            Token::If => {
                self.advance();
                "if".to_string()
            }
            Token::Else => {
                self.advance();
                "else".to_string()
            }
            Token::Loop => {
                self.advance();
                "loop".to_string()
            }
            Token::For => {
                self.advance();
                "for".to_string()
            }
            Token::In => {
                self.advance();
                "in".to_string()
            }
            Token::Break => {
                self.advance();
                "break".to_string()
            }
            Token::True => {
                self.advance();
                "true".to_string()
            }
            Token::False => {
                self.advance();
                "false".to_string()
            }
            Token::Print => {
                self.advance();
                "print".to_string()
            }
            Token::While => {
                self.advance();
                "while".to_string()
            }
            Token::As => {
                self.advance();
                "as".to_string()
            }
            Token::Struct => {
                self.advance();
                "struct".to_string()
            }
            Token::Int => {
                self.advance();
                "int".to_string()
            }
            Token::Float => {
                self.advance();
                "float".to_string()
            }
            Token::String => {
                self.advance();
                "string".to_string()
            }
            Token::Bool => {
                self.advance();
                "bool".to_string()
            }
            Token::TyI8 => {
                self.advance();
                "i8".to_string()
            }
            Token::TyI16 => {
                self.advance();
                "i16".to_string()
            }
            Token::TyI32 => {
                self.advance();
                "i32".to_string()
            }
            Token::TyI64 => {
                self.advance();
                "i64".to_string()
            }
            Token::TyU8 => {
                self.advance();
                "u8".to_string()
            }
            Token::TyU16 => {
                self.advance();
                "u16".to_string()
            }
            Token::TyU32 => {
                self.advance();
                "u32".to_string()
            }
            Token::TyU64 => {
                self.advance();
                "u64".to_string()
            }
            Token::Auto => {
                self.advance();
                "auto".to_string()
            }
            Token::Assert => {
                self.advance();
                "assert".to_string()
            }
            Token::Enum => {
                self.advance();
                "enum".to_string()
            }
            Token::Match => {
                self.advance();
                "match".to_string()
            }
            Token::Trait => {
                self.advance();
                "trait".to_string()
            }
            Token::Impl => {
                self.advance();
                "impl".to_string()
            }
            Token::Mod => {
                self.advance();
                "mod".to_string()
            }
            Token::Const => {
                self.advance();
                "const".to_string()
            }
            Token::Import => {
                self.advance();
                "import".to_string()
            }
            Token::When => {
                self.advance();
                "when".to_string()
            }
            t => {
                let e = self.make_error(
                    "P004",
                    format!("Expected a name but found {}", token_name(&t)),
                    "Provide a valid identifier (a name for a variable, function, type, etc.)"
                        .to_string(),
                );
                self.errors.push(e);
                "_error_".to_string()
            }
        }
    }
}
