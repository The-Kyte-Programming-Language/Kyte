fn main() {
    // inkwell 0.8.0 compiles initialization functions for ALL LLVM targets,
    // but our LLVM installation may only have a subset of targets.
    // Generate C stubs for missing target initialization symbols.
    let missing_targets = [
        "AMDGPU",
        "Hexagon",
        "Lanai",
        "LoongArch",
        "Mips",
        "MSP430",
        "PowerPC",
        "Sparc",
        "SystemZ",
        "XCore",
        "AVR",
        "VE",
    ];
    let suffixes = [
        "Target",
        "TargetInfo",
        "AsmPrinter",
        "AsmParser",
        "Disassembler",
        "TargetMC",
    ];

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let stub_path = format!("{}/llvm_target_stubs.c", out_dir);

    let mut code = String::new();
    for target in &missing_targets {
        for suffix in &suffixes {
            code.push_str(&format!(
                "void LLVMInitialize{}{}(void) {{}}\n",
                target, suffix
            ));
        }
    }

    std::fs::write(&stub_path, &code).unwrap();
    cc::Build::new()
        .file(&stub_path)
        .compile("llvm_target_stubs");
}
