# 함수

함수는 `fn`으로 선언합니다. 매개변수는 `타입 이름` 순서, 반환 타입은 `->` 뒤에.

---

## 기본 문법

```kyte
fn add(int a, int b) -> int {
    return a + b;
}

fn greet(string name) {
    print(f"안녕하세요, {name}!");
}
```

```kyte
@main(main) {
    int result = add(3, 4);  // 7
    greet("Kyte");            // 안녕하세요, Kyte!
}
```

반환값이 없으면 `->`를 생략합니다. 빈 `return;`으로 함수 중간에 빠져나올 수 있습니다.

---

## 왜 `타입 이름` 순서인가요?

Kyte의 매개변수는 타입이 먼저 옵니다: `int a`, `string name`. C/Java와 같은 스타일입니다. `a: int` 스타일 (Rust/Swift)이 아닌 이유는 struct 필드, 변수 선언과 일관성을 유지하기 위해서입니다.

```kyte
// 선언 스타일이 일관됩니다:
int x = 42;                    // 변수
struct Point { int x; }        // 필드
fn move(int dx, int dy) { }    // 매개변수
```

---

## >> 파이프 연산자

값을 함수에 순서대로 넘길 때 `>>`를 씁니다. 왼쪽 값이 오른쪽 함수의 **첫 번째 인수**로 들어갑니다:

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
    x >> clamp(0, 8) >> print     // clamp(x, 0, 8) → 5, 그 다음 print
}
```

추가 인수가 있으면 `fn_name(arg1, arg2)` 형태로 씁니다 — 파이프된 값이 맨 앞 인수로 자동 삽입됩니다:

| 쓴 코드 | 컴파일되는 코드 |
|---|---|
| `data >> print` | `print(data)` |
| `data >> clamp(0, 10)` | `clamp(data, 0, 10)` |
| `a >> f(b) >> g(c)` | `g(f(a, b), c)` |

파이프는 **파싱 시점에 Call 노드로 변환**됩니다 — 런타임 오버헤드 없음. 중첩 함수 호출을 안에서 밖으로 읽는 불편함 없이 왼쪽에서 오른쪽으로 흐르는 코드를 쓸 수 있습니다.

---

## 조기 반환

중간에 `return`으로 바로 나올 수 있습니다. 중첩 조건을 피하는 가장 깔끔한 방법입니다:

```kyte
fn find(int[] arr, int target) -> int {
    for i in 0..len(arr) {
        if arr[i] == target { return i; }
    }
    return -1;
}
```

중첩 if 안에서 행복한 경로만 쫓는 것보다 훨씬 읽기 좋습니다.

---

## 클로저

클로저는 변수에 할당하는 익명 함수입니다. 일회성 변환 로직, 콜백에 쓰면 좋습니다:

```kyte
auto double = |n: int| { return n * 2; };
auto clamp  = |v: int, lo: int, hi: int| {
    if v < lo { return lo; }
    if v > hi { return hi; }
    return v;
};

print(double(21));       // 42
print(clamp(150, 0, 100));  // 100
```

매개변수는 `|이름: 타입, ...|` 형식입니다. 클로저는 외부 로컬 변수를 캡처하지 않습니다 — 가볍고 예측 가능한 함수 포인터라고 생각하면 됩니다.

---

## 제네릭

`<T>`로 타입에 상관없이 동작하는 함수를 만듭니다:

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
    int m  = max_of(10, 20);  // 20
}
```

제네릭이 왜 필요한가요? `max_int(a, b)`, `max_float(a, b)`를 따로 만드는 대신, 하나의 함수가 모든 비교 가능한 타입에 대해 동작합니다. 컴파일러가 사용 지점에서 실제 타입으로 특수화(monomorphization)합니다 — 런타임 오버헤드 없음.

---

## 메서드 스타일 함수

struct에 연관 함수를 붙이려면 `fn 타입이름.메서드이름()` 문법을 씁니다:

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

호출할 때는 타입 이름으로 명시적으로 호출합니다:

```kyte
@main(main) {
    Vec2 v = Vec2 { x: 3.0, y: 4.0 };
    float len = Vec2.length(v);  // v를 self로 넘김
    print(len);                  // 25.0 (제곱합)
}
```

> **참고:** `impl` + `trait`을 쓰면 더 체계적으로 타입에 메서드를 부여할 수 있습니다. 자세한 내용은 [Trait & Impl](traits-impl.md) 참고.

---

## 재귀

재귀 함수도 잘 동작합니다:

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

## 팁

- 기본 타입 매개변수(`int`, `float`, `bool`)는 값으로 전달됩니다.
- `string`과 struct는 포인터로 전달됩니다 (내부적으로).
- 반환 타입을 선언하면 컴파일러가 모든 코드 경로에서 값을 반환하는지 검사합니다.
- 함수는 최상위에서만 선언됩니다 — 함수 안에 함수는 없습니다. 대신 클로저를 쓰세요.
