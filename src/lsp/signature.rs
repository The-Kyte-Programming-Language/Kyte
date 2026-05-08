use lsp_types::{
    ParameterInformation, ParameterLabel, Position, SignatureHelp, SignatureInformation,
};

use super::imports::preprocess_source;
use super::util::ty_str;
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_signature_help(text: &str, pos: Position) -> Option<SignatureHelp> {
    // 현재 줄에서 커서 왼쪽으로 미완성 호출 찾기: foo(
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let col = (pos.character as usize).min(line.len());
    let before = &line[..col];

    // 가장 마지막 '(' 찾기
    let paren_pos = before.rfind('(')?;
    let fn_part = before[..paren_pos].trim_end();
    // 함수 이름 추출
    let fn_name: String = fn_part
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    if fn_name.is_empty() {
        return None;
    }

    // 현재 몇 번째 인수인지 쉼표 카운트
    let active_param = before[paren_pos + 1..]
        .chars()
        .filter(|&c| c == ',')
        .count() as u32;

    // AST에서 함수 정의 찾기
    let src = preprocess_source(text);
    let tokens = Lexer::new(&src).tokenize();
    let mut par = Parser::new(tokens, &src);
    let ast = par.parse();
    for (item, _) in &ast.items {
        if let TopLevel::Function {
            name,
            params,
            return_ty,
            ..
        } = item
        {
            if name != &fn_name {
                continue;
            }
            let ps: Vec<String> = params
                .iter()
                .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                .collect();
            let ret = return_ty
                .as_ref()
                .map(|t| format!(" -> {}", ty_str(t)))
                .unwrap_or_default();
            let label = format!("fn {}({}){}", name, ps.join(", "), ret);
            let param_infos: Vec<ParameterInformation> = params
                .iter()
                .map(|p| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{} {}", ty_str(&p.ty), p.name)),
                    documentation: None,
                })
                .collect();
            return Some(SignatureHelp {
                signatures: vec![SignatureInformation {
                    label,
                    documentation: None,
                    parameters: Some(param_infos),
                    active_parameter: Some(active_param),
                }],
                active_signature: Some(0),
                active_parameter: Some(active_param),
            });
        }
    }
    None
}
