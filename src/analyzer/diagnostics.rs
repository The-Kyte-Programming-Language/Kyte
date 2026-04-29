use std::collections::HashMap;

use crate::analyzer::shared::{nearest_name, CompileError, Severity, VarInfo};
use crate::analyzer::Analyzer;

impl Analyzer {
    pub(super) fn err(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self
            .source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        self.errors.push(CompileError {
            code,
            severity: Severity::Error,
            message: msg,
            hint,
            span,
            source_line,
        });
    }

    pub(super) fn warn(&mut self, code: &'static str, msg: String, hint: String) {
        let span = self.current_span;
        let source_line = self
            .source_lines
            .get(span.line.saturating_sub(1))
            .cloned()
            .unwrap_or_default();
        // --Werror: 경고를 오류로 승격 (A05)
        let severity = if self.config.werror {
            Severity::Error
        } else {
            Severity::Warning
        };
        self.errors.push(CompileError {
            code,
            severity,
            message: msg,
            hint,
            span,
            source_line,
        });
    }

    pub(super) fn err_undeclared_var(&mut self, name: &str, scope: &HashMap<String, VarInfo>) {
        let hint = if let Some(similar) = nearest_name(name, scope.keys()) {
            format!("Did you mean '{}' ?", similar)
        } else {
            format!("Declare '{}' before use, e.g. int {} = ...;", name, name)
        };
        self.err("E004", format!("Undeclared variable '{}'", name), hint);
    }

    pub(super) fn err_undeclared_fn(&mut self, name: &str) {
        let hint = if let Some(similar) = nearest_name(name, self.functions.keys()) {
            format!("Did you mean '{}' ?", similar)
        } else {
            format!("Define '{}' with fn {}(...) {{ ... }}", name, name)
        };
        self.err("E013", format!("Undeclared function '{}'", name), hint);
    }
}
