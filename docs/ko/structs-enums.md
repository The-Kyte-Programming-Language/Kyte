# Struct & Enum

커스텀 타입을 만드는 두 가지 방법. Struct는 **연관된 데이터를 묶을 때**, Enum은 **여러 상태 중 하나를 표현할 때**.

---

## Struct

관련 필드를 하나의 타입으로 묶습니다:

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

필드마다 타입을 명시하고 `;`으로 끝냅니다.

### 인스턴스 생성

```kyte
Point origin = Point { x: 0.0, y: 0.0 };
User alice = User { name: "앨리스", age: 30, active: true };
```

필드명을 반드시 명시해야 합니다. 순서만 맞추고 이름을 생략하는 건 안 됩니다 — 필드가 많아지면 어느 값이 어느 필드인지 헷갈리는 버그를 막기 위해서입니다.

### 필드 접근 & 변경

```kyte
print(alice.name);    // 앨리스
print(alice.age);     // 30

alice.age = 31;
origin.x += 1.0;
```

---

## Enum

정해진 변형 중 하나를 갖는 타입입니다:

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

왜 bool 대신 enum을 쓰나요? `bool is_north`보다 `Direction.North`가 더 명확합니다. 나중에 변형이 추가될 때도 (`Northeast` 등) 기존 코드를 망가뜨리지 않습니다.

### 페이로드가 있는 열거형

변형에 값을 하나 담을 수 있습니다:

```kyte
enum Shape {
    Circle(float),   // 반지름
    Rect(float),     // 넓이 (간단한 예시)
}

enum Event {
    Click(int),      // 클릭된 요소 ID
    Resize,          // 크기 변경 (페이로드 없음)
    Quit,
}
```

```kyte
Shape s = Shape.Circle(3.14);
Event e = Event.Click(42);
```

페이로드를 꺼낼 때는 `match`를 씁니다:

```kyte
match s {
    Shape.Circle(r) => { print(f"원, 반지름 {r}"); }
    Shape.Rect(a)   => { print(f"직사각형, 넓이 {a}"); }
}
```

`r`은 패턴 바인딩입니다 — 이름은 자유롭게 지을 수 있습니다.

---

## Struct + Enum 조합

실제로 가장 많이 쓰이는 패턴입니다:

```kyte
enum Status {
    Active,
    Banned(string),  // 사유
    Pending,
}

struct User {
    string name;
    int age;
    Status status;
}

@main(main) {
    User bob = User {
        name: "밥",
        age: 25,
        status: Status.Banned("스팸"),
    };

    match bob.status {
        Status.Active      => { print(f"{bob.name}: 활성"); }
        Status.Banned(why) => { print(f"{bob.name}: 차단됨 — {why}"); }
        Status.Pending     => { print(f"{bob.name}: 대기 중"); }
    }
}
```

출력:
```
밥: 차단됨 — 스팸
```

---

## match에서 구조체 구조 분해

struct를 match arm에서 직접 분해할 수 있습니다:

```kyte
match point {
    Point { x, y } when x > 0 => { print(f"오른쪽: {x}, {y}"); }
    Point { x, y }             => { print(f"왼쪽: {x}, {y}"); }
}
```

필요한 필드만 꺼낼 수도 있습니다 — 나머지는 무시됩니다:

```kyte
match user {
    User { name, status: Status.Active }       => { print(f"활성: {name}"); }
    User { name, status: Status.Banned(code) } => { print(f"차단: {name} ({code})"); }
}
```

전체 문법(중첩 열거형 패턴, `when` 가드 포함)은 [제어 흐름](control-flow.md)의 **구조체 패턴 구조 분해** 섹션을 참고하세요.

---

## 알아두면 좋은 점

- 페이로드 변형은 값을 **하나**만 담습니다. 여러 필드가 필요하면 struct를 만들어서 담으세요.
- Struct 필드는 **기본값이 없습니다** — 인스턴스 생성 시 모든 필드를 채워야 합니다.
- Enum에 직접 메서드를 붙이려면 `impl` + `trait`을 사용합니다 ([Trait & Impl](traits-impl.md) 참고).
