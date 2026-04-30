use crate::ast::*;
use crate::parser::Parser;

impl Parser {
    // catch (string param) { body } 블록 파싱 헬퍼
    fn parse_catch(&mut self) -> (Option<String>, Option<Vec<(Stmt, Span)>>) {
        if self.current() != &Token::Catch {
            return (None, None);
        }
        self.advance(); // consume 'catch'
        self.expect(&Token::LParen);
        // 선택적 타입 어노테이션 소비 (string, int 등)
        match self.current() {
            Token::String | Token::Int | Token::Float | Token::Bool
            | Token::Auto | Token::TyI8 | Token::TyI16 | Token::TyI32 | Token::TyI64
            | Token::TyU8 | Token::TyU16 | Token::TyU32 | Token::TyU64 => {
                self.advance();
            }
            _ => {}
        }
        let param = if matches!(self.current(), Token::Ident(_)) {
            Some(self.eat_ident())
        } else {
            None
        };
        self.expect(&Token::RParen);
        self.expect(&Token::LBrace);
        let body = self.parse_body();
        self.expect(&Token::RBrace);
        (param, Some(body))
    }

    // 앵커 종류 파싱 (공통 헬퍼)
    pub(super) fn parse_anchor_kind(&mut self) -> AnchorKind {
        // 빈 앵커: @name()
        if self.current() == &Token::RParen {
            return AnchorKind::Plain;
        }

        let kind_ident = self.eat_ident();
        match kind_ident.as_str() {
            "main" => AnchorKind::Main,
            "thread" => AnchorKind::Thread,
            "event" => {
                self.expect(&Token::LParen);
                let event_name = self.eat_ident();
                self.expect(&Token::RParen);
                AnchorKind::Event(event_name)
            }
            k => {
                self.errors.push(format!(
                    "Unknown anchor kind: {} at line {}:{}",
                    k,
                    self.current_line(),
                    self.current_col()
                ));
                AnchorKind::Plain
            }
        }
    }

    // 블록 내부 인라인 앵커 파싱 (중괄호 필수)
    pub(super) fn parse_inline_anchor(&mut self) -> (Stmt, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        let body = self.parse_body();
        self.expect(&Token::RBrace);

        let (catch_param, catch_body) = self.parse_catch();
        (Stmt::InlineAnchor { name, kind, body, catch_param, catch_body }, span)
    }

    // 최상위 앵커 파싱 @이름(형태) — 중괄호 필수
    pub(super) fn parse_anchor(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        // 본문 + 자식 앵커
        let mut body = Vec::new();
        let mut children = Vec::new();

        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At => children.push(self.parse_child_anchor()),
                _ => body.push(self.parse_stmt()),
            }
        }
        self.expect(&Token::RBrace);

        let (catch_param, catch_body) = self.parse_catch();
        (
            TopLevel::Anchor {
                name,
                kind,
                body,
                children,
                catch_param,
                catch_body,
            },
            span,
        )
    }

    // 자식 앵커 파싱 (인라인과 동일하지만 TopLevel 반환)
    pub(super) fn parse_child_anchor(&mut self) -> (TopLevel, Span) {
        let span = self.current_span();
        self.expect(&Token::At);
        let name = self.eat_ident();
        self.expect(&Token::LParen);
        let kind = self.parse_anchor_kind();
        self.expect(&Token::RParen);

        self.expect(&Token::LBrace);
        let mut body = Vec::new();
        let mut children = Vec::new();
        loop {
            match self.current() {
                Token::RBrace | Token::EOF => break,
                Token::Hash => self.skip_decorator(),
                Token::At => children.push(self.parse_child_anchor()),
                _ => body.push(self.parse_stmt()),
            }
        }
        self.expect(&Token::RBrace);

        let (catch_param, catch_body) = self.parse_catch();
        (
            TopLevel::Anchor {
                name,
                kind,
                body,
                children,
                catch_param,
                catch_body,
            },
            span,
        )
    }
}
