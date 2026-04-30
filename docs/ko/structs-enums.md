# Struct & Enum

---

## Struct

struct는 관련 필드를 묶습니다. 필드는 명시적 타입, `;`으로 끝냅니다.

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

### 인스턴스 생성

```kyte
Point p = Point { x: 1.0, y: 2.5 };
User u = User { name: "앨리스", age: 30, active: true };
```

### 필드 접근

```kyte
print(p.x);        // 1.0
print(u.name);     // 앨리스
```

### 필드 변경

```kyte
u.age = 31;
p.x += 0.5;
```

---

## Enum

Enum은 고정된 변형 집합을 가진 타입을 정의합니다. 변형에는 선택적으로 값을 담을 수 있습니다.

### 단순 열거형

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

### 페이로드가 있는 열거형

변형에 값 하나를 담을 수 있습니다:

```kyte
enum Option {
    Some(int),
    None,
}

enum Shape {
    Circle(float),    // 반지름
    Square(float),    // 변의 길이
}
```

```kyte
Option val = Option.Some(42);
Shape s = Shape.Circle(3.14);
```

### match에서 enum 사용

enum이 빛나는 순간:

```kyte
Option result = Option.Some(99);

match result {
    Option.Some(n) => { print(n); }   // 99 출력
    Option.None    => { print("없음"); }
}
```

페이로드 변형에서 내부 값은 패턴의 식별자에 바인딩됩니다 — 여기서는 `n`.

---

## 조합 예제

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
        Color.Red   => { print("빨간 픽셀"); }
        Color.Green => { print("초록 픽셀"); }
        Color.Blue  => { print("파란 픽셀"); }
    }
}
```

---

## 팁

- Struct 초기화 시 필드 순서는 중요합니다 — 항상 필드명을 명시하세요 (`{ x: 1.0, y: 2.5 }`).
- Enum 자체에는 메서드가 없습니다 — `impl`과 조합하면 됩니다 ([Trait & Impl](traits-impl.md) 참고).
- 페이로드 변형은 값 하나만 담을 수 있습니다. 여러 필드가 필요하면 struct를 사용하세요.
