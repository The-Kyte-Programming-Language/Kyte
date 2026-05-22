use crate::ast::*;
use crate::parser::Parser;

impl Parser {
    // 함수 파싱
    pub(super) fn parse_function(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Function);
        let first_name = self.eat_ident();
        let mut method_owner: Option<String> = None;
        let name = if self.current() == &Token::Dot {
            self.advance();
            let method = self.eat_ident();
            method_owner = Some(first_name.clone());
            format!("{}.{}", first_name, method)
        } else {
            first_name
        };

        // 제네릭 타입 파라미터 파싱: function foo<T, U>(...)
        let mut type_params = Vec::new();
        if self.current() == &Token::Lt {
            self.advance();
            while self.current() != &Token::Gt && self.current() != &Token::EOF {
                let tp = self.eat_ident();
                type_params.push(tp);
                if self.current() == &Token::Comma {
                    self.advance();
                }
            }
            self.expect(&Token::Gt);
        }
        self.fn_type_params = type_params.clone();

        self.expect(&Token::LParen);

        let mut params = Vec::new();
        if let Some(owner) = &method_owner {
            params.push(Param {
                ty: Ty::Struct(owner.clone()),
                name: "self".to_string(),
            });
        }
        while self.current() != &Token::RParen {
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

        self.expect(&Token::LBrace);
        let body = self.parse_body();
        self.expect(&Token::RBrace);
        self.fn_type_params.clear();

        (
            TopLevel::Function {
                name,
                type_params,
                params,
                return_ty,
                body,
                decorators: Vec::new(),
            },
            span,
        )
    }

    // struct 선언 파싱
    pub(super) fn parse_struct(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Struct);
        let name = self.eat_ident();
        self.expect(&Token::LBrace);

        let mut fields = Vec::new();
        while self.current() != &Token::RBrace {
            let ty = self.parse_ty();
            let fname = self.eat_ident();
            self.expect(&Token::Semicolon);
            fields.push(StructField { ty, name: fname });
        }
        self.expect(&Token::RBrace);

        (TopLevel::Struct { name, fields }, span)
    }

    // enum 선언 파싱
    pub(super) fn parse_enum(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Enum);
        let name = self.eat_ident();
        self.enum_names.insert(name.clone());
        self.expect(&Token::LBrace);

        let mut variants = Vec::new();
        while self.current() != &Token::RBrace && self.current() != &Token::EOF {
            let vname = self.eat_ident();
            let ty = if self.current() == &Token::LParen {
                self.advance();
                let t = self.parse_ty();
                self.expect(&Token::RParen);
                Some(t)
            } else {
                None
            };
            variants.push(EnumVariant { name: vname, ty });
            if self.current() == &Token::Comma {
                self.advance();
            }
        }
        self.expect(&Token::RBrace);

        (TopLevel::Enum { name, variants }, span)
    }

    // extern fn 선언 파싱
    pub(super) fn parse_extern_fn(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::Extern);
        self.expect(&Token::Function);
        let name = self.eat_ident();
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
        (TopLevel::ExternFn { name, params, return_ty }, span)
    }
}
