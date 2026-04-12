# Kyte

**Kyte** is a compiled programming language powered by LLVM. It uses an **anchor-based** execution model where programs are organized around named execution contexts called *anchors*.

## Features

- **Anchor-based execution** — `@name(kind)` blocks define scoped execution contexts (`main`, `thread`, `event`, …)
- **Static type system** — `int`, `float`, `string`, `bool` with compile-time checking
- **LLVM backend** — compiles to native machine code via LLVM 21
- **First-class functions** — `fn` declarations with typed parameters and return types
- **Control flow** — `if` / `else`, `for i in 0..n`, `loop`, `break`
- **Managed memory** — `vault` declarations with explicit `free()`
- **Semantic analysis** — rich error diagnostics with codes (E001–E013), hints, and ANSI-colored output
- **LSP server** — real-time diagnostics, hover, and completion in VS Code

## Quick Start

### Prerequisites

| Tool | Version |
|------|---------|
| Rust | 1.75+ |
| LLVM | 21 |
| Clang | (for linking) |

### Build

```bash
cargo build --release
```

### Compile a program

```bash
# Compile hello.ky → hello.o
cargo run --release -- examples/hello.ky

# Link with clang
clang examples/hello.o -o examples/hello.exe
./examples/hello
```

### Run tests

```bash
cargo run -- test
```

## Language Guide

### Hello World

```kyte
@app(main)
    yield "Hello, World!";
```

### Functions

```kyte
fn add(int a, int b) -> int {
    return a + b;
}

fn factorial(int n) -> int {
    if n <= 1 {
        return 1;
    }
    return n * factorial(n - 1);
}
```

### Anchors

Anchors are named execution contexts. The `main` anchor is the entry point.

```kyte
@app(main)
    int x = 10;
    int y = 20;
    yield add(x, y);

    @worker(thread)
        yield "running in a thread context";
```

Anchor kinds: `main`, `thread`, `event`, `plain`, `onerror`, `timeout`

### Types

| Type | Description | Example |
|------|-------------|---------|
| `int` | 64-bit signed integer | `42` |
| `float` | 64-bit float | `3.14` |
| `string` | UTF-8 string | `"hello"` |
| `bool` | Boolean | `true` / `false` |

### Control Flow

```kyte
// if / else
if x > 10 {
    yield "big";
} else {
    yield "small";
}

// for loop (range)
for i in 0..5 {
    yield i;
}

// infinite loop
loop {
    if done { break; }
}
```

### Vault (Managed Memory)

```kyte
vault int buffer = 1024;
// ... use buffer ...
free(buffer);
```

### Operators

| Category | Operators |
|----------|-----------|
| Arithmetic | `+` `-` `*` `/` `%` |
| Comparison | `==` `!=` `<` `>` `<=` `>=` |
| Logical | `&&` `\|\|` `!` |
| Assignment | `=` `+=` `-=` `*=` `/=` `%=` |

### Special Statements

| Statement | Description |
|-----------|-------------|
| `yield expr` | Print / output a value |
| `exit` | Terminate the program |
| `kill` | Terminate the current anchor |
| `return expr` | Return from a function |

## Editor Support

### VS Code

1. Build the compiler: `cargo build --release`
2. Make sure `kyte` is in your `PATH`
3. Install the extension:
   ```bash
   cd editors/vscode
   npm install
   ```
4. Open VS Code, run **Developer: Install Extension from Location…** and select `editors/vscode/`

The extension provides:
- Syntax highlighting for `.ky` files
- Real-time error diagnostics (via LSP)
- Hover information for keywords and functions
- Auto-completion for keywords and declared functions

### LSP Server

The LSP server runs over stdio:

```bash
kyte lsp
```

Any editor that supports the Language Server Protocol can use it.

## Project Structure

```
kyte/
├── src/
│   ├── ast.rs        # Token, expression, statement, and program types
│   ├── lexer.rs      # Tokenizer
│   ├── parser.rs     # Recursive-descent parser
│   ├── analyzer.rs   # Semantic analysis and type checking
│   ├── codegen.rs    # LLVM IR code generation (inkwell)
│   ├── lsp.rs        # Language Server Protocol implementation
│   ├── lib.rs        # Library crate root
│   └── main.rs       # CLI entry point
├── editors/
│   └── vscode/       # VS Code extension
├── examples/
│   └── hello.ky      # Example program
├── build.rs          # LLVM target stub generation
├── Cargo.toml
└── LICENSE           # Apache 2.0
```

## License

Licensed under the [Apache License 2.0](LICENSE).
