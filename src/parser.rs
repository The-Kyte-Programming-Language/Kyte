use crate::ast::*;

#[path = "parser/anchor.rs"]
mod anchor;
#[path = "parser/expr.rs"]
mod expr;
#[path = "parser/fstring.rs"]
mod fstring;
#[path = "parser/items.rs"]
mod items;
#[path = "parser/stmt.rs"]
mod stmt;
#[path = "parser/toplevel.rs"]
mod toplevel;
#[path = "parser/ty.rs"]
mod ty;
#[path = "parser/util.rs"]
mod util;

pub struct Parser {
    tokens: Vec<Token>,
    lines: Vec<usize>,
    cols: Vec<usize>,
    pos: usize,
    pub errors: Vec<String>,
    depth: usize,
    no_struct_init: bool,
    enum_names: std::collections::HashSet<String>,
}

impl Parser {
    pub fn new(spanned_tokens: Vec<(Token, usize, usize)>) -> Self {
        let mut tokens = Vec::with_capacity(spanned_tokens.len());
        let mut lines = Vec::with_capacity(spanned_tokens.len());
        let mut cols = Vec::with_capacity(spanned_tokens.len());
        for (tok, line, col) in spanned_tokens {
            tokens.push(tok);
            lines.push(line);
            cols.push(col);
        }
        Parser {
            tokens,
            lines,
            cols,
            pos: 0,
            errors: Vec::new(),
            depth: 0,
            no_struct_init: false,
            enum_names: std::collections::HashSet::new(),
        }
    }

    // 전체 파싱
    pub fn parse(&mut self) -> Program {
        let mut items = Vec::new();
        loop {
            match self.current() {
                Token::EOF => break,
                Token::Hash => {
                    // A10: 연속 decorator 수집 → 이어서 fn에 부착
                    let mut dec_names = Vec::new();
                    while self.current() == &Token::Hash {
                        dec_names.push(self.parse_decorator());
                    }
                    if self.current() == &Token::Function {
                        let (mut tl, sp) = self.parse_function();
                        if let TopLevel::Function {
                            ref mut decorators, ..
                        } = tl
                        {
                            decorators.extend(dec_names);
                        }
                        items.push((tl, sp));
                    }
                    // decorator가 fn이 아닌 곳에 붙은 경우 무시
                }
                Token::At => items.push(self.parse_anchor()),
                Token::Function => items.push(self.parse_function()),
                Token::Struct => items.push(self.parse_struct()),
                Token::Enum => items.push(self.parse_enum()),
                Token::Trait => items.push(self.parse_trait()),
                Token::Impl => items.push(self.parse_impl()),
                Token::Mod => items.push(self.parse_mod()),
                Token::Import => {
                    self.advance(); /* import는 현재 무시 */
                }
                Token::Const => {
                    let span = self.current_span();
                    self.advance();
                    let ty = self.parse_ty();
                    let name = self.eat_var_ident();
                    self.expect(&Token::Eq);
                    let value = self.parse_expr();
                    self.expect(&Token::Semicolon);
                    items.push((TopLevel::ConstDecl { ty, name, value }, span));
                }
                t => {
                    self.errors.push(format!(
                        "Unexpected top-level token: {:?} at line {}:{}",
                        t,
                        self.current_line(),
                        self.current_col()
                    ));
                    self.advance(); // 에러 복구: 스킵
                }
            }
        }
        Program { items }
    }
}
