# Kyte

> 빠르고 표현력 있는 시스템 언어. 개발자 방해 안 함.

Kyte는 LLVM을 통해 네이티브 코드로 컴파일되는 정적 타입 언어입니다. Rust 수준의 성능을 원하지만 borrow checker와 씨름하기 싫은 개발자를 위한 언어 — 그리고 **Anchor** 시스템 덕분에 뭔가 잘못돼도 프로그램이 *계속 돌아갑니다*.

---

## 한 눈에 보기

```kyte
// 기본 struct + 루프
struct Counter {
    int value;
}

@main(main) {
    Counter c = Counter { value: 0 };

    while c.value < 5 {
        print(c.value);
        c.value += 1;
    }
}
```

출력:
```
0
1
2
3
4
```

---

## Kyte를 쓰는 이유

| 기능 | 당신에게 주는 것 |
|---|---|
| **Anchor** | 실패 시 자동으로 재시작하는 이벤트 기반 진입점. 크래시 없이 계속 실행. |
| **Vault** | 옵트인 힙 할당 — 스택이냐 힙이냐, 당신이 결정. |
| **LLVM 백엔드** | 네이티브 머신 코드로 컴파일. 빠름. |
| **LSP 지원** | 설치 즉시 풀스펙 IDE 경험. |
| **단순한 문법** | C, Rust, Go 경험이 있다면 10분 안에 익숙해짐. |

---

## 문서 구성

| 섹션 | 내용 |
|---|---|
| [타입 & 변수](types.md) | int, float, string, bool, auto, Vault 등 |
| [함수](functions.md) | fn, 클로저, 제네릭 |
| [제어 흐름](control-flow.md) | if/else, for, while, loop, match |
| [Struct & Enum](structs-enums.md) | 커스텀 타입과 페이로드 |
| [Trait & Impl](traits-impl.md) | Kyte식 다형성 |
| [모듈](modules.md) | mod 블록과 import |
| [Anchor](anchors.md) | 핵심 기능 — 복원력 있는 진입점 |
| [메모리](memory.md) | Vault, free, 그리고 메모리 할당 |

---

## 설치

```sh
# (곧 출시 예정 — GitHub 릴리즈 페이지 확인)
kyte --version
```

---

## Hello, World

```kyte
@main(main) {
    print("Hello, World!");
}
```

네. 그게 다입니다.
