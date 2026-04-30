# Types & Variables

Kyte is statically typed. Every variable has a type, and the compiler checks it. But thanks to `auto`, you don't always have to say it out loud.

---

## Primitive Types

| Type | Description | Example |
|---|---|---|
| `int` | 64-bit signed integer | `int x = 42;` |
| `float` | 64-bit floating point | `float pi = 3.14;` |
| `string` | UTF-8 string | `string s = "hello";` |
| `bool` | Boolean | `bool flag = true;` |

### Fixed-width integers

When you need exact sizes (interop, bitwise ops, etc.):

| Signed | Unsigned |
|---|---|
| `i8` | `u8` |
| `i16` | `u16` |
| `i32` | `u32` |
| `i64` | `u64` |

```kyte
i32 counter = 0;
u8 byte = 255;
```

---

## Type Inference with `auto`

Don't want to write the type? Let Kyte figure it out:

```kyte
auto x = 42;          // int
auto pi = 3.14;       // float
auto name = "kyte";   // string
auto ok = true;       // bool
```

`auto` works for variables whose type is obvious from the right-hand side. It doesn't work for function parameters or struct fields — those must be explicit.

---

## Type Casting with `as`

```kyte
int x = 42;
float y = x as float;   // 42.0

float f = 3.99;
int i = f as int;        // 3  (truncates)
```

---

## Arrays

```kyte
int[] nums = [1, 2, 3, 4, 5];
string[] names = ["alice", "bob"];
```

Access by index:

```kyte
int first = nums[0];
nums[1] = 99;
```

---

## Strings & F-strings

Regular strings are just `"..."`:

```kyte
string greeting = "Hello, World!";
```

F-strings let you embed expressions inline (no `malloc`, stack-allocated):

```kyte
string name = "Kyte";
int version = 1;
string msg = f"Welcome to {name} v{version}!";
print(msg);  // Welcome to Kyte v1!
```

Escape sequences: `\n`, `\t`, `\r`, `\\`, `\"`, `\0`, `\xHH` (hex), `\uHHHH` (unicode).

---

## Vault — Heap Allocation

By default, all variables live on the stack. If you need heap allocation, prefix with `Vault`:

```kyte
Vault int x = 42;            // heap-allocated int
Vault int[] arr = [1, 2, 3]; // heap-allocated array

free(x);    // manual deallocation
free(arr);
```

See [Memory](memory.md) for the full story.

---

## Constants

Top-level immutable values:

```kyte
const int MAX_RETRIES = 3;
const string APP_NAME = "Kyte";
```

Constants must have an explicit type. They can be used anywhere in the file.
