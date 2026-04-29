use crate::analyzer::Analyzer;
use crate::ast::*;

impl Analyzer {
    pub(super) fn stmt_is_early_exit(stmt: &Stmt) -> bool {
        matches!(
            stmt,
            Stmt::Return(_) | Stmt::Exit | Stmt::Break | Stmt::Kill(_) | Stmt::Yield(_)
        )
    }

    pub(super) fn block_has_break(stmts: &[(Stmt, Span)]) -> bool {
        for (stmt, _) in stmts {
            match stmt {
                Stmt::Break => return true,
                Stmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if Self::block_has_break(then_body) {
                        return true;
                    }
                    if let Some(eb) = else_body {
                        if Self::block_has_break(eb) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// 블록이 반드시 일찍 종료(return/exit/break/kill/yield)되는지 확인
    pub(super) fn block_definitely_exits(stmts: &[(Stmt, Span)]) -> bool {
        for (stmt, _) in stmts {
            if Self::stmt_is_early_exit(stmt) {
                return true;
            }
            // if/else 양쪽 모두 exit하면 전체가 exit
            if let Stmt::If {
                then_body,
                else_body: Some(else_body),
                ..
            } = stmt
            {
                if Self::block_definitely_exits(then_body)
                    && Self::block_definitely_exits(else_body)
                {
                    return true;
                }
            }
        }
        false
    }
}
