use crate::ast::*;
use crate::parser::Parser;

impl Parser {
    // 타입 파싱
    pub(super) fn parse_ty(&mut self) -> Ty {
        // fn(int, bool) -> string  함수 타입
        if self.current() == &Token::Function {
            self.advance();
            self.expect(&Token::LParen);
            let mut param_tys = Vec::new();
            while self.current() != &Token::RParen && self.current() != &Token::EOF {
                param_tys.push(self.parse_ty());
                if self.current() == &Token::Comma {
                    self.advance();
                }
            }
            self.expect(&Token::RParen);
            let ret_ty = if self.current() == &Token::Arrow {
                self.advance();
                Some(Box::new(self.parse_ty()))
            } else {
                None
            };
            return Ty::Fn(param_tys, ret_ty);
        }
        let base = match self.current().clone() {
            Token::Int => {
                self.advance();
                Ty::Int
            }
            Token::Float => {
                self.advance();
                Ty::Float
            }
            Token::String => {
                self.advance();
                Ty::String
            }
            Token::Bool => {
                self.advance();
                Ty::Bool
            }
            Token::TyI8 => {
                self.advance();
                Ty::I8
            }
            Token::TyI16 => {
                self.advance();
                Ty::I16
            }
            Token::TyI32 => {
                self.advance();
                Ty::I32
            }
            Token::TyI64 => {
                self.advance();
                Ty::I64
            }
            Token::TyU8 => {
                self.advance();
                Ty::U8
            }
            Token::TyU16 => {
                self.advance();
                Ty::U16
            }
            Token::TyU32 => {
                self.advance();
                Ty::U32
            }
            Token::TyU64 => {
                self.advance();
                Ty::U64
            }
            Token::Ident(name) => {
                self.advance();
                if self.enum_names.contains(&name) {
                    Ty::Enum(name)
                } else {
                    Ty::Struct(name)
                }
            }
            t => {
                self.errors.push(format!(
                    "Expected type but got {:?} at line {}:{}",
                    t,
                    self.current_line(),
                    self.current_col()
                ));
                Ty::Int // fallback
            }
        };
        // int[] 같은 배열 타입
        if self.current() == &Token::LBracket {
            self.advance();
            self.expect(&Token::RBracket);
            Ty::Array(Box::new(base))
        } else {
            base
        }
    }
}
