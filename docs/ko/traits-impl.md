# Trait & Impl

Trait은 계약입니다 — 타입이 구현해야 하는 함수 집합을 정의합니다. `impl`이 타입과 trait을 연결합니다.

---

## Trait 정의

```kyte
trait Greet {
    fn greet(string name) -> string;
}

trait Drawable {
    fn draw();
    fn area() -> float;
}
```

Trait 본문에는 함수 시그니처만 들어갑니다 — 구현은 없습니다.

---

## Trait 구현

```kyte
struct Dog {
    string name;
}

impl Greet for Dog {
    fn greet(string name) -> string {
        return f"왈! 나는 {name}야!";
    }
}
```

이제 `Dog`가 `Greet` 계약을 만족합니다. Trait에 있는 모든 함수를 구현해야 합니다.

---

## 전체 예제

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
        print(f"반지름 {Circle.radius}인 원");
    }
}

impl Shape for Rect {
    fn area() -> float {
        return Rect.width * Rect.height;
    }
    fn describe() {
        print(f"직사각형 {Rect.width}x{Rect.height}");
    }
}
```

---

## Trait 메서드 호출

타입 한정 이름으로 메서드 호출:

```kyte
@main(main) {
    Circle c = Circle { radius: 5.0 };
    float a = Circle.area();
    Circle.describe();
}
```

---

## 팁

- 하나의 타입이 여러 trait을 구현할 수 있습니다 — `impl` 블록을 여러 개 쓰면 됩니다.
- Trait은 서로 다른 struct 타입에 일관된 API를 강제하는 좋은 방법입니다.
- Rust의 `dyn Trait` 같은 동적 디스패치는 현재 지원하지 않습니다 — trait 호출은 컴파일 시점에 해석됩니다.
