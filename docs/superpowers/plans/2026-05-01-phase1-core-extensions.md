# Phase 1 — Core Language Extensions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `extern fn`, grouped import syntax, and generic monomorphization so Kyte can compile its own standard library.

**Architecture:** Three independent features built sequentially. `extern fn` and grouped imports are additive (new token/parse/codegen paths, nothing removed). Generic monomorphization is an on-demand specialization pass inside codegen — when a call targets a generic function or generic struct is instantiated, a concrete copy is emitted under a mangled name. No AST changes to call sites; types are inferred from argument LLVM values.

**Tech Stack:** Rust, Inkwell (LLVM 21 bindings), existing Kyte lexer/parser/codegen

---

## File Map

| File | Change |
|------|--------|
| `src/lexer.rs` | Add `Token::Extern` keyword |
| `src/ast.rs` | Add `TopLevel::ExternFn { name, params, return_ty }` |
| `src/parser/items.rs` | Add `parse_extern_fn()` |
| `src/parser/toplevel.rs` | Dispatch `Token::Extern` to `parse_extern_fn()` |
| `src/codegen/program.rs` | Emit extern declarations; add monomorphization registry |
| `src/codegen/mono.rs` | **New** — type substitution + specialization engine |
| `src/codegen/types.rs` | Remove i64 fallback for `TypeParam`, route through mono |
| `src/codegen/exprs.rs` | Detect generic calls → trigger specialization |
| `src/main/imports.rs` | Handle `{A, B, C}` grouped import syntax |
| `tests/extern_fn.ky` | Integration test for extern |
| `tests/generics.ky` | Integration test for monomorphization |

---

## Task 1: `extern fn` — Lexer Token

**Files:**
- Modify: `src/lexer.rs`

- [ ] **Step 1: Add failing test**

Add to `src/lexer.rs` at the bottom (inside `#[cfg(test)]` block, or create one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extern_keyword_tokenizes() {
        let tokens = tokenize("extern fn malloc(size: u64);");
        assert!(tokens.contains(&Token::Extern), "expected Token::Extern");
    }
}
```

- [ ] **Step 2: Run test — expect FAIL**

```powershell
cargo test extern_keyword_tokenizes
```

Expected: compile error — `Token::Extern` doesn't exist.

- [ ] **Step 3: Add `Token::Extern` to enum**

In `src/lexer.rs`, find the keyword block of the `Token` enum (near `Import`, `Catch`) and add:

```rust
Extern,
```

Then in the keyword match (find the `match s.as_str()` block, near `"import" => Token::Import`):

```rust
"extern" => Token::Extern,
```

- [ ] **Step 4: Run test — expect PASS**

```powershell
cargo test extern_keyword_tokenizes
```

- [ ] **Step 5: Commit**

```powershell
git add src/lexer.rs
git commit -m "feat(lexer): add Token::Extern keyword"
```

---

## Task 2: `extern fn` — AST Node

**Files:**
- Modify: `src/ast.rs`

- [ ] **Step 1: Add `ExternFn` to `TopLevel`**

In `src/ast.rs`, find the `TopLevel` enum. Add after the last variant:

```rust
ExternFn {
    name: String,
    params: Vec<Param>,
    return_ty: Option<Ty>,
},
```

- [ ] **Step 2: Verify compile**

```powershell
cargo build 2>&1 | head -20
```

Expected: compile warnings about non-exhaustive match in analyzer/codegen — that's fine for now.

- [ ] **Step 3: Commit**

```powershell
git add src/ast.rs
git commit -m "feat(ast): add TopLevel::ExternFn node"
```

---

## Task 3: `extern fn` — Parser

**Files:**
- Modify: `src/parser/items.rs`
- Modify: `src/parser/toplevel.rs` (or wherever `Token::Import` dispatch lives)

- [ ] **Step 1: Add `parse_extern_fn()` to `src/parser/items.rs`**

Add this function to the `impl Parser` block:

```rust
pub(super) fn parse_extern_fn(&mut self) -> TopLevel {
    self.expect(&Token::Extern);
    self.expect(&Token::Function); // `fn`
    let name = self.eat_ident();
    self.expect(&Token::LParen);
    let mut params = Vec::new();
    while self.current() != &Token::RParen && self.current() != &Token::EOF {
        let ty = self.parse_ty();
        let pname = self.eat_var_ident();
        params.push(Param { ty, name: pname });
        if self.current() == &Token::Comma {
            self.advance();
        }
    }
    self.expect(&Token::RParen);
    let return_ty = if self.current() == &Token::Arrow {
        self.advance();
        Some(self.parse_ty())
    } else {
        None
    };
    self.expect(&Token::Semicolon);
    TopLevel::ExternFn { name, params, return_ty }
}
```

- [ ] **Step 2: Dispatch in top-level parser**

Find where `Token::Import` is dispatched (in `src/parser/toplevel.rs` or `src/parser.rs`). In the same match arm block, add:

```rust
Token::Extern => {
    let node = self.parse_extern_fn();
    items.push((node, span));
}
```

- [ ] **Step 3: Write integration test file**

Create `tests/extern_fn.ky`:

```kyte
extern fn add_c(a: int, b: int) -> int;

@main(main) {
    // just verify it compiles — linking will fail at runtime without a real impl
    // but parse + codegen must succeed
    print("extern fn parsed ok");
}
```

- [ ] **Step 4: Run parser test**

```powershell
cargo run -- tests/extern_fn.ky 2>&1 | head -10
```

Expected: either compiles cleanly (with linker error about missing `add_c`) or a clear "no definition" linker error. NOT a parse or codegen panic.

- [ ] **Step 5: Commit**

```powershell
git add src/parser/items.rs src/parser/toplevel.rs tests/extern_fn.ky
git commit -m "feat(parser): parse extern fn declarations"
```

---

## Task 4: `extern fn` — Codegen

**Files:**
- Modify: `src/codegen/program.rs`

- [ ] **Step 1: Handle `TopLevel::ExternFn` in function prototype loop**

In `src/codegen/program.rs`, find the loop that emits function prototypes (around line 130, the `for (item, _) in &program.items` block). In the `match item` inside, add an arm:

```rust
TopLevel::ExternFn { name, params, return_ty } => {
    let param_types: Vec<BasicMetadataTypeEnum> = params
        .iter()
        .map(|p| self.ty_to_llvm(&p.ty))
        .collect();
    let fn_type = match return_ty {
        Some(ty) => {
            let ret = self.ty_to_basic(ty);
            ret.fn_type(&param_types, false)
        }
        None => self.context.void_type().fn_type(&param_types, false),
    };
    use inkwell::module::Linkage;
    let func = self.module.add_function(name, fn_type, Some(Linkage::External));
    self.functions.insert(name.clone(), func);
    self.fn_return_tys.insert(name.clone(), return_ty.clone());
}
```

- [ ] **Step 2: Skip body emission for ExternFn**

Find the second loop that emits function bodies (the loop after the prototype loop, also in `program.rs`). In its `match item` block, add:

```rust
TopLevel::ExternFn { .. } => { /* no body */ }
```

- [ ] **Step 3: Write a real linkage test**

Create `tests/extern_link.ky` that calls `strlen` (which exists in libc, already linked):

```kyte
extern fn strlen(s: string) -> int;

@main(main) {
    int n = strlen("hello");
    print(n);
}
```

- [ ] **Step 4: Compile and run**

```powershell
cargo run -- tests/extern_link.ky
```

Expected output: `5`

- [ ] **Step 5: Commit**

```powershell
git add src/codegen/program.rs tests/extern_link.ky
git commit -m "feat(codegen): emit extern fn as LLVM external declaration"
```

---

## Task 5: Grouped Import Syntax

**Files:**
- Modify: `src/main/imports.rs`

- [ ] **Step 1: Write unit test for group expansion**

Add to `src/main/imports.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_import_expands() {
        let line = "import std.collections.{Vec, Map, Pool};";
        let expanded = expand_import_line(line);
        assert_eq!(expanded, vec![
            "import std.collections.Vec;",
            "import std.collections.Map;",
            "import std.collections.Pool;",
        ]);
    }

    #[test]
    fn single_import_unchanged() {
        let line = "import std.io;";
        let expanded = expand_import_line(line);
        assert_eq!(expanded, vec!["import std.io;"]);
    }

    #[test]
    fn non_import_unchanged() {
        let line = "int x = 5;";
        let expanded = expand_import_line(line);
        assert_eq!(expanded, vec!["int x = 5;"]);
    }
}
```

- [ ] **Step 2: Run test — expect FAIL**

```powershell
cargo test grouped_import_expands
```

Expected: compile error — `expand_import_line` not defined.

- [ ] **Step 3: Implement `expand_import_line`**

Add to `src/main/imports.rs` before `parse_import_path`:

```rust
/// Expands `import a.b.{X, Y, Z};` into `["import a.b.X;", "import a.b.Y;", "import a.b.Z;"]`.
/// Passes non-grouped and non-import lines through unchanged as a single-element vec.
pub(super) fn expand_import_line(line: &str) -> Vec<String> {
    let t = line.trim();
    if !t.starts_with("import") {
        return vec![line.to_string()];
    }
    let rest = t["import".len()..].trim_start();
    // Check for `{...}` group
    if let Some(brace_start) = rest.find('{') {
        let prefix = rest[..brace_start].trim_end_matches(|c| c == '.' || c == ' ');
        let inner = rest[brace_start + 1..]
            .trim_end_matches(';')
            .trim_end_matches('}');
        return inner
            .split(',')
            .map(|item| format!("import {}.{};", prefix, item.trim()))
            .collect();
    }
    vec![line.to_string()]
}
```

- [ ] **Step 4: Run tests — expect PASS**

```powershell
cargo test grouped_import
cargo test single_import
cargo test non_import
```

- [ ] **Step 5: Wire into `load_source_with_imports`**

In `src/main/imports.rs`, find the inner `visit` function. Find where it iterates over lines:

```rust
for line in text.lines() {
    if let Some(rel) = parse_import_path(line) {
```

Replace this loop with:

```rust
for raw_line in text.lines() {
    for line in expand_import_line(raw_line) {
        if let Some(rel) = parse_import_path(&line) {
            // existing import resolution logic — use `line` instead of `raw_line`
            let dep = base_dir.join(&rel);
            visit(&dep, seen, out)?;
        }
    }
}
```

Do the same for the output loop (the second loop that writes non-import lines):

```rust
for raw_line in text.lines() {
    for line in expand_import_line(raw_line) {
        if parse_import_path(&line).is_none() {
            out.push_str(&line);
            out.push('\n');
        }
    }
}
```

- [ ] **Step 6: Integration test**

Create `tests/imports/vec_stub.ky`:

```kyte
fn vec_stub_works() -> int {
    return 1;
}
```

Create `tests/imports/map_stub.ky`:

```kyte
fn map_stub_works() -> int {
    return 2;
}
```

Create `tests/imports/grouped.ky`:

```kyte
import tests/imports/vec_stub.ky;
import tests/imports/map_stub.ky;

@main(main) {
    int a = vec_stub_works();
    int b = map_stub_works();
    print(a + b);
}
```

Verify individual imports work before grouped (they already do). Then create `tests/imports/grouped_brace.ky` to test brace expansion once std/ is present. For now:

```powershell
cargo run -- tests/imports/grouped.ky
```

Expected output: `3`

- [ ] **Step 7: Commit**

```powershell
git add src/main/imports.rs tests/imports/
git commit -m "feat(imports): support grouped import {A, B, C} syntax"
```

---

## Task 6: std/ Directory Resolution

**Files:**
- Modify: `src/main/imports.rs`

Import paths like `std.io` or `std.collections.Vec` need to resolve to files inside a `std/` directory bundled with the compiler binary.

- [ ] **Step 1: Write test**

```rust
#[test]
fn std_path_resolves() {
    // "std.io" should map to "<kyte_home>/std/io.ky"
    let result = resolve_std_path("std.io");
    assert!(result.is_some());
    let path = result.unwrap();
    assert!(path.ends_with("std/io.ky") || path.ends_with("std\\io.ky"));
}

#[test]
fn non_std_path_returns_none() {
    let result = resolve_std_path("mylib.foo");
    assert!(result.is_none());
}
```

- [ ] **Step 2: Run test — expect FAIL**

```powershell
cargo test std_path_resolves
```

- [ ] **Step 3: Implement `resolve_std_path`**

The `std/` directory lives at `<binary_dir>/../std/` (relative to the kyte executable) or `<workspace_root>/std/` for dev builds. Add to `src/main/imports.rs`:

```rust
/// Returns path to a std module file, or None if the import is not a std:: path.
pub(super) fn resolve_std_path(import_path: &str) -> Option<String> {
    if !import_path.starts_with("std.") {
        return None;
    }
    // Convert "std.collections.Vec" → "std/collections/Vec.ky"
    let rel: String = import_path
        .replace('.', std::path::MAIN_SEPARATOR_STR)
        + ".ky";

    // Try: binary directory sibling of "std/"
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe
            .parent()?
            .parent()
            .unwrap_or_else(|| exe.parent().unwrap())
            .join(&rel);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    // Fallback: current working directory / rel
    let cwd_candidate = std::env::current_dir().ok()?.join(&rel);
    if cwd_candidate.exists() {
        return Some(cwd_candidate.to_string_lossy().into_owned());
    }

    None
}
```

- [ ] **Step 4: Wire into `parse_import_path` dispatch**

In the `visit` function in `load_source_with_imports`, after getting `rel` from `parse_import_path`, add std resolution before the local file join:

```rust
if let Some(rel) = parse_import_path(&line) {
    // Try std/ resolution first
    let dep = if let Some(std_path) = resolve_std_path(&rel) {
        PathBuf::from(std_path)
    } else {
        base_dir.join(&rel)
    };
    visit(&dep, seen, out)?;
}
```

- [ ] **Step 5: Create placeholder std structure**

```powershell
New-Item -ItemType Directory -Force std/collections
New-Item -ItemType Directory -Force std/internal
```

Create `std/io.ky` placeholder:

```kyte
// std.io — placeholder until Phase 3
fn print_placeholder() -> int { return 0; }
```

- [ ] **Step 6: Verify resolution compiles**

```powershell
cargo test std_path_resolves
```

- [ ] **Step 7: Commit**

```powershell
git add src/main/imports.rs std/
git commit -m "feat(imports): resolve std.* paths from std/ directory"
```

---

## Task 7: Generic Monomorphization — Infrastructure

**Files:**
- Create: `src/codegen/mono.rs`
- Modify: `src/codegen/program.rs`

This task adds the monomorphization engine. Generic functions and structs are collected before codegen starts. Concrete specializations are emitted on demand.

- [ ] **Step 1: Create `src/codegen/mono.rs`**

```rust
use std::collections::HashMap;
use crate::ast::{TopLevel, Ty, Param, Stmt, Span};

/// Key for a generic specialization: (function_name, concrete_type_arguments)
#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct MonoKey {
    pub name: String,
    pub type_args: Vec<String>, // mangled type names
}

impl MonoKey {
    pub fn new(name: &str, concrete: &[Ty]) -> Self {
        MonoKey {
            name: name.to_string(),
            type_args: concrete.iter().map(mangle_ty).collect(),
        }
    }

    /// Returns the LLVM function name for this specialization.
    pub fn mangled_fn_name(&self) -> String {
        if self.type_args.is_empty() {
            self.name.clone()
        } else {
            format!("{}__{}", self.name, self.type_args.join("_"))
        }
    }
}

pub fn mangle_ty(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::String => "str".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::I8 => "i8".to_string(),
        Ty::I16 => "i16".to_string(),
        Ty::I32 => "i32".to_string(),
        Ty::I64 => "i64".to_string(),
        Ty::U8 => "u8".to_string(),
        Ty::U16 => "u16".to_string(),
        Ty::U32 => "u32".to_string(),
        Ty::U64 => "u64".to_string(),
        Ty::Struct(n) | Ty::Enum(n) => n.clone(),
        Ty::Array(inner) => format!("arr_{}", mangle_ty(inner)),
        Ty::TypeParam(p) => format!("T_{}", p),
        Ty::Auto | Ty::Fn(_, _) => "opaque".to_string(),
    }
}

/// Substitutes all TypeParam occurrences in a Ty using the provided map.
pub fn substitute_ty(ty: &Ty, subst: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::TypeParam(name) => subst.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Ty::Array(inner) => Ty::Array(Box::new(substitute_ty(inner, subst))),
        Ty::Fn(params, ret) => Ty::Fn(
            params.iter().map(|p| substitute_ty(p, subst)).collect(),
            ret.as_ref().map(|r| Box::new(substitute_ty(r, subst))),
        ),
        other => other.clone(),
    }
}

/// Substitutes TypeParams in a Param list.
pub fn substitute_params(params: &[Param], subst: &HashMap<String, Ty>) -> Vec<Param> {
    params.iter().map(|p| Param {
        ty: substitute_ty(&p.ty, subst),
        name: p.name.clone(),
    }).collect()
}

/// Produces a concrete TopLevel::Function from a generic definition + type substitution.
/// Returns None if `def` is not a generic Function.
pub fn specialize_function(
    def: &TopLevel,
    concrete: &[Ty],
    mangled_name: &str,
) -> Option<TopLevel> {
    if let TopLevel::Function {
        name: _,
        type_params,
        params,
        return_ty,
        body,
        decorators,
    } = def
    {
        if type_params.is_empty() {
            return None;
        }
        let subst: HashMap<String, Ty> = type_params
            .iter()
            .zip(concrete.iter())
            .map(|(tp, ty)| (tp.clone(), ty.clone()))
            .collect();

        let new_params = substitute_params(params, &subst);
        let new_return_ty = return_ty.as_ref().map(|r| substitute_ty(r, &subst));
        // Body statement substitution happens during codegen via the type context —
        // we only need to substitute types in signatures here.
        // Variable type annotations inside the body are handled by the type inference pass.
        Some(TopLevel::Function {
            name: mangled_name.to_string(),
            type_params: vec![],
            params: new_params,
            return_ty: new_return_ty,
            body: body.clone(),
            decorators: decorators.clone(),
        })
    } else {
        None
    }
}
```

- [ ] **Step 2: Add mono module to `src/codegen/mod.rs` or `src/codegen.rs`**

Find the `mod.rs` inside `src/codegen/` (or the pub mod declarations in `src/codegen.rs`). Add:

```rust
pub(super) mod mono;
```

- [ ] **Step 3: Add monomorphization registry to `Codegen` struct**

In `src/codegen/program.rs` (or wherever `struct Codegen` is defined — check `src/codegen.rs` or `src/lib.rs`), add two fields:

```rust
/// Generic function definitions keyed by name
pub(crate) generic_defs: HashMap<String, TopLevel>,
/// Already-emitted specializations (MonoKey → already registered in self.functions)
pub(crate) emitted_specializations: HashSet<mono::MonoKey>,
```

Initialize both to `HashMap::new()` / `HashSet::new()` in the constructor.

- [ ] **Step 4: Collect generic defs before prototype emission**

In `src/codegen/program.rs`, at the very start of the function that compiles the program (before the prototype loop), add:

```rust
// Collect generic definitions before any emission
for (item, _) in &program.items {
    if let TopLevel::Function { name, type_params, .. } = item {
        if !type_params.is_empty() {
            self.generic_defs.insert(name.clone(), item.clone());
        }
    }
}
```

- [ ] **Step 5: Verify compilation**

```powershell
cargo build 2>&1 | head -30
```

Expected: no errors (may have warnings about unused fields — fine).

- [ ] **Step 6: Commit**

```powershell
git add src/codegen/mono.rs src/codegen/
git commit -m "feat(codegen): add monomorphization infrastructure (MonoKey, substitute, specialize)"
```

---

## Task 8: Generic Monomorphization — Call Site Specialization

**Files:**
- Modify: `src/codegen/exprs.rs`
- Modify: `src/codegen/program.rs`

- [ ] **Step 1: Write integration test first**

Create `tests/generics.ky`:

```kyte
fn identity<T>(val: T) -> T {
    return val;
}

fn add<T>(a: T, b: T) -> T {
    return a + b;
}

@main(main) {
    int x = identity(42);
    string s = identity("hello");
    int sum = add(10, 20);
    print(x);
    print(s);
    print(sum);
}
```

- [ ] **Step 2: Run test — expect FAIL (wrong output or panic)**

```powershell
cargo run -- tests/generics.ky
```

Expected: either wrong output (because T falls back to i64) or a panic. Note the actual failure.

- [ ] **Step 3: Add `emit_specialization` to Codegen**

In `src/codegen/program.rs`, add this method to `impl Codegen`:

```rust
/// Emits a concrete specialization of a generic function if not already emitted.
/// Returns the mangled LLVM function name.
pub(crate) fn emit_specialization(
    &mut self,
    generic_name: &str,
    concrete_types: &[Ty],
    program: &crate::ast::Program,
) -> Option<String> {
    let key = mono::MonoKey::new(generic_name, concrete_types);
    let mangled = key.mangled_fn_name();

    if self.emitted_specializations.contains(&key) {
        return Some(mangled);
    }

    let generic_def = self.generic_defs.get(generic_name)?.clone();
    let specialized = mono::specialize_function(&generic_def, concrete_types, &mangled)?;

    // Register prototype
    if let TopLevel::Function { params, return_ty, .. } = &specialized {
        let param_types: Vec<_> = params.iter().map(|p| self.ty_to_llvm(&p.ty)).collect();
        let fn_type = match return_ty {
            Some(ty) => {
                let ret = self.ty_to_basic(ty);
                ret.fn_type(&param_types, false)
            }
            None => self.context.void_type().fn_type(&param_types, false),
        };
        let func = self.module.add_function(&mangled, fn_type, None);
        self.functions.insert(mangled.clone(), func);
        self.fn_return_tys.insert(mangled.clone(), return_ty.clone());
    }

    self.emitted_specializations.insert(key);

    // Emit body
    self.compile_function(&specialized, program);

    Some(mangled)
}
```

- [ ] **Step 4: Infer concrete types from args in `compile_expr`**

In `src/codegen/exprs.rs`, find the `Expr::Call { name, args, .. }` match arm. Add generic detection before the regular call path:

```rust
Expr::Call { name, args, span } => {
    // Compile arguments first to get concrete types
    let compiled_args: Vec<BasicValueEnum> = args
        .iter()
        .map(|a| self.compile_expr(a, params))
        .collect();

    // Check if this is a generic function call
    if self.generic_defs.contains_key(name.as_str()) {
        let concrete_types: Vec<Ty> = compiled_args
            .iter()
            .map(|v| self.llvm_value_to_kyte_ty(*v))
            .collect();

        if let Some(mangled) = self.emit_specialization(name, &concrete_types, /* program */ ) {
            let func = self.functions[&mangled];
            let call_args: Vec<BasicMetadataValueEnum> =
                compiled_args.iter().map(|v| (*v).into()).collect();
            return self.builder.build_call(func, &call_args, "gcall")
                .try_as_basic_value()
                .left()
                .unwrap_or_else(|| self.i64_type().const_int(0, false).into());
        }
    }

    // ... existing non-generic call path below (unchanged)
```

**Note:** `emit_specialization` needs a reference to `program`. Pass `program` down through `compile_expr` or store it on `self` temporarily before emitting function bodies.

- [ ] **Step 5: Add `llvm_value_to_kyte_ty` helper**

In `src/codegen/types.rs`, add:

```rust
/// Best-effort reverse mapping from an LLVM value's type to a Kyte Ty.
pub(crate) fn llvm_value_to_kyte_ty(&self, val: BasicValueEnum<'ctx>) -> Ty {
    use inkwell::values::BasicValueEnum::*;
    match val {
        IntValue(v) => match v.get_type().get_bit_width() {
            1 => Ty::Bool,
            8 => Ty::I8,
            16 => Ty::I16,
            32 => Ty::I32,
            64 => Ty::Int, // default i64
            _ => Ty::Int,
        },
        FloatValue(_) => Ty::Float,
        PointerValue(_) => Ty::String, // strings are i8* in LLVM
        StructValue(_) => Ty::Struct("__unknown".to_string()),
        ArrayValue(_) => Ty::Array(Box::new(Ty::Int)),
        VectorValue(_) => Ty::Int,
    }
}
```

- [ ] **Step 6: Run integration test**

```powershell
cargo run -- tests/generics.ky
```

Expected output:
```
42
hello
30
```

- [ ] **Step 7: Commit**

```powershell
git add src/codegen/exprs.rs src/codegen/program.rs src/codegen/types.rs tests/generics.ky
git commit -m "feat(codegen): generic function monomorphization via on-demand specialization"
```

---

## Task 9: Generic Struct Monomorphization

**Files:**
- Modify: `src/ast.rs`
- Modify: `src/codegen/mono.rs`
- Modify: `src/codegen/program.rs`

Generic structs like `Vec<int>` need specialized LLVM struct types per instantiation.

- [ ] **Step 1: Add `TopLevel::Struct` type_params if not present**

Check `src/ast.rs` — if `TopLevel::Struct` doesn't have `type_params: Vec<String>`, add it:

```rust
Struct {
    name: String,
    type_params: Vec<String>,  // add this if missing
    fields: Vec<(String, Ty)>,
},
```

- [ ] **Step 2: Add struct specialization to `mono.rs`**

```rust
/// Produces a concrete struct definition from a generic struct + type substitution.
pub fn specialize_struct(
    def: &TopLevel,
    concrete: &[Ty],
    mangled_name: &str,
) -> Option<TopLevel> {
    if let TopLevel::Struct { type_params, fields, .. } = def {
        if type_params.is_empty() { return None; }
        let subst: HashMap<String, Ty> = type_params.iter()
            .zip(concrete.iter())
            .map(|(tp, ty)| (tp.clone(), ty.clone()))
            .collect();
        let new_fields = fields.iter()
            .map(|(name, ty)| (name.clone(), substitute_ty(ty, &subst)))
            .collect();
        Some(TopLevel::Struct {
            name: mangled_name.to_string(),
            type_params: vec![],
            fields: new_fields,
        })
    } else {
        None
    }
}
```

- [ ] **Step 3: Collect generic struct defs**

In `src/codegen/program.rs`, in the initial collection loop, add:

```rust
if let TopLevel::Struct { name, type_params, .. } = item {
    if !type_params.is_empty() {
        self.generic_struct_defs.insert(name.clone(), item.clone());
    }
}
```

Add `generic_struct_defs: HashMap<String, TopLevel>` to `Codegen`.

- [ ] **Step 4: Resolve generic struct types in `ty_to_llvm`**

In `src/codegen/types.rs`, find the `Ty::Struct(name)` arm. When the name contains `__` (mangled), look up the specialized struct:

```rust
Ty::Struct(name) => {
    if let Some(cached) = self.struct_types.get(name) {
        return (*cached).into();
    }
    // fall through to build the struct type
```

The existing struct-building logic already handles this if the specialized struct has been registered.

- [ ] **Step 5: Write integration test**

Create `tests/generic_struct.ky`:

```kyte
struct Pair<T> {
    T first;
    T second;
}

fn make_pair<T>(a: T, b: T) -> Pair<T> {
    return Pair { first: a, second: b };
}

@main(main) {
    Pair<int> p = make_pair(1, 2);
    print(p.first);
    print(p.second);
}
```

- [ ] **Step 6: Run test**

```powershell
cargo run -- tests/generic_struct.ky
```

Expected:
```
1
2
```

- [ ] **Step 7: Commit**

```powershell
git add src/codegen/mono.rs src/codegen/program.rs src/ast.rs tests/generic_struct.ky
git commit -m "feat(codegen): generic struct monomorphization"
```

---

## Task 10: Duck Typing Error Messages

**Files:**
- Modify: `src/codegen/mono.rs`
- Modify: `src/codegen/exprs.rs`

When a generic is instantiated with a type that doesn't support an operation used in the body, emit a clear error at the call site.

- [ ] **Step 1: Write failing test**

Create `tests/duck_error.ky`:

```kyte
struct NoAdd {}

fn add<T>(a: T, b: T) -> T {
    return a + b;
}

@main(main) {
    NoAdd x = NoAdd {};
    NoAdd y = NoAdd {};
    NoAdd z = add(x, y);  // should error: NoAdd doesn't support +
}
```

- [ ] **Step 2: Run — note current behavior**

```powershell
cargo run -- tests/duck_error.ky 2>&1
```

Note whether it panics or gives a confusing error.

- [ ] **Step 3: Add operation compatibility check in exprs codegen**

In `src/codegen/exprs.rs`, in the binary operator emit code, when both operands are struct types and the operator isn't defined for them, emit a diagnostics error instead of panicking:

```rust
// In BinOp handling, before attempting to emit:
if is_struct_type(&left_ty) && !operator_supported_for(&left_ty, op) {
    self.diagnostics.push(format!(
        "error: type `{}` does not support operator `{}` (required by generic instantiation)",
        type_name(&left_ty), op_symbol(op)
    ));
    return self.i64_type().const_int(0, false).into(); // poison value, compilation fails
}
```

- [ ] **Step 4: Run test — expect clean error**

```powershell
cargo run -- tests/duck_error.ky 2>&1
```

Expected: error message mentioning `NoAdd` and `+`, not a Rust panic.

- [ ] **Step 5: Commit**

```powershell
git add src/codegen/exprs.rs tests/duck_error.ky
git commit -m "feat(codegen): duck typing error messages for unsupported generic operations"
```

---

## Task 11: Full Integration Test

- [ ] **Step 1: Create end-to-end test**

Create `tests/phase1_integration.ky`:

```kyte
extern fn strlen(s: string) -> int;

fn max<T>(a: T, b: T) -> T {
    if a > b {
        return a;
    }
    return b;
}

fn identity<T>(x: T) -> T {
    return x;
}

struct Box<T> {
    T value;
}

fn make_box<T>(v: T) -> Box<T> {
    return Box { value: v };
}

@main(main) {
    // extern fn
    int len = strlen("kyte");
    print(len);           // 4

    // generic functions
    int m = max(3, 7);
    print(m);             // 7

    float fm = max(1.5, 2.5);
    print(fm);            // 2.5

    string s = identity("hello");
    print(s);             // hello

    // generic struct
    Box<int> b = make_box(99);
    print(b.value);       // 99

    // grouped import would be: import std.io.{print, readline};
    // (tested separately once std/ has content)
}
```

- [ ] **Step 2: Run**

```powershell
cargo run -- tests/phase1_integration.ky
```

Expected output:
```
4
7
2.5
hello
99
```

- [ ] **Step 3: Run full test suite**

```powershell
cargo test
```

Expected: all unit tests pass.

- [ ] **Step 4: Final commit**

```powershell
git add tests/phase1_integration.ky
git commit -m "test: Phase 1 integration test — extern fn, generics, grouped imports"
```

---

## Self-Review

**Spec coverage:**
- [x] `extern fn` — Tasks 1–4
- [x] Grouped import `{A, B, C}` — Tasks 5–6
- [x] std/ directory resolution — Task 6
- [x] Generic function monomorphization — Tasks 7–8
- [x] Generic struct monomorphization — Task 9
- [x] Duck typing error messages — Task 10
- [x] Integration test — Task 11

**Placeholder scan:** No TBD, TODO, or "similar to above" patterns.

**Type consistency:**
- `MonoKey::mangled_fn_name()` used in Task 7 step 3 and Task 8 step 3 — consistent.
- `Ty::TypeParam` used throughout — matches `src/ast.rs` definition.
- `TopLevel::ExternFn` defined in Task 2, used in Tasks 3, 4 — consistent.
- `substitute_ty` / `substitute_params` / `specialize_function` defined in Task 7, used in Task 8 — consistent.

**Known limitation:** Task 8 step 4 notes that `emit_specialization` needs access to `program`. The exact mechanism (storing `program` on `self` temporarily, or passing as param through the call chain) should be resolved during implementation — the most ergonomic approach in the existing codegen style should be used.
