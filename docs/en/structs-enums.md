# Structs & Enums

---

## Structs

A struct groups related fields together. Fields have explicit types, terminated by `;`.

```kyte
struct Point {
    float x;
    float y;
}

struct User {
    string name;
    int age;
    bool active;
}
```

### Creating instances

```kyte
Point p = Point { x: 1.0, y: 2.5 };
User u = User { name: "Alice", age: 30, active: true };
```

### Accessing fields

```kyte
print(p.x);        // 1.0
print(u.name);     // Alice
```

### Mutating fields

```kyte
u.age = 31;
p.x += 0.5;
```

---

## Enums

Enums define a type with a fixed set of variants. Variants can optionally carry a value.

### Simple enum

```kyte
enum Direction {
    North,
    South,
    East,
    West,
}
```

```kyte
Direction d = Direction.North;
```

### Enum with payload

Variants can carry a single value:

```kyte
enum Option {
    Some(int),
    None,
}

enum Shape {
    Circle(float),    // radius
    Square(float),    // side length
    Rectangle(float), // width (simplified)
}
```

```kyte
Option val = Option.Some(42);
Shape s = Shape.Circle(3.14);
```

### Using enums in match

This is where enums shine:

```kyte
Option result = Option.Some(99);

match result {
    Option.Some(n) => { print(n); }
    Option.None    => { print("nothing"); }
}
```

For payload variants, the inner value is bound to the identifier you name in the pattern — `n` in this case.

---

## Putting it Together

```kyte
enum Color {
    Red,
    Green,
    Blue,
}

struct Pixel {
    int x;
    int y;
    Color color;
}

@main(main) {
    Pixel px = Pixel { x: 10, y: 20, color: Color.Red };

    match px.color {
        Color.Red   => { print("red pixel"); }
        Color.Green => { print("green pixel"); }
        Color.Blue  => { print("blue pixel"); }
    }
}
```

---

## Tips

- Struct field order matters at initialization — always use field names (`{ x: 1.0, y: 2.5 }`).
- Enums don't have methods by themselves — combine with `impl` for that (see [Traits & Impl](traits-impl.md)).
- Enum variants with payloads can only carry one value. Use a struct if you need multiple fields.
