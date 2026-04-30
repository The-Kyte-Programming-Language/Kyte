# Traits & Impl

Traits define a contract — a set of functions that a type must implement. `impl` wires a type to a trait.

---

## Defining a Trait

```kyte
trait Greet {
    fn greet(string name) -> string;
}

trait Drawable {
    fn draw();
    fn area() -> float;
}
```

Trait bodies contain function signatures only — no implementations.

---

## Implementing a Trait

```kyte
struct Dog {
    string name;
}

impl Greet for Dog {
    fn greet(string name) -> string {
        return f"Woof! I'm {name}!";
    }
}
```

Now `Dog` satisfies the `Greet` contract. Every function in the trait must be implemented.

---

## Full Example

```kyte
trait Shape {
    fn area() -> float;
    fn describe();
}

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
        print(f"Circle with radius {Circle.radius}");
    }
}

impl Shape for Rect {
    fn area() -> float {
        return Rect.width * Rect.height;
    }
    fn describe() {
        print(f"Rect {Rect.width}x{Rect.height}");
    }
}
```

---

## Calling Trait Methods

Call the methods using the type-qualified name:

```kyte
@main(main) {
    Circle c = Circle { radius: 5.0 };
    float a = Circle.area();
    Circle.describe();
}
```

---

## Tips

- A type can implement multiple traits — just write multiple `impl` blocks.
- Traits are a great way to enforce consistent APIs across different struct types.
- Unlike Rust, there's no `dyn Trait` dispatch yet — trait calls are resolved at compile time.
