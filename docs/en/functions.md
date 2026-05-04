# Functions

Functions are declared with `fn`. Parameters are `type name` pairs; return type follows `->`. Omit `->` for void.

---

## Basic Syntax

```kyte
fn add(int a, int b) -> int {
    return a + b;
}

fn greet(string name) {
    print(f"Hello, {name}!");
}
```

```kyte
@main(main) {
    int result = add(3, 4);  // 7
    greet("Kyte");            // Hello, Kyte!
}
```

---

## Why `type name` Order?

Kyte puts the type first: `int a`, `string name`. This matches variable declarations and struct fields, so the pattern is always the same:

```kyte
int x = 42;                    // variable
struct Point { int x; }        // field
fn move(int dx, int dy) { }    // parameter
```

---

## >> Pipe Operator

Thread a value through a chain of functions with `>>`. The left-hand value becomes the **first argument** of the right-hand function:

```kyte
fn double(int n) -> int { return n * 2; }
fn clamp(int n, int lo, int hi) -> int {
    if n < lo { return lo; }
    if n > hi { return hi; }
    return n;
}

@main(main) {
    int x = 5;
    x >> double >> print          // print(double(x)) → 10
    x >> clamp(0, 8) >> print     // clamp(x, 0, 8) → 5, then print
}
```

When the target function takes extra arguments, write them in parentheses — the piped value is inserted first:

| Written | Compiled as |
|---|---|
| `data >> print` | `print(data)` |
| `data >> clamp(0, 10)` | `clamp(data, 0, 10)` |
| `a >> f(b) >> g(c)` | `g(f(a, b), c)` |

Pipe desugars to a `Call` node at parse time — no runtime overhead. Code reads left to right instead of inside out.

---

## Early Return

Use `return` to exit at any point. Great for guard clauses — reject bad input upfront and keep the happy path clean:

```kyte
fn find(int[] arr, int target) -> int {
    for i in 0..len(arr) {
        if arr[i] == target { return i; }
    }
    return -1;
}
```

Void functions can use bare `return;` to exit early:

```kyte
fn process(int x) {
    if x < 0 { return; }
    print(x);
}
```

---

## Closures

Closures are anonymous functions assigned to a variable. Use them for one-off transformations or callbacks:

```kyte
auto double = |n: int| { return n * 2; };
auto clamp  = |v: int, lo: int, hi: int| {
    if v < lo { return lo; }
    if v > hi { return hi; }
    return v;
};

print(double(21));           // 42
print(clamp(150, 0, 100));   // 100
```

Parameters use `|name: type, ...|` syntax. Closures are capture-free function pointers — they don't close over local variables. Think of them as lightweight lambdas you can pass around.

---

## Generics

Write a function once and have it work across types with `<T>`:

```kyte
fn identity<T>(T val) -> T {
    return val;
}

fn max_of<T>(T a, T b) -> T {
    if a > b { return a; }
    return b;
}
```

```kyte
@main(main) {
    int x  = identity(42);
    float y = identity(3.14);
    int m  = max_of(10, 20);   // 20
}
```

Why bother? Without generics you'd write `max_int`, `max_float`, `max_string` separately. With `<T>`, one function covers all comparable types. The compiler monomorphizes it at each call site — zero runtime overhead.

---

## Method-Style Functions

Attach a function to a struct using `fn TypeName.method()` syntax:

```kyte
struct Vec2 {
    float x;
    float y;
}

fn Vec2.length(Vec2 self) -> float {
    return (self.x * self.x + self.y * self.y) as float;
}

fn Vec2.scale(Vec2 self, float factor) -> Vec2 {
    return Vec2 { x: self.x * factor, y: self.y * factor };
}
```

Call it by passing the instance explicitly:

```kyte
@main(main) {
    Vec2 v = Vec2 { x: 3.0, y: 4.0 };
    float len = Vec2.length(v);
    print(len);   // 25.0 (sum of squares)
}
```

> For a more structured approach — enforced contracts across types — see [Traits & Impl](traits-impl.md).

---

## Recursion

Recursive functions work normally:

```kyte
fn factorial(int n) -> int {
    if n <= 1 { return 1; }
    return n * factorial(n - 1);
}

fn fib(int n) -> int {
    if n <= 1 { return n; }
    return fib(n - 1) + fib(n - 2);
}
```

---

## Tips

- Primitive parameters (`int`, `float`, `bool`) are passed by value.
- `string` and structs are passed as pointers internally.
- If you declare a return type, the compiler checks that every code path returns a value.
- Functions are top-level only — no nested functions. Use closures instead.
