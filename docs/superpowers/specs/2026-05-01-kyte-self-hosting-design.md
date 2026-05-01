# Kyte Self-Hosting Design

**Date:** 2026-05-01  
**Goal:** Add language features + standard library so Kyte can write its own compiler.

---

## Priorities

1. **Performance** — zero-cost abstractions, no hidden allocations
2. **Safety** — no raw pointers, no dangling references, no manual memory
3. **DX** — minimal syntax, consistent with existing Kyte style

---

## Core Decisions

| Topic | Decision |
|-------|----------|
| Pointers | Not added. Handle + Pool pattern instead. |
| Generic constraints | Duck typing — checked at usage site, no `<T: Trait>` syntax |
| Error handling | anchor catch for failures, `Option<T>` for nullable values, no Result |
| FFI | `extern fn` keyword, stdlib-internal only |
| Import syntax | `import std.io;` / `import std.collections.{Vec, Map};` |
| Stdlib language | Kyte (using `extern` at the bottom layer) |
| Builtins migration | `print`/`assert`/`len`/`emit` move to stdlib, core stays minimal |

---

## Phase 1 — Language Core Extensions

### 1. Generic Monomorphization

Current state: `<T>` parses but codegen falls back to i64.  
Fix: at usage site, instantiate separate LLVM function per concrete type.

```kyte
struct Vec<T> { data: Handle, len: u32, cap: u32 }
fn push<T>(v: Vec<T>, val: T) { ... }

Vec<int> nums = Vec.new();    // instantiates Vec_int
Vec<string> words = Vec.new(); // instantiates Vec_string
```

Duck typing: if T doesn't support an operation used inside the generic body, error at the call site with a clear message.

### 2. `extern` Keyword

Declares a C function for linking. Used only inside `std/internal/`.

```kyte
extern fn malloc(size: u64) -> Handle;
extern fn free(ptr: Handle);
extern fn write(fd: i32, buf: Handle, count: u64) -> i64;
extern fn read(fd: i32, buf: Handle, count: u64) -> i64;
```

Compiler emits an LLVM external function declaration, no body generated.  
Regular Kyte code can technically use `extern` but it is unsupported outside stdlib.

### 3. Import System Extension

```kyte
import std.io;                           // entire module
import std.collections.Vec;              // single item
import std.collections.{Vec, Map, Pool}; // grouped
```

Resolution order:
1. Built-in `std/` path (bundled with compiler)
2. Project-local path relative to workspace root
3. Compile error if not found

Circular imports → compile error with cycle printed.

---

## Phase 2 — Type System

### Option\<T\>

Defined in `std/option.ky`, auto-imported (no explicit import needed).

```kyte
enum Option<T> {
    Some(T),
    None
}
```

Usage:
```kyte
Option<int> found = map.get("key");

match found {
    Some(val) => print(val),
    None      => print("not found")
}
```

### Handle

Opaque u32 wrapping a heap address. Returned by Pool/Vec internals, never exposed as a raw pointer.  
User code never creates a Handle directly.

### Pool\<T\>

Arena allocator. All nodes live in a contiguous slab. Safe — no dangling possible.

```kyte
Vault Pool<Expr> pool = Pool.new();
ExprId id = pool.alloc(Expr { ... });
Expr node = pool.get(id);
```

`ExprId` is a type alias for u32 — just an index into the pool.  
Pool freed automatically when it goes out of scope (Vault semantics).

---

## Phase 3 — Standard Library

### Directory Structure

```
std/
├── option.ky          Option<T>  (auto-imported)
├── io.ky              print, println, eprintln, stdin
├── fs.ky              File, read_file, write_file, exists
├── string.ky          split, trim, contains, starts_with, parse_int
├── process.ky         exec, exit, env_get
├── collections/
│   ├── vec.ky         Vec<T>
│   ├── pool.ky        Pool<T>, Handle
│   └── map.ky         Map<K, V>
└── internal/
    └── mem.ky         extern malloc, free, realloc (not public)
```

### Key APIs

**Vec\<T\>**
```kyte
Vec<T> v = Vec.new();
v.push(item);
T x = v.get(0);
int n = v.len();
v.pop();
```

**Map\<K, V\>**
```kyte
Map<string, int> m = Map.new();
m.set("key", 42);
Option<int> val = m.get("key");
bool has = m.has("key");
m.delete("key");
```

**std.io**
```kyte
import std.io;
io.print("hello");
io.println(f"value = {x}");
string line = io.readline();
```

**std.string**
```kyte
import std.string;
Vec<string> parts = string.split(s, " ");
string t = string.trim(s);
bool b = string.contains(s, "foo");
Option<int> n = string.parse_int(s);
```

**std.fs**
```kyte
import std.fs;
string content = fs.read_file("main.ky");
fs.write_file("out.c", generated);
bool ok = fs.exists("path/to/file");
```

**std.process**
```kyte
import std.process;
process.exec("cargo build");
Option<string> val = process.env_get("PATH");
process.exit(0);
```

---

## Phase 4 — Builtin Migration

| Current builtin | Moves to | Compatibility |
|-----------------|----------|---------------|
| `print(x)` | `std.io.print` | Kept as shorthand, deprecated warning |
| `assert(c)` | `std.assert` | Same |
| `len(x)` | `.len()` method on Vec/string | `len()` removed |
| `emit(e, p)` | `std.anchor.emit` | Same |

After migration, language core contains only:
- Type system (primitives, struct, enum, trait, impl)
- Control flow (if/else, loop, while, for, match, break, return)
- Vault, Anchor
- Operators

---

## Implementation Order

```
Phase 1: Generic monomorphization  →  extern keyword  →  import extension
Phase 2: Option<T>  →  Vec<T>  →  Pool<T>  →  Map<K,V>
Phase 3: std.io  →  std.string  →  std.fs  →  std.process
Phase 4: Migrate builtins  →  remove from core
```

Each phase is independently usable. Stop at any phase and Kyte still compiles.
