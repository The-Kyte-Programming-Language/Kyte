use crate::ast::*;
use crate::parser::Parser;

use super::fstring::parse_fstring_parts;

const MAX_DEPTH: usize = 256;

impl Parser {
    // 표현식 파싱 (우선순위: or < and < 비교 < 덧뺄셈 < 곱나눗셈 < 단항 < primary)
    pub(super) fn parse_expr(&mut self) -> Expr {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.errors.push(format!(
                "Expression nesting too deep (>{}) at line {}:{}",
                MAX_DEPTH,
                self.current_line(),
                self.current_col()
            ));
            self.depth -= 1;
            return Expr::IntLit(0);
        }
        let result = self.parse_or();
        self.depth -= 1;
        result
    }

    fn parse_or(&mut self) -> Expr {
        let mut left = self.parse_and();
        loop {
            if self.current() != &Token::Or {
                break;
            }
            self.advance();
            let right = self.parse_and();
            left = Expr::BinOp {
                left: Box::new(left),
                op: BinOpKind::Or,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_and(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        loop {
            if self.current() != &Token::And {
                break;
            }
            self.advance();
            let right = self.parse_comparison();
            left = Expr::BinOp {
                left: Box::new(left),
                op: BinOpKind::And,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_additive();
        loop {
            let op = match self.current() {
                Token::Lt => BinOpKind::Lt,
                Token::Gt => BinOpKind::Gt,
                Token::Le => BinOpKind::Le,
                Token::Ge => BinOpKind::Ge,
                Token::EqEq => BinOpKind::Eq,
                Token::Neq => BinOpKind::Neq,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive();
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
        let mut left = self.parse_multiplicative();
        loop {
            let op = match self.current() {
                Token::Plus => BinOpKind::Add,
                Token::Minus => BinOpKind::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative();
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut left = self.parse_unary();
        loop {
            let op = match self.current() {
                Token::Star => BinOpKind::Mul,
                Token::Slash => BinOpKind::Div,
                Token::Percent => BinOpKind::Mod,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary();
            left = Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_unary(&mut self) -> Expr {
        match self.current() {
            Token::Minus => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryOp {
                    op: UnaryOpKind::Neg,
                    expr: Box::new(expr),
                }
            }
            Token::Not => {
                self.advance();
                let expr = self.parse_unary();
                Expr::UnaryOp {
                    op: UnaryOpKind::Not,
                    expr: Box::new(expr),
                }
            }
            _ => self.parse_primary(),
        }
    }

    pub(super) fn parse_primary(&mut self) -> Expr {
        let mut expr = match self.current().clone() {
            Token::IntLit(n) => {
                self.advance();
                Expr::IntLit(n)
            }
            Token::FloatLit(f) => {
                self.advance();
                Expr::FloatLit(f)
            }
            Token::StringLit(s) => {
                self.advance();
                Expr::StringLit(s)
            }
            Token::True => {
                self.advance();
                Expr::Bool(true)
            }
            Token::False => {
                self.advance();
                Expr::Bool(false)
            }
            Token::Ident(s) => {
                let span = self.current_span();
                self.advance();
                if !self.no_struct_init && self.current() == &Token::LBrace {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.current() != &Token::RBrace {
                        let fname = self.eat_ident();
                        self.expect(&Token::Colon);
                        let fexpr = self.parse_expr();
                        fields.push((fname, fexpr));
                        if self.current() == &Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(&Token::RBrace);
                    Expr::StructInit { name: s, fields }
                }
                // 함수 호출인지 확인
                else if self.current() == &Token::LParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.current() != &Token::RParen {
                        args.push(self.parse_expr());
                        if self.current() == &Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(&Token::RParen);
                    Expr::Call {
                        name: s,
                        args,
                        span,
                    }
                } else {
                    Expr::Ident(s, span)
                }
            }
            // 배열 리터럴: [1, 2, 3]
            Token::LBracket => {
                self.advance();
                let mut elems = Vec::new();
                while self.current() != &Token::RBracket {
                    elems.push(self.parse_expr());
                    if self.current() == &Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::RBracket);
                Expr::ArrayLit(elems)
            }
            Token::LParen => {
                self.advance();
                let e = self.parse_expr();
                self.expect(&Token::RParen);
                e
            }
            // 클로저: |x: int, y| { ... } 또는 |x| expr
            Token::Pipe => {
                self.advance();
                let mut closure_params = Vec::new();
                while self.current() != &Token::Pipe && self.current() != &Token::EOF {
                    let pname = self.eat_ident();
                    let ty = if self.current() == &Token::Colon {
                        self.advance();
                        Some(self.parse_ty())
                    } else {
                        None
                    };
                    closure_params.push((pname, ty));
                    if self.current() == &Token::Comma {
                        self.advance();
                    }
                }
                self.expect(&Token::Pipe);
                let body = if self.current() == &Token::LBrace {
                    self.advance();
                    let b = self.parse_body();
                    self.expect(&Token::RBrace);
                    b
                } else {
                    let e = self.parse_expr();
                    let span = self.current_span();
                    vec![(Stmt::Return(Some(e)), span)]
                };
                Expr::Closure {
                    params: closure_params,
                    body,
                }
            }
            // f-string 리터럴 파싱
            Token::FStringLit(raw) => {
                let raw = raw.clone();
                let line = self.current_line();
                self.advance();
                let mut tmp_errors = Vec::new();
                let parts = parse_fstring_parts(&raw, &mut tmp_errors, line);
                self.errors.extend(tmp_errors);
                Expr::FStringLit(parts)
            }
            t => {
                self.errors.push(format!(
                    "Unexpected token in expression: {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                self.advance();
                Expr::IntLit(0) // fallback
            }
        };
        expr = self.parse_postfix_expr(expr);
        expr
    }

    // 후위: 인덱스 접근 expr[index], 타입 캐스팅 expr as ty, 멤버 접근/메서드 호출
    pub(super) fn parse_postfix_expr(&mut self, mut expr: Expr) -> Expr {
        loop {
            match self.current() {
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr();
                    self.expect(&Token::RBracket);
                    expr = Expr::Index {
                        array: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                Token::As => {
                    self.advance();
                    let ty = self.parse_ty();
                    expr = Expr::Cast {
                        expr: Box::new(expr),
                        ty,
                    };
                }
                Token::Dot => {
                    self.advance();
                    let member = self.eat_ident();
                    // Check if base is an enum name → EnumVariant
                    if let Expr::Ident(ref base_name, _) = expr {
                        if self.enum_names.contains(base_name) {
                            // EnumName.Variant or EnumName.Variant(value)
                            let value = if self.current() == &Token::LParen {
                                self.advance();
                                let v = self.parse_expr();
                                self.expect(&Token::RParen);
                                Some(Box::new(v))
                            } else {
                                None
                            };
                            expr = Expr::EnumVariant {
                                enum_name: base_name.clone(),
                                variant: member,
                                value,
                            };
                            continue;
                        }
                    }
                    if self.current() == &Token::LParen {
                        self.advance();
                        let mut args = Vec::new();
                        while self.current() != &Token::RParen {
                            args.push(self.parse_expr());
                            if self.current() == &Token::Comma {
                                self.advance();
                            }
                        }
                        self.expect(&Token::RParen);
                        expr = Expr::MethodCall {
                            base: Box::new(expr),
                            method: member,
                            args,
                        };
                    } else {
                        expr = Expr::FieldAccess {
                            base: Box::new(expr),
                            field: member,
                        };
                    }
                }
                _ => break,
            }
        }
        expr
    }
}
