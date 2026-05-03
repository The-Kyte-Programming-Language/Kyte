use std::collections::HashMap;
use crate::ast::*;

/// Returns true if `name` appears as a read anywhere in `expr`.
pub(super) fn expr_uses(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Ident(n, _) => n == name,
        Expr::IntLit(_) | Expr::FloatLit(_) | Expr::StringLit(_) | Expr::Bool(_) => false,
        Expr::BinOp { left, right, .. } => expr_uses(left, name) || expr_uses(right, name),
        Expr::UnaryOp { expr, .. } => expr_uses(expr, name),
        Expr::Call { args, .. } => args.iter().any(|a| expr_uses(a, name)),
        Expr::Index { array, index } => expr_uses(array, name) || expr_uses(index, name),
        Expr::FieldAccess { base, .. } => expr_uses(base, name),
        Expr::MethodCall { base, args, .. } => {
            expr_uses(base, name) || args.iter().any(|a| expr_uses(a, name))
        }
        Expr::Cast { expr, .. } => expr_uses(expr, name),
        Expr::ArrayLit(elems) => elems.iter().any(|e| expr_uses(e, name)),
        Expr::StructInit { fields, .. } => fields.iter().any(|(_, v)| expr_uses(v, name)),
        Expr::Closure { body, .. } => stmts_use(body, name),
        Expr::EnumVariant { value, .. } => {
            value.as_ref().map_or(false, |p| expr_uses(p, name))
        }
        Expr::FStringLit(parts) => parts.iter().any(|p| match p {
            FStringPart::Expr(e) => expr_uses(e, name),
            FStringPart::Literal(_) => false,
        }),
    }
}

/// Returns true if `name` appears anywhere in `stmt` (including nested blocks).
pub(super) fn stmt_uses(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::VarDecl { value, .. } | Stmt::ConstDecl { value, .. } => expr_uses(value, name),
        Stmt::VaultDecl { name: vname, value, .. } => {
            // The name being declared is not a "use"; only its value expression matters.
            vname != name && expr_uses(value, name)
        }
        Stmt::Assign { value, .. } => expr_uses(value, name),
        Stmt::CompoundAssign { name: n, value, .. } => n == name || expr_uses(value, name),
        Stmt::IndexAssign { index, value, .. } => {
            expr_uses(index, name) || expr_uses(value, name)
        }
        Stmt::FieldAssign { value, .. } => expr_uses(value, name),
        Stmt::Print(args) => args.iter().any(|a| expr_uses(a, name)),
        Stmt::Return(Some(e)) => expr_uses(e, name),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue | Stmt::Exit => false,
        Stmt::Kill(Some(e)) => expr_uses(e, name),
        Stmt::Kill(None) => false,
        Stmt::Yield(e) => expr_uses(e, name),
        Stmt::Assert { cond, message } => {
            expr_uses(cond, name) || message.as_ref().map_or(false, |m| expr_uses(m, name))
        }
        Stmt::ExprStmt(e) => expr_uses(e, name),
        Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            expr_uses(cond, name)
                || stmts_use(then_body, name)
                || else_body.as_ref().map_or(false, |b| stmts_use(b, name))
        }
        Stmt::While { cond, body } => expr_uses(cond, name) || stmts_use(body, name),
        Stmt::Loop(body) => stmts_use(body, name),
        Stmt::For {
            from, to, body, ..
        } => expr_uses(from, name) || expr_uses(to, name) || stmts_use(body, name),
        Stmt::InlineAnchor { body, catch_body, .. } => {
            stmts_use(body, name)
                || catch_body
                    .as_ref()
                    .map_or(false, |b| stmts_use(b, name))
        }
        Stmt::Match { expr, arms } => {
            expr_uses(expr, name)
                || arms.iter().any(|arm| {
                    arm.guard.as_ref().map_or(false, |g| expr_uses(g, name))
                        || stmts_use(&arm.body, name)
                })
        }
    }
}

/// Returns true if `name` is used in any statement in `stmts`.
pub(super) fn stmts_use(stmts: &[(Stmt, Span)], name: &str) -> bool {
    stmts.iter().any(|(s, _)| stmt_uses(s, name))
}

/// Describes where to free Vault variables in a block.
pub(super) struct FreeSchedule {
    /// Free after stmts[i] in the current block.
    pub after: HashMap<usize, Vec<String>>,
    /// For if-stmt at index i: free at end of the then-branch.
    /// (Only populated when both branches exist.)
    pub in_then: HashMap<usize, Vec<String>>,
    /// For if-stmt at index i: free at end of the else-branch.
    /// (Only populated when both branches exist.)
    pub in_else: HashMap<usize, Vec<String>>,
}

impl FreeSchedule {
    pub fn new() -> Self {
        FreeSchedule {
            after: HashMap::new(),
            in_then: HashMap::new(),
            in_else: HashMap::new(),
        }
    }

    pub fn after_vars(&self, idx: usize) -> &[String] {
        self.after.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn then_vars(&self, idx: usize) -> &[String] {
        self.in_then.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn else_vars(&self, idx: usize) -> &[String] {
        self.in_else.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Compute the free schedule for `vault_vars` over `stmts`.
///
/// `vault_vars` are the Vault variables declared **in this block** (not outer scopes).
/// For each var, we find the earliest safe free point and record it in the schedule.
pub(super) fn compute_schedule(
    stmts: &[(Stmt, Span)],
    vault_vars: &[String],
) -> FreeSchedule {
    let mut sched = FreeSchedule::new();
    for var in vault_vars {
        schedule_one(stmts, var, &mut sched);
    }
    sched
}

fn schedule_one(stmts: &[(Stmt, Span)], var: &str, sched: &mut FreeSchedule) {
    // Backward scan: find the last statement that uses `var`.
    for (i, (stmt, _)) in stmts.iter().enumerate().rev() {
        if !stmt_uses(stmt, var) {
            continue;
        }
        // Found the last use at index i.
        // Determine the best free point.
        match stmt {
            Stmt::If {
                cond,
                then_body: _,
                else_body: Some(_),
            } => {
                // If used in cond, must free after the whole if/else.
                if expr_uses(cond, var) {
                    sched.after.entry(i).or_default().push(var.to_string());
                    return;
                }
                // Used in then or else (or both) — free at end of BOTH branches.
                // This guarantees both execution paths free the var exactly once.
                sched
                    .in_then
                    .entry(i)
                    .or_default()
                    .push(var.to_string());
                sched
                    .in_else
                    .entry(i)
                    .or_default()
                    .push(var.to_string());
            }
            // if without else, loop, while, for, match, InlineAnchor:
            // free after the entire stmt (conservative, always correct).
            _ => {
                sched.after.entry(i).or_default().push(var.to_string());
            }
        }
        return;
    }
    // Var is never used after declaration — free right after the declaration itself.
    for (i, (stmt, _)) in stmts.iter().enumerate() {
        if let Stmt::VaultDecl { name, .. } = stmt {
            if name.as_str() == var {
                sched.after.entry(i).or_default().push(var.to_string());
                return;
            }
        }
    }
}
