# Types & Variables

Kyte is statically typed — every value has a type, and the compiler enforces it. But `auto` means you don't have to spell it out when it's obvious.

---

## Primitive Types

| Type | Description | Literals |
|---|---|---|
| `int` | 64-bit signed integer | `42`, `-7`, `0` |
| `float` | 64-bit floating point | `3.14`, `-0.5`, `1.0` |
| `string` | UTF-8 string | `"hello"`, `""` |
| `bool` | Boolean | `true`, `false` |

```kyte
int score = 100;
float ratio = 0.75;
string label = "Kyte";
bool done = false;
```

---

## Fixed-Width Integers

`int` is 64-bit and covers most use cases. For memory-layout-sensitive code or C FFI, specify exactly what you need:

| Signed | Unsigned | Signed range |
|---|---|---|
| `i8` | `u8` | -128 to 127 |
| `i16` | `u16` | -32,768 to 32,767 |
| `i32` | `u32` | ±2 billion |
| `i64` | `u64` | same as `int` |

```kyte
u8 byte_val = 255;
i32 pixel_x = -400;
```

---

## Type Inference with `auto`

When the type is obvious from the right-hand side, use `auto`:

```kyte
auto x = 42;         // int
auto pi = 3.14;      // float
auto name = "kyte";  // string
auto ok = true;      // bool
```

Why bother? It makes refactoring easier — change the right-hand side and you don't have to update the type annotation everywhere. Also handy for long struct/enum type names.

**`auto` doesn't work in:** function parameters, struct fields, or return types. Those must be explicit.

---

## Type Casting with `as`

Explicit conversions only — Kyte won't silently coerce numeric types:

```kyte
int x = 42;
float y = x as float;   // 42.0

float f = 3.99;
int i = f as int;       // 3 (truncates, does NOT round)

bool b = true;
int n = b as int;       // 1
```

If you write `int + float`, you get a compile error. Use `as` to make the intent clear.

---

## Arrays

```kyte
int[] nums = [1, 2, 3, 4, 5];
string[] tags = ["kyte", "fast", "native"];
float[] coords = [1.0, 2.5, -0.3];
```

Access and mutate by index:

```kyte
int first = nums[0];    // 1
nums[2] = 99;           // [1, 2, 99, 4, 5]
int count = len(nums);  // 5
```

Out-of-bounds access is caught at runtime — you get a clear error instead of silently reading garbage memory.

**Iterate with index:**

```kyte
for i in 0..len(nums) {
    print(nums[i]);
}
```

---

## Strings & F-strings

Plain string literals:

```kyte
string greeting = "Hello, World!";
```

**F-strings** — embed expressions directly with `{}`:

```kyte
string name = "Kyte";
int version = 2;
string msg = f"Welcome to {name} v{version}!";
print(msg);
// Welcome to Kyte v2!
```

Any expression works inside `{}`:

```kyte
int a = 3;
int b = 4;
print(f"{a} + {b} = {a + b}");
// 3 + 4 = 7
```

**Float formatting:** F-strings strip trailing zeros (`3.14`). Direct `print(float_val)` prints 6 decimal places (`3.140000`).

Escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`, `\0`.

---

## Vault — Heap Allocation

By default, everything lives on the **stack** and disappears when it goes out of scope. You need the heap in two cases:

1. **State that must survive an Anchor restart** — Kill discards the stack but not the heap.
2. **Data too large for the stack** — big buffers, long arrays.

Prefix with `Vault`:

```kyte
Vault int counter = 0;
Vault int[] buffer = [1, 2, 3, 4, 5];
```

Vault variables are **freed automatically** by the compiler — no `free()` calls needed. The compiler runs last-use analysis and inserts `free()` at exactly the right point.

See [Memory](memory.md) for the full story.

---

## Constants

Top-level, immutable values. Accessible anywhere in the file:

```kyte
const int MAX_RETRIES = 5;
const float PI = 3.14159;
const string APP_NAME = "MyApp";
const bool DEBUG = false;
```

Why use constants? Named constants make magic numbers disappear. When you see `MAX_RETRIES` you know what it means. When the value changes, you update one line.

Constants require an explicit type — `auto` isn't allowed here.
