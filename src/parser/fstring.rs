use crate::ast::FStringPart;
use crate::parser::Parser;

/// f-string raw 내용을 파싱하여 FStringPart 목록으로 변환
/// 예: "Hello {name}, score={x+1}" -> [Literal("Hello "), Expr(Ident("name")), Literal(", score="), Expr(BinOp...)]
pub(super) fn parse_fstring_parts(
    raw: &str,
    errors: &mut Vec<String>,
    line: usize,
) -> Vec<FStringPart> {
    let mut parts = Vec::new();
    let mut chars = raw.chars().peekable();
    let mut literal_buf = String::new();

    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                literal_buf.push('{');
            } else {
                if !literal_buf.is_empty() {
                    parts.push(FStringPart::Literal(std::mem::take(&mut literal_buf)));
                }
                let mut expr_src = String::new();
                let mut depth = 1usize;
                for ec in chars.by_ref() {
                    if ec == '{' {
                        depth += 1;
                        expr_src.push(ec);
                    } else if ec == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        expr_src.push(ec);
                    } else {
                        expr_src.push(ec);
                    }
                }
                let tokens = crate::lexer::Lexer::new(&expr_src).tokenize();
                let mut sub_parser = Parser::new(tokens);
                let expr = sub_parser.parse_expr();
                if !sub_parser.errors.is_empty() {
                    for e in sub_parser.errors {
                        errors.push(format!("f-string expr error at line {}: {}", line, e));
                    }
                }
                parts.push(FStringPart::Expr(expr));
            }
        } else if c == '}' && chars.peek() == Some(&'}') {
            chars.next();
            literal_buf.push('}');
        } else {
            literal_buf.push(c);
        }
    }

    if !literal_buf.is_empty() {
        parts.push(FStringPart::Literal(literal_buf));
    }
    parts
}
