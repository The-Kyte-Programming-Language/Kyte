# Traits & Impl

A trait is a contract — it declares what functions a type must have. `impl` fulfills that contract for a specific type.

Use traits when multiple structs need to share the same interface. For example, if both `Circle` and `Rect` must have an `area()` function, define that in a trait.

---

## Defining a Trait

Trait bodies contain function signatures only — no implementations:

```kyte
trait Printable {
    fn print_info();
}

trait Shape {
    fn area() -> float;
    fn describe();
}
```

---

## Implementing a Trait

Use `impl TraitName for TypeName`. Inside the method body, access the current instance's fields via `TypeName.fieldName`:

```kyte
struct Circle {
    float radius;
}

struct Rect {
    float width;
    float height;
}

impl Shape for Circle {
    fn area() -> float {
        return 3.14159 * Circle.radius * Circle.radius;
    }
    fn describe() {
        print(f"circle, radius = {Circle.radius}");
    }
}

impl Shape for Rect {
    fn area() -> float {
        return Rect.width * Rect.height;
    }
    fn describe() {
        print(f"rect, {Rect.width} x {Rect.height}");
    }
}
```

`Circle.radius` inside `impl Shape for Circle` refers to the field of the current instance — not a static value.

---

## Calling Trait Methods

Create an instance and call via `TypeName.method()`:

```kyte
@main(main) {
    Circle c = Circle { radius: 5.0 };
    Rect r   = Rect { width: 4.0, height: 3.0 };

    float ca = Circle.area();   // resolved to Circle's impl
    float ra = Rect.area();     // resolved to Rect's impl

    Circle.describe();   // circle, radius = 5.0
    Rect.describe();     // rect, 4.0 x 3.0
}
```

---

## Multiple Traits on One Type

A struct can implement any number of traits — just write separate `impl` blocks:

```kyte
trait Named {
    fn name() -> string;
}

trait Drawable {
    fn draw();
}

struct Button {
    string label;
    int x;
    int y;
}

impl Named for Button {
    fn name() -> string {
        return Button.label;
    }
}

impl Drawable for Button {
    fn draw() {
        print(f"[{Button.label}] @ ({Button.x}, {Button.y})");
    }
}
```

---

## Why Use Traits?

You could write `fn Circle.area()` and `fn Rect.area()` separately without a trait. What traits add:

- **Enforcement** — the compiler verifies you've implemented every declared function.
- **Explicit contract** — "this type behaves like a Shape" is stated in code, not comments.
- **Consistent API** — same function names and signatures across all implementing types.

---

## Current Limitations

- No dynamic dispatch (`dyn Trait` style) — all trait calls are resolved at compile time.
- No trait inheritance.
- No default method implementations — every method must be implemented explicitly.
