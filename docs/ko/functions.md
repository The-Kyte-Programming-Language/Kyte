# 함수

함수는 `fn`으로 선언합니다. 매개변수는 타입 명시, 반환 타입은 선택 (없으면 void).

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

호출은 예상대로:

```kyte
@main(main) {
    int result = add(3, 4);   // 7
    greet("Kyte");             // 안녕하세요, Kyte!
}
```

---

## 여러 매개변수

쉼표로 구분하고 각각 `타입 이름` 형식으로:

```kyte
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}
```

---

## void 함수

반환값 없으면 `->` 생략:

```kyte
fn log(string msg) {
    print(msg);
}
```

조기 종료에 `return;` 사용 가능:

```kyte
fn process(int x) {
    if x < 0 { return; }
    print(x);
}
```

---

## 클로저

클로저는 변수에 할당하는 익명 함수입니다. `|파라미터: 타입|` 문법을 씁니다:

```kyte
auto double = |n: int| { return n * 2; };
auto add = |a: int, b: int| { return a + b; };

int d = double(21);   // 42
int s = add(10, 5);   // 15
```

클로저는 캡처 없는 함수 포인터입니다 — 로컬 변수를 가두지 않습니다. 가벼운 이름 없는 함수라고 생각하세요.

---

## 제네릭

`<T>`로 타입에 대해 제네릭한 함수 작성:

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

## 메서드 스타일 함수

타입 이름을 접두사로 붙여 메서드처럼 정의:

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

## 조기 반환

`return`으로 어디서든 함수 종료:

```kyte
fn find(int[] arr, int target) -> int {
    for i in 0..10 {
        if arr[i] == target { return i; }
    }
    return -1;
}
```

---

## 팁

- 기본 타입 매개변수는 값으로 전달됩니다.
- 반환값이 있는 함수는 반환 타입 명시가 필수입니다.
- 재귀 함수도 잘 동작합니다.
