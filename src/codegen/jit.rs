use inkwell::execution_engine::ExecutionEngine;

use super::Codegen;
use crate::ast::Ty;

impl<'ctx> Codegen<'ctx> {
    pub fn get_ir_string(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn print_ir(&self) {
        println!("{}", self.module.print_to_string());
    }

    pub fn run_i64_function(&self, name: &str) -> Result<i64, String> {
        let func = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("function '{}' not found", name))?;

        let engine: ExecutionEngine<'ctx> = self
            .module
            .create_jit_execution_engine(self.opt_level)
            .map_err(|e| format!("failed to create JIT engine: {}", e))?;

        let ret = unsafe { engine.run_function(func, &[]) };
        let value = ret.as_int(false) as i64;
        std::mem::forget(engine);
        Ok(value)
    }

    pub fn run_f64_function(&self, name: &str) -> Result<f64, String> {
        let func = self
            .module
            .get_function(name)
            .ok_or_else(|| format!("function '{}' not found", name))?;

        let engine: ExecutionEngine<'ctx> = self
            .module
            .create_jit_execution_engine(self.opt_level)
            .map_err(|e| format!("failed to create JIT engine: {}", e))?;

        let ret = unsafe { engine.run_function(func, &[]) };
        let value = ret.as_float(&self.f64_type());
        std::mem::forget(engine);
        Ok(value)
    }

    pub fn get_function_return_ty(&self, name: &str) -> Option<Ty> {
        self.fn_return_tys.get(name).and_then(|opt| opt.clone())
    }
}
