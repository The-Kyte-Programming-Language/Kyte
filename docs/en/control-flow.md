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

No parentheses around the condition. Braces are always required, even for one-liners.

---

## for — Range Loop

```kyte
for i in 0..5 {
    print(i);
}
// 0 1 2 3 4
```

`0..5` is exclusive on the right (same as Rust). Iterate over an array:

```kyte
int[] nums = [10, 20, 30, 40, 50];
for i in 0..len(nums) {
    print(nums[i]);
}
```

Need to go backwards? Use a while loop:

```kyte
int i = 4;
while i >= 0 {
    print(i);
    i -= 1;
}
// 4 3 2 1 0
```

---

## while

Repeats as long as the condition holds:

```kyte
int n = 1;
while n < 1000 {
    n *= 2;
}
print(n);  // 1024
```

Parentheses around the condition are optional — omitting them is cleaner.

---

## loop

Infinite loop. You decide when to exit:

```kyte
int count = 0;
loop {
    count += 1;
    if count >= 5 { break; }
}
print(count);  // 5
```

Clearer than `while true` — explicitly signals "the exit condition lives inside the body."

---

## break

Exits the innermost loop immediately:

```kyte
for i in 0..100 {
    if i == 7 { break; }
    print(i);
}
// 0 1 2 3 4 5 6
```

In nested loops, `break` only exits the **innermost** one. To break out of multiple levels, use a flag variable or extract the loops into a function.

---

## continue

Skips the rest of the current iteration and moves on to the next one:

```kyte
// Print only even numbers
for i in 0..10 {
    if i % 2 != 0 { continue; }
    print(i);
}
// 0 2 4 6 8
```

```kyte
// Sum only positive values
int[] vals = [3, -1, 7, -2, 5];
int sum = 0;
for i in 0..len(vals) {
    if vals[i] < 0 { continue; }
    sum = sum + vals[i];
}
print(sum);  // 15
```

`break` **ends** the loop. `continue` **skips** the current iteration.

---

## match

Pattern matching — the clean alternative to long `if/else if` chains.

### Match on integers

```kyte
int code = 404;

match code {
    200 => { print("OK"); }
    404 => { print("Not Found"); }
    500 => { print("Server Error"); }
    _   => { print(f"Unknown: {code}"); }
}
```

`_` is the wildcard — it catches everything not matched above. It must come last.

### Match on strings

```kyte
string cmd = "quit";

match cmd {
    "start" => { print("Starting!"); }
    "stop"  => { print("Stopping!"); }
    "quit"  => { print("Bye!"); }
    _       => { print("Unknown command"); }
}
```

### Match on enums

This is where match really shines:

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
    Result.Ok(n) => { print(f"Success: {n}"); }
    Result.Err   => { print("Failed"); }
}
```

`n` is a pattern binding — the payload is extracted and bound to that name. You can name it anything.

### when Guards

Add a condition after a pattern with `when`. The arm only fires if the pattern matches *and* the guard is true:

```kyte
int score = 85;

match score {
    n when n >= 90 => { print("A"); }
    n when n >= 80 => { print("B"); }
    n              => { print(f"C: {n}"); }
}
```

`n` is a **binding pattern** — it matches any value and binds it to `n`. The type is inferred from the match subject.

When the guard is false, matching falls through to the next arm. Works on enum payloads too:

```kyte
match result {
    Result.Ok(val) when val > 100 => { print("big"); }
    Result.Ok(val)                => { print(f"ok: {val}"); }
    Result.Err                    => { print("fail"); }
}
```

### Struct Pattern Destructuring

Pull struct fields out directly in a match arm:

```kyte
struct Point { int x; int y; }

Point p = Point { x: 3, y: 7 };

match p {
    Point { x, y } when x > 0 => { print(f"right: {x}, {y}"); }
    Point { x, y }             => { print(f"left: {x}, {y}"); }
}
```

Shorthand `{ x }` binds the field to a variable of the same name. Nested enum patterns are also supported:

```kyte
enum Status { Active, Banned(int) }

struct User {
    string name;
    Status status;
}

match user {
    User { name, status: Status.Active }       => { print(f"{name}: active"); }
    User { name, status: Status.Banned(code) } => { print(f"{name}: banned {code}"); }
}
```

| Syntax | Meaning |
|---|---|
| `{ x }` | bind field `x` to variable `x` |
| `{ x: 42 }` | match field `x` == 42 |
| `{ x: Status.Banned(n) }` | match field `x` against enum variant, bind payload to `n` |

Unlisted fields are ignored — no need to name every field.

---

## assert

Validates invariants. Panics immediately if the condition is false:

```kyte
assert(x > 0);
assert(len(arr) > 0, "array must not be empty");
```

For debugging and invariant checks. Don't use it for user input validation — if it fires, the program dies. Use `if`/`Kill` for recoverable errors.

---

## Exit

Terminates the program immediately. No cleanup, no unwinding:

```kyte
if config_missing {
    print("No config file found.");
    Exit;
}
```

Equivalent to `exit(0)`. Reserve it for unrecoverable initialization failures.
