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

        // Prevent LLVM TargetMachine destructor crash on Windows (inkwell bug).
        // Target / TargetTriple do not implement Drop, so only machine needs forget.
        std::mem::forget(machine);
    }

    pub fn write_ir_file(&self, path: &str) {
        self.module
            .print_to_file(Path::new(path))
            .expect("Failed to write IR file");
    }

    // ── Kyte runtime declarations ─────────────────────────────────────────

    /// `void kyte_install_signals(void)` — install SIGFPE/SIGILL/SIGBUS handlers
    pub(super) fn declare_kyte_install_signals(&mut self) {
        if self.module.get_function("kyte_install_signals").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[], false);
        self.module.add_function(
            "kyte_install_signals",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `int kyte_anchor_enter(void)` — setjmp-based anchor entry.
    ///
    /// Returns 0 on first entry; signal number when recovered from a signal
    /// via longjmp inside the C signal handler.
    ///
    /// Note: we intentionally do NOT declare this as `returns_twice` at the
    /// LLVM IR level.  The `returns_twice` attribute makes LLVM treat every
    /// call site as a potential longjmp target, which prevents many
    /// optimizations and, on the Windows MSVC LLVM backend, causes an
    /// internal error when the call is inside a loop.  Signal-based recovery
    /// still works correctly because `kyte_anchor_enter` is an opaque
    /// external call — LLVM cannot see or optimize the longjmp path.
    pub(super) fn declare_kyte_anchor_enter(&mut self) {
        if self.module.get_function("kyte_anchor_enter").is_some() {
            return;
        }
        let fn_type = self.context.i32_type().fn_type(&[], false);
        self.module.add_function(
            "kyte_anchor_enter",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_anchor_exit(void)` — pop the sigsetjmp slot when done.
    pub(super) fn declare_kyte_anchor_exit(&mut self) {
        if self.module.get_function("kyte_anchor_exit").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[], false);
        self.module.add_function(
            "kyte_anchor_exit",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_log_kill(char*, char*)` — log a Kill event.
    pub(super) fn declare_kyte_log_kill(&mut self) {
        if self.module.get_function("kyte_log_kill").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.ptr_type().into(), self.ptr_type().into()], false);
        self.module.add_function(
            "kyte_log_kill",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_log_escalate(char*)` — log escalation to parent anchor.
    pub(super) fn declare_kyte_log_escalate(&mut self) {
        if self.module.get_function("kyte_log_escalate").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(&[self.ptr_type().into()], false);
        self.module.add_function(
            "kyte_log_escalate",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_log_restart(char*, int)` — log anchor restart.
    pub(super) fn declare_kyte_log_restart(&mut self) {
        if self.module.get_function("kyte_log_restart").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(
            &[self.ptr_type().into(), self.context.i32_type().into()],
            false,
        );
        self.module.add_function(
            "kyte_log_restart",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_register_event(char*, fn*)` — register an event handler.
    pub(super) fn declare_kyte_register_event(&mut self) {
        if self.module.get_function("kyte_register_event").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(
            &[self.ptr_type().into(), self.ptr_type().into()],
            false,
        );
        self.module.add_function(
            "kyte_register_event",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `void kyte_emit_event(char*, char*)` — fire all handlers matching an event name.
    pub(super) fn declare_kyte_emit_event(&mut self) {
        if self.module.get_function("kyte_emit_event").is_some() {
            return;
        }
        let void_type = self.context.void_type();
        let fn_type = void_type.fn_type(
            &[self.ptr_type().into(), self.ptr_type().into()],
            false,
        );
        self.module.add_function(
            "kyte_emit_event",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// `int kyte_spawn_thread_anchor(fn*, void*, int, char*)` — spawn a supervised thread.
    pub(super) fn declare_kyte_spawn_thread_anchor(&mut self) {
        if self
            .module
            .get_function("kyte_spawn_thread_anchor")
            .is_some()
        {
            return;
        }
        let i32_type = self.context.i32_type();
        let fn_type = i32_type.fn_type(
            &[
                self.ptr_type().into(), // function pointer
                self.ptr_type().into(), // arg
                i32_type.into(),        // max_restarts
                self.ptr_type().into(), // name
            ],
            false,
        );
        self.module.add_function(
            "kyte_spawn_thread_anchor",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );
    }

    /// Declare all Kyte runtime functions at once.
    pub(super) fn declare_kyte_runtime(&mut self) {
        self.declare_kyte_install_signals();
        self.declare_kyte_anchor_enter();
        self.declare_kyte_anchor_exit();
        self.declare_kyte_log_kill();
        self.declare_kyte_log_escalate();
        self.declare_kyte_log_restart();
        self.declare_kyte_spawn_thread_anchor();
        self.declare_kyte_register_event();
        self.declare_kyte_emit_event();
    }
}
