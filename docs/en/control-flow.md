# Control Flow

---

## if / else

```kyte
int score = 85;

if score >= 90 {
    print("A");
} else if score >= 80 {
    print("B");
} else {
    print("C or below");
}
```

Conditions don't need parentheses. Braces are required.

---

## for — Range Loop

```kyte
for i in 0..5 {
    print(i);   // 0 1 2 3 4
}
```

The range `0..5` is exclusive on the right side (like Rust).

Loop backwards? Use a while loop for now:

```kyte
int i = 5;
while i > 0 {
    i -= 1;
    print(i);   // 4 3 2 1 0
}
```

---

## while

```kyte
int n = 1;
while n < 100 {
    n *= 2;
}
print(n);   // 128
```

---

## loop

An infinite loop. Use `break` or `return` to exit:

```kyte
int count = 0;
loop {
    count += 1;
    if count >= 5 { break; }
}
print(count);   // 5
```

---

## break

Exits the innermost loop:

```kyte
for i in 0..100 {
    if i == 7 { break; }
    print(i);
}
```

---

## match

Pattern matching. Clean alternative to long if/else chains.

### Match on integers

```kyte
int x = 2;
match x {
    1 => { print("one"); }
    2 => { print("two"); }
    3 => { print("three"); }
    _ => { print("other"); }   // wildcard — catches everything else
}
```

### Match on enums

```kyte
enum Direction { North, South, East, West }

Direction d = Direction.North;

match d {
    Direction.North => { print("Going north"); }
    Direction.South => { print("Going south"); }
    _               => { print("East or West"); }
}
```

### Match on enums with payloads

```kyte
enum Result {
    Ok(int),
    Err,
}

Result r = Result.Ok(42);

match r {
    Result.Ok(n) => { print(n); }   // prints 42
    Result.Err   => { print("error"); }
}
```

The `_` wildcard must come last. Every match arm body is a block `{ }`.

---

## assert

Runtime assertion — panics if the condition is false:

```kyte
int x = 42;
assert(x > 0);       // fine
assert(x == 0);      // panics at runtime
```

Use it for invariants and debugging. Not a substitute for error handling.

---

## yield

Yields a value from an anchor (see [Anchors](anchors.md)):

```kyte
@worker(thread) {
    int result = compute();
    yield result;
}
```

---

## Exit

Terminates the program immediately:

```kyte
if fatal_error {
    Exit;
}
```

Think of it as `exit(0)` — no cleanup, no unwinding, just done.
