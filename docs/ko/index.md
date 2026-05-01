# Kyte

> 빠르고, 실용적이며, 크래시하지 않는 언어.

Kyte는 LLVM 네이티브 코드로 컴파일되는 정적 타입 시스템 언어입니다. C나 Rust 수준의 성능이 필요한데 borrow checker는 싫고, 예외 처리 코드로 함수를 도배하기도 싫을 때 — 그 자리를 Kyte가 채웁니다.

Kyte의 핵심 아이디어는 **Anchor**입니다. 뭔가 잘못돼도 프로그램이 *그냥 살아남습니다*. 재시작 로직이 언어 수준에서 빌트인.

---

## 맛보기

```kyte
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

```
0
1
2
3
4
```

`@main(main)` — 이게 Kyte 프로그램의 진입점입니다. `main`은 anchor 종류입니다. 왜 그냥 `main()` 함수가 아니냐고요? Anchor는 단순한 함수가 아니기 때문입니다. 재시작할 수 있고, 이벤트를 받을 수 있고, 자체 에러 복구 로직을 가집니다.

---

## 왜 Kyte인가요?

### 크래시 대신 재시도

서버 코드를 짤 때 가장 짜증나는 것 중 하나: 처리 중에 뭔가 잘못되면 전체 프로세스가 죽어버리는 것. Kyte의 Anchor는 이걸 언어 수준에서 해결합니다.

```kyte
@main(main) {
    Vault int attempts = 0;
    attempts += 1;

    // 데이터베이스 연결이 가끔 실패한다고 가정
    if attempts < 3 {
        Kill "연결 실패 — 재시도";
    }

    print(f"{attempts}번 만에 성공");
}
```

```
1
2
3번 만에 성공
```

`Kill`을 만나면 anchor가 처음부터 재시작합니다. `Vault`로 선언된 변수는 재시작을 넘어서 살아남습니다. try/catch를 6단계로 쌓는 대신, 이 두 가지만 알면 됩니다.

### 메모리 걱정 없는 힙 할당

Kyte는 어디서 해제할지 컴파일러가 직접 계산합니다. 마지막으로 사용되는 시점을 추적해서 그 바로 다음에 자동으로 `free`를 삽입합니다.

```kyte
@main(main) {
    Vault int[] buffer = [1, 2, 3, 4, 5];
    print(buffer[0]);  // 마지막 사용
    // 컴파일러가 여기서 자동 해제
    print("끝");
}
```

### 예측 가능한 성능

GC 없습니다. 런타임 없습니다. LLVM이 직접 네이티브 코드로. 컴파일 결과물은 C로 짠 것과 동등합니다.

---

## 문서 구성

| 섹션 | 배울 것 |
|---|---|
| [타입 & 변수](types.md) | int, float, string, auto 추론, 배열, 상수 |
| [함수](functions.md) | fn, 클로저, 제네릭 |
| [제어 흐름](control-flow.md) | if/else, for, while, loop, match, break, continue |
| [Struct & Enum](structs-enums.md) | 커스텀 타입, 페이로드 enum |
| [Trait & Impl](traits-impl.md) | 다형성, 타입별 메서드 |
| [모듈](modules.md) | mod 블록, import |
| [Anchor](anchors.md) | ← 여기서 Kyte다워집니다 |
| [메모리](memory.md) | Vault, 자동 해제, 언제 힙을 쓸지 |

---

## Hello, World

```kyte
@main(main) {
    print("Hello, World!");
}
```

네. 그게 다입니다.

---

## 설치

```sh
# 곧 출시 예정 — GitHub 릴리즈 페이지 확인
kyte --version
```
