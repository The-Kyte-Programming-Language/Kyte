# Trait & Impl

Trait은 "이 타입이 무엇을 할 수 있는지"를 정의하는 계약입니다. `impl`이 그 계약을 실제로 이행합니다.

여러 struct가 같은 인터페이스를 공유해야 할 때 쓰세요. 예: `Circle`과 `Rect` 둘 다 `area()`를 가져야 한다면.

---

## Trait 정의

함수 시그니처만 선언합니다 — 구현 없음:

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

## Impl — 타입에 Trait 구현하기

`impl 트레잇 for 타입` 블록 안에 실제 구현을 작성합니다. 메서드 본문에서는 타입 이름으로 필드에 접근합니다:

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
        print(f"원, 반지름 = {Circle.radius}");
    }
}

impl Shape for Rect {
    fn area() -> float {
        return Rect.width * Rect.height;
    }
    fn describe() {
        print(f"직사각형, {Rect.width} x {Rect.height}");
    }
}
```

`Circle.radius`처럼 **타입 이름.필드명**으로 현재 인스턴스의 필드에 접근합니다.

---

## 메서드 호출

인스턴스를 만들고 `타입이름.메서드이름(인스턴스)` 형식으로 호출합니다:

```kyte
@main(main) {
    Circle c = Circle { radius: 5.0 };
    Rect r = Rect { width: 4.0, height: 3.0 };

    float ca = Circle.area();   // Circle의 현재 컨텍스트로 호출
    float ra = Rect.area();     // Rect의 현재 컨텍스트로 호출

    Circle.describe();   // 원, 반지름 = 5.0
    Rect.describe();     // 직사각형, 4.0 x 3.0
}
```

---

## 하나의 타입에 여러 Trait

하나의 struct가 여러 trait을 구현할 수 있습니다:

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

## Trait이 필요한 이유

Trait 없이도 `fn Circle.area()`, `fn Rect.area()`를 따로 만들 수 있습니다. Trait이 추가로 제공하는 것:

- **강제성** — Trait을 `impl`하면 선언된 모든 함수를 구현했는지 컴파일러가 검사합니다.
- **명시적 계약** — "이 타입은 Shape처럼 동작한다"는 걸 코드로 표현합니다.
- **일관된 API** — 여러 타입에 걸쳐 같은 이름/시그니처를 보장합니다.

---

## 현재 제한

- 동적 디스패치 (`dyn Trait` 스타일)는 지원하지 않습니다 — 모든 trait 호출은 컴파일 시점에 정해집니다.
- Trait끼리 상속은 없습니다.
- 기본 메서드 구현(default impl)은 없습니다 — 모든 메서드를 직접 구현해야 합니다.
