use crate::ast::*;
use crate::parser::Parser;

impl Parser {
    // trait 선언 파싱: trait Name { fn method(params) -> ret; ... }
    pub(super) fn parse_trait(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Trait);
        let name = self.eat_ident();
        self.expect(&Token::LBrace);
        let mut methods = Vec::new();
        while self.current() != &Token::RBrace && self.current() != &Token::EOF {
            if self.current() == &Token::Hash {
                self.skip_decorator();
                continue;
            }
            self.expect(&Token::Function);
            let mname = self.eat_ident();
            self.expect(&Token::LParen);
            let mut params = Vec::new();
            while self.current() != &Token::RParen && self.current() != &Token::EOF {
                let ty = self.parse_ty();
                let pname = self.eat_var_ident();
                params.push(Param { ty, name: pname });
                if self.current() == &Token::Comma {
                    self.advance();
                }
            }
            self.expect(&Token::RParen);
            let return_ty = if self.current() == &Token::Arrow {
                self.advance();
                Some(self.parse_ty())
            } else {
                None
            };
            self.expect(&Token::Semicolon);
            methods.push(TraitMethod {
                name: mname,
                params,
                return_ty,
            });
        }
        self.expect(&Token::RBrace);
        (TopLevel::Trait { name, methods }, span)
    }

    // impl 선언 파싱: impl TraitName for TypeName { fn method(...) { ... } }
    pub(super) fn parse_impl(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Impl);
        let trait_name = self.eat_ident();
        let target_ty = if matches!(self.current(), Token::Ident(s) if s == "for") {
            self.advance();
            self.eat_ident()
        } else if matches!(self.current(), Token::Ident(_)) {
            self.eat_ident()
        } else {
            trait_name.clone()
        };
        self.expect(&Token::LBrace);
        let mut methods = Vec::new();
        while self.current() != &Token::RBrace && self.current() != &Token::EOF {
            if self.current() == &Token::Hash {
                self.skip_decorator();
                continue;
            }
            let (tl, sp) = self.parse_function();
            methods.push((tl, sp));
        }
        self.expect(&Token::RBrace);
        (
            TopLevel::Impl {
                trait_name,
                target_ty,
                methods,
            },
            span,
        )
    }

    // mod 선언 파싱: mod name { fn ... struct ... }
    pub(super) fn parse_mod(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Mod);
        let name = self.eat_ident();
        self.expect(&Token::LBrace);
        let mut inner_items = Vec::new();
        while self.current() != &Token::RBrace && self.current() != &Token::EOF {
            match self.current() {
                Token::Function => inner_items.push(self.parse_function()),
                Token::Struct => inner_items.push(self.parse_struct()),
                Token::Enum => inner_items.push(self.parse_enum()),
                Token::Hash => {
                    self.skip_decorator();
                }
                _ => {
                    self.errors.push(format!(
                        "Unexpected token in mod at line {}:{}",
                        self.current_line(),
                        self.current_col()
                    ));
                    self.advance();
                }
            }
        }
        self.expect(&Token::RBrace);
        (
            TopLevel::Module {
                name,
                items: inner_items,
            },
            span,
        )
    }
}
