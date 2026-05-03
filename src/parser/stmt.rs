use crate::ast::*;
use crate::parser::Parser;

impl Parser {
    pub(super) fn parse_ident_stmt(&mut self, name: String, span: Span) -> Stmt {
        // 구조체/enum 타입 변수 선언: User u = User { ... }; or Color c = Color.Red;
        if let Token::Ident(var_name) = self.current().clone() {
            let ty = if self.enum_names.contains(&name) {
                Ty::Enum(name)
            } else {
                Ty::Struct(name)
            };
            self.advance();
            self.expect(&Token::Eq);
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return Stmt::VarDecl {
                ty,
                name: var_name,
                value,
            };
        }

        // 구조체 배열 타입 변수 선언: User[] users = [...];
        if self.current() == &Token::LBracket
            && self.peek(1) == &Token::RBracket
            && matches!(self.peek(2), Token::Ident(_))
        {
            self.expect(&Token::LBracket);
            self.expect(&Token::RBracket);
            let var_name = self.eat_ident();
            self.expect(&Token::Eq);
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return Stmt::VarDecl {
                ty: Ty::Array(Box::new(Ty::Struct(name))),
                name: var_name,
                value,
            };
        }

        let base = if self.current() == &Token::LParen {
            // 함수 호출: name(args...)
            self.advance();
            let mut args = Vec::new();
            while self.current() != &Token::RParen {
                args.push(self.parse_expr());
                if self.current() == &Token::Comma {
                    self.advance();
                }
            }
            self.expect(&Token::RParen);
            Expr::Call { name, args, span }
        } else {
            Expr::Ident(name, span)
        };
        let postfix = self.parse_postfix_expr(base);
        let expr = self.parse_pipe_tail(postfix);

        // 복합 대입 연산자 (+=, -=, *=, /=, %=)
        let compound_op = match self.current() {
            Token::PlusEq => Some(BinOpKind::Add),
            Token::MinusEq => Some(BinOpKind::Sub),
            Token::StarEq => Some(BinOpKind::Mul),
            Token::SlashEq => Some(BinOpKind::Div),
            Token::PercentEq => Some(BinOpKind::Mod),
            _ => None,
        };
        if let Some(op) = compound_op {
            self.advance();
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            if let Expr::Ident(var_name, _) = expr {
                return Stmt::CompoundAssign {
                    name: var_name,
                    op,
                    value,
                };
            }
            self.errors.push(format!(
                "Invalid compound assignment target at line {}:{}",
                self.current_line(),
                self.current_col()
            ));
            return Stmt::ExprStmt(Expr::IntLit(0));
        }

        if self.current() == &Token::Eq {
            self.advance();
            let value = self.parse_expr();
            self.expect(&Token::Semicolon);
            return match expr {
                Expr::Ident(var_name, _) => Stmt::Assign {
                    name: var_name,
                    value,
                },
                Expr::Index { array, index } => match *array {
                    Expr::Ident(var_name, _) => Stmt::IndexAssign {
                        name: var_name,
                        index: *index,
                        value,
                    },
                    _ => {
                        self.errors.push(format!(
                            "Invalid index assignment target at line {}:{}",
                            self.current_line(),
                            self.current_col()
                        ));
                        Stmt::ExprStmt(Expr::IntLit(0))
                    }
                },
                Expr::FieldAccess { base, field } => match *base {
                    Expr::Ident(var_name, _) => Stmt::FieldAssign {
                        name: var_name,
                        field,
                        value,
                    },
                    _ => {
                        self.errors.push(format!(
                            "Invalid field assignment target at line {}:{}",
                            self.current_line(),
                            self.current_col()
                        ));
                        Stmt::ExprStmt(Expr::IntLit(0))
                    }
                },
                _ => {
                    self.errors.push(format!(
                        "Invalid assignment target at line {}:{}",
                        self.current_line(),
                        self.current_col()
                    ));
                    Stmt::ExprStmt(Expr::IntLit(0))
                }
            };
        }

        // Semicolon is optional after a pipe-chain expression statement
        if self.current() == &Token::Semicolon {
            self.advance();
        }
        Stmt::ExprStmt(expr)
    }

    // 구문 파싱
    pub(super) fn parse_stmt(&mut self) -> (Stmt, Span) {
        let span = self.current_span();
        let stmt = match self.current().clone() {
            // Kill — 표현식(문자열 연결 등) 허용
            Token::Kill => {
                self.advance();
                if self.current() == &Token::Semicolon {
                    self.advance();
                    Stmt::Kill(None)
                } else {
                    let e = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    Stmt::Kill(Some(e))
                }
            }
            // Exit
            Token::Exit => {
                self.advance();
                self.expect(&Token::Semicolon);
                Stmt::Exit
            }
            // yield
            Token::Yield => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::Yield(e)
            }
            // return
            Token::Return => {
                self.advance();
                if self.current() == &Token::Semicolon {
                    self.advance();
                    Stmt::Return(None)
                } else {
                    let e = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    Stmt::Return(Some(e))
                }
            }
            // break
            Token::Break => {
                self.advance();
                self.expect(&Token::Semicolon);
                Stmt::Break
            }
            // continue
            Token::Continue => {
                self.advance();
                self.expect(&Token::Semicolon);
                Stmt::Continue
            }
            // print(expr, ...)
            Token::Print => {
                self.advance();
                self.expect(&Token::LParen);
                let mut args = Vec::new();
                while self.current() != &Token::RParen {
                    args.push(self.parse_expr());
                    if self.current() == &Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RParen);
                self.expect(&Token::Semicolon);
                Stmt::Print(args)
            }
            // assert(cond) or assert(cond, "msg")
            Token::Assert => {
                self.advance();
                self.expect(&Token::LParen);
                let cond = self.parse_expr();
                let message = if self.current() == &Token::Comma {
                    self.advance();
                    Some(self.parse_expr())
                } else {
                    None
                };
                self.expect(&Token::RParen);
                self.expect(&Token::Semicolon);
                Stmt::Assert { cond, message }
            }
            // if
            Token::If => {
                self.advance();
                let prev = self.no_struct_init;
                self.no_struct_init = true;
                let cond = self.parse_expr();
                self.no_struct_init = prev;
                self.expect(&Token::LBrace);
                let then_body = self.parse_body();
                self.expect(&Token::RBrace);
                let else_body = if self.current() == &Token::Else {
                    self.advance();
                    if self.current() == &Token::If {
                        // else if → 중첩 if로 변환
                        Some(vec![self.parse_stmt()])
                    } else {
                        self.expect(&Token::LBrace);
                        let b = self.parse_body();
                        self.expect(&Token::RBrace);
                        Some(b)
                    }
                } else {
                    None
                };
                Stmt::If {
                    cond,
                    then_body,
                    else_body,
                }
            }
            // loop
            Token::Loop => {
                self.advance();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::Loop(body)
            }
            // while
            Token::While => {
                self.advance();
                let cond = self.parse_expr();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::While { cond, body }
            }
            // for
            Token::For => {
                self.advance();
                let var = self.eat_ident();
                self.expect(&Token::In);
                let from = self.parse_primary();
                self.expect(&Token::DotDot);
                let to = self.parse_primary();
                self.expect(&Token::LBrace);
                let body = self.parse_body();
                self.expect(&Token::RBrace);
                Stmt::For {
                    var,
                    from,
                    to,
                    body,
                }
            }
            // Vault 선언
            Token::Vault => {
                self.advance();
                let ty = self.parse_ty();
                let name = self.eat_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VaultDecl { ty, name, value }
            }
            // 변수 선언 or 대입 or 표현식
            Token::Int
            | Token::Float
            | Token::String
            | Token::Bool
            | Token::TyI8
            | Token::TyI16
            | Token::TyI32
            | Token::TyI64
            | Token::TyU8
            | Token::TyU16
            | Token::TyU32
            | Token::TyU64 => {
                let ty = self.parse_ty();
                let name = self.eat_var_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VarDecl { ty, name, value }
            }
            // A07: auto 타입 추론
            Token::Auto => {
                self.advance();
                let name = self.eat_var_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::VarDecl {
                    ty: Ty::Auto,
                    name,
                    value,
                }
            }
            // match 문
            Token::Match => self.parse_match_stmt(),
            // const 선언
            Token::Const => {
                self.advance();
                let ty = self.parse_ty();
                let name = self.eat_var_ident();
                self.expect(&Token::Eq);
                let value = self.parse_expr();
                self.expect(&Token::Semicolon);
                Stmt::ConstDecl { ty, name, value }
            }
            Token::Ident(name) => {
                let span = self.current_span();
                self.advance();
                self.parse_ident_stmt(name, span)
            }
            t => {
                self.errors.push(format!(
                    "Unexpected token in statement: {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                // 에러 복구: 세미콜론 또는 닫는 중괄호까지 스킵
                while !matches!(
                    self.current(),
                    Token::Semicolon | Token::RBrace | Token::EOF
                ) {
                    self.advance();
                }
                if self.current() == &Token::Semicolon {
                    self.advance();
                }
                Stmt::ExprStmt(Expr::IntLit(0))
            }
        };
        (stmt, span)
    }

    pub(super) fn parse_body(&mut self) -> Vec<(Stmt, Span)> {
        let mut stmts = Vec::new();
        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At => stmts.push(self.parse_inline_anchor()),
                _ => stmts.push(self.parse_stmt()),
            }
        }
        stmts
    }

    // match 표현식 파싱: match expr { pattern => { body }, ... }
    pub(super) fn parse_match_stmt(&mut self) -> Stmt {
        self.expect(&Token::Match);
        self.no_struct_init = true;
        let expr = self.parse_expr();
        self.no_struct_init = false;
        self.expect(&Token::LBrace);

        let mut arms = Vec::new();
        while self.current() != &Token::RBrace && self.current() != &Token::EOF {
            let pattern = self.parse_pattern();
            let guard = if self.current() == &Token::When {
                self.advance();
                Some(self.parse_expr())
            } else {
                None
            };
            self.expect(&Token::FatArrow);
            self.expect(&Token::LBrace);
            let body = self.parse_body();
            self.expect(&Token::RBrace);
            arms.push(MatchArm { pattern, guard, body });
            // optional trailing comma
            if self.current() == &Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RBrace);

        Stmt::Match { expr, arms }
    }

    // 패턴 파싱
    pub(super) fn parse_pattern(&mut self) -> Pattern {
        match self.current().clone() {
            Token::IntLit(n) => {
                self.advance();
                Pattern::IntLit(n)
            }
            Token::FloatLit(f) => {
                self.advance();
                Pattern::FloatLit(f)
            }
            Token::StringLit(s) => {
                self.advance();
                Pattern::StringLit(s)
            }
            Token::True => {
                self.advance();
                Pattern::Bool(true)
            }
            Token::False => {
                self.advance();
                Pattern::Bool(false)
            }
            Token::Minus => {
                // 음수 리터럴: -42
                self.advance();
                if let Token::IntLit(n) = self.current().clone() {
                    self.advance();
                    Pattern::IntLit(-n)
                } else {
                    self.errors.push(format!(
                        "Expected integer after '-' in pattern at line {}:{}",
                        self.current_line(),
                        self.current_col()
                    ));
                    Pattern::Wildcard
                }
            }
            Token::Ident(name) => {
                self.advance();
                if name == "_" {
                    Pattern::Wildcard
                } else if self.current() == &Token::Dot {
                    // EnumName.Variant or EnumName.Variant(binding)
                    self.advance();
                    let variant = self.eat_ident();
                    let binding = if self.current() == &Token::LParen {
                        self.advance();
                        let b = self.eat_var_ident();
                        self.expect(&Token::RParen);
                        Some(b)
                    } else {
                        None
                    };
                    Pattern::EnumVariant {
                        enum_name: name,
                        variant,
                        binding,
                    }
                } else {
                    Pattern::Binding(name)
                }
            }
            _ => {
                self.errors.push(format!(
                    "Expected pattern at line {}:{}",
                    self.current_line(),
                    self.current_col()
                ));
                self.advance();
                Pattern::Wildcard
            }
        }
    }
}
