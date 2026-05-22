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
                if self.fn_type_params.contains(&name) {
                    Ty::TypeParam(name)
                } else if self.enum_names.contains(&name) {
                    Ty::Enum(name)
                } else {
                    Ty::Struct(name)
                }
            }
            t => {
                let e = self.make_error(
                    "P016",
                    format!("Expected a type but found {}", super::util::token_name(&t)),
                    "Valid types: int, float, string, bool, i8, i16, i32, i64, u8, u16, u32, u64, a struct or enum name, or an array type like int[]".to_string(),
                );
                self.errors.push(e);
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
