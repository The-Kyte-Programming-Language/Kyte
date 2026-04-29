use crate::ast::*;
use crate::parser::Parser;

impl Parser {
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

        (Stmt::InlineAnchor { name, kind, body }, span)
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

        (
            TopLevel::Anchor {
                name,
                kind,
                body,
                children,
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

        (
            TopLevel::Anchor {
                name,
                kind,
                body,
                children,
            },
            span,
        )
    }
}
