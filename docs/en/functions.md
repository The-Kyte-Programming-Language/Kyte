# Functions

Functions are declared with `fn`. Parameters are typed, return types are optional (void if omitted).

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

Call them like you'd expect:

```kyte
@main(main) {
    int result = add(3, 4);   // 7
    greet("Kyte");             // Hello, Kyte!
}
```

---

## Multiple Parameters

Parameters are separated by commas, each with `type name`:

```kyte
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}
```

---

## Void Functions

Omit the `->` return type for functions that don't return a value:

```kyte
fn log(string msg) {
    print(msg);
}
```

You can still use `return;` to exit early:

```kyte
fn process(int x) {
    if x < 0 { return; }
    print(x);
}
```

---

## Closures

Closures are anonymous functions assigned to variables. They use `|params: types|` syntax:

```kyte
auto double = |n: int| { return n * 2; };
auto add = |a: int, b: int| { return a + b; };

int d = double(21);   // 42
int s = add(10, 5);   // 15
```

Closures are capture-free function pointers — they can't close over local variables (yet). Think of them as lightweight named lambdas.

---

## Generics

Functions can be generic over types using `<T>`:

```kyte
fn identity<T>(T val) -> T {
    return val;
}

fn max<T>(T a, T b) -> T {
    if a > b { return a; }
    return b;
}

@main(main) {
    int x = identity(42);
    float y = identity(3.14);
    int m = max(10, 20);   // 20
}
```

---

## Method-style Functions

Functions can be defined with a type prefix to act like methods:

```kyte
struct Vec2 {
    float x;
    float y;
}

fn Vec2.length(Vec2 self) -> float {
    return (self.x * self.x + self.y * self.y) as float;
}

@main(main) {
    Vec2 v = Vec2 { x: 3.0, y: 4.0 };
    float len = Vec2.length(v);
    print(len);
}
```

---

## Early Return

Use `return` to exit a function at any point:

```kyte
fn find(int[] arr, int target) -> int {
    for i in 0..10 {
        if arr[i] == target { return i; }
    }
    return -1;
}
```

---

## Tips

- Parameters are always pass-by-value for primitives.
- Return type annotation is required when the function returns something.
- Recursive functions work just fine.
