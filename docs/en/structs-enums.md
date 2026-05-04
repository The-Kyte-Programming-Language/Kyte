# Structs & Enums

Two ways to build custom types. Structs **group related data**; enums **represent one of several states**.

---

## Structs

A struct bundles related fields under one type name. Every field has an explicit type ending with `;`:

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

Field names are always required — positional initialization isn't allowed. This prevents subtle bugs when fields are reordered or new ones are added:

```kyte
Point origin = Point { x: 0.0, y: 0.0 };
User alice   = User { name: "Alice", age: 30, active: true };
```

### Accessing and mutating fields

```kyte
print(alice.name);    // Alice
print(alice.age);     // 30

alice.age = 31;
origin.x += 1.0;
```

---

## Enums

Enums define a type with a fixed set of variants. Use them instead of magic booleans or magic integers — the name makes intent clear and the compiler enforces exhaustive handling.

```kyte
enum Direction {
    North,
    South,
    East,
    West,
}
```

```kyte
Direction heading = Direction.North;
```

Why enum instead of `bool`? `bool is_north` can only mean two things and doesn't scale when you add `Northeast`. `Direction` names all the cases, and the compiler tells you when you've missed one in a `match`.

### Variants with payloads

A variant can carry a single value:

```kyte
enum Shape {
    Circle(float),   // radius
    Rect(float),     // area (simplified)
}

enum Event {
    Click(int),   // element ID
    Resize,
    Quit,
}
```

```kyte
Shape s = Shape.Circle(3.14);
Event e = Event.Click(42);
```

Extract the payload with `match`:

```kyte
match s {
    Shape.Circle(r) => { print(f"circle, radius {r}"); }
    Shape.Rect(a)   => { print(f"rect, area {a}"); }
}
```

`r` is a pattern binding — you can name it anything you like.

---

## Putting it Together

The most common pattern: an enum field inside a struct, unpacked with `match`:

```kyte
enum Status {
    Active,
    Banned(string),   // reason
    Pending,
}

struct User {
    string name;
    int age;
    Status status;
}

@main(main) {
    User bob = User {
        name: "Bob",
        age: 25,
        status: Status.Banned("spam"),
    };

    match bob.status {
        Status.Active      => { print(f"{bob.name}: active"); }
        Status.Banned(why) => { print(f"{bob.name}: banned — {why}"); }
        Status.Pending     => { print(f"{bob.name}: pending"); }
    }
}
```

Output:
```
Bob: banned — spam
```

---

## Pattern Matching on Structs

Structs can be destructured directly inside a `match` arm:

```kyte
match point {
    Point { x, y } when x > 0 => { print(f"right: {x}, {y}"); }
    Point { x, y }             => { print(f"left: {x}, {y}"); }
}
```

You can name only the fields you care about — unlisted fields are ignored:

```kyte
match user {
    User { name, status: Status.Active }       => { print(f"active: {name}"); }
    User { name, status: Status.Banned(code) } => { print(f"banned: {name} ({code})"); }
}
```

For full syntax (nested enum sub-patterns, `when` guards) see **Struct Pattern Destructuring** in [Control Flow](control-flow.md).

---

## Tips

- Payload variants carry **one** value. If you need multiple fields, put them in a struct.
- Struct fields have no defaults — every field must be set at initialization.
- To attach methods to enums or structs, use `impl` + `trait` (see [Traits & Impl](traits-impl.md)).
