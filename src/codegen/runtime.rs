use std::path::Path;

use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};

use super::Codegen;

impl<'ctx> Codegen<'ctx> {
    pub(super) fn declare_printf(&mut self) {
        if self.module.get_function("printf").is_some() {
            return;
        }
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(&[self.ptr_type().into()], true);
        self.module
            .add_function("printf", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub(super) fn declare_exit_fn(&mut self) {
        if self.module.get_function("exit").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.context.i32_type().into()], false);
        self.module
            .add_function("exit", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub(super) fn declare_snprintf(&mut self) {
        if self.module.get_function("snprintf").is_some() {
            return;
        }
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(
            &[
                self.ptr_type().into(),
                self.i64_type().into(),
                self.ptr_type().into(),
            ],
            true,
        );
        self.module.add_function(
            "snprintf",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    pub(super) fn declare_strlen(&mut self) {
        if self.module.get_function("strlen").is_some() {
            return;
        }
        let fn_type = self.i64_type().fn_type(&[self.ptr_type().into()], false);
        self.module
            .add_function("strlen", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub(super) fn declare_malloc(&mut self) {
        if self.module.get_function("malloc").is_some() {
            return;
        }
        let fn_type = self.ptr_type().fn_type(&[self.i64_type().into()], false);
        self.module
            .add_function("malloc", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub(super) fn declare_free_fn(&mut self) {
        if self.module.get_function("free").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.ptr_type().into()], false);
        self.module
            .add_function("free", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub(super) fn declare_strcmp(&mut self) {
        if self.module.get_function("strcmp").is_some() {
            return;
        }
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(&[self.ptr_type().into(), self.ptr_type().into()], false);
        self.module
            .add_function("strcmp", fn_type, Some(inkwell::module::Linkage::External));
    }

    pub fn write_object_file(&self, path: &str) {
        Target::initialize_native(&InitializationConfig::default())
            .expect("Failed to initialize native target");

        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).expect("Failed to get target");
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                self.opt_level,
                RelocMode::Default,
                CodeModel::Default,
            )
            .expect("Failed to create target machine");

        machine
            .write_to_file(&self.module, FileType::Object, Path::new(path))
            .expect("Failed to write object file");
        std::mem::forget(machine);
    }

    pub fn write_ir_file(&self, path: &str) {
        self.module
            .print_to_file(Path::new(path))
            .expect("Failed to write IR file");
    }
}
