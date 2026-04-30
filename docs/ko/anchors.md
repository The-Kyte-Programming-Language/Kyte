# Anchor

Anchor는 Kyte의 대표 기능입니다. 이벤트 기반 진입점으로, **실패 시 자동으로 재시작**할 수 있습니다. 스스로 회복하는 함수라고 생각하세요 — 뭔가 잘못되면 anchor가 처음부터 다시 시작합니다.

---

## 첫 번째 Anchor

모든 Kyte 프로그램은 `@main(main)` anchor로 시작합니다:

```kyte
@main(main) {
    print("Hello, Kyte!");
}
```

`@` 접두사가 anchor를 나타냅니다. `main`은 종류(kind)입니다.

---

## Anchor 종류

| 문법 | 종류 | 설명 |
|---|---|---|
| `@이름(main)` | main | 프로그램 기본 진입점 |
| `@이름(plain)` | plain | 단순 핸들러, 스레딩 없음 |
| `@이름(thread)` | thread | 별도 스레드에서 실행 |
| `@이름(event(error))` | event | 이벤트 타입으로 트리거됨 |

```kyte
@main(main) {
    print("진입점");
}

@worker(thread) {
    // 자체 스레드에서 실행
}

@error_handler(event(error)) {
    // 에러 이벤트 처리
}
```

---

## Kill — 재시작 트리거

`Kill`은 anchor에게 처음부터 재시작하라고 신호를 보냅니다:

```kyte
@main(main) {
    int attempt = 0;
    attempt += 1;

    print(attempt);

    if attempt < 3 {
        Kill "실패 시뮬레이션";   // 재시작!
    }

    print("3번 시도 후 안정");
}
```

출력:
```
1
2
3
3번 시도 후 안정
```

잠깐 — anchor가 재시작하는데 왜 `attempt`가 값을 기억하나요?

---

## Vault — 재시작을 넘나드는 영속 상태

일반 변수는 매 재시작마다 초깃값으로 돌아갑니다. `Vault` 변수는 힙에 살기 때문에 재시작에서 살아남습니다:

```kyte
@main(main) {
    Vault int attempt = 0;   // 힙 할당, Kill에서 살아남음
    attempt += 1;

    print(attempt);

    if attempt < 3 {
        Kill "재시도";
    }

    print("완료!");
}
```

출력:
```
1
2
3
완료!
```

이것이 고전적인 재시도 패턴입니다. 스택 변수는 리셋; Vault 변수는 기억.

---

## 중첩 Anchor

Anchor 안에 Anchor를 넣을 수 있습니다:

```kyte
@main(main) {
    Vault int outer = 0;
    outer += 1;

    @retry(plain) {
        Vault int inner = 0;
        inner += 1;
        if inner < 2 { Kill "내부 재시도"; }
        print(f"내부 완료: {inner}");
    }

    if outer < 2 { Kill "외부 재시도"; }
    print(f"외부 완료: {outer}");
}
```

각 anchor는 자체 재시작 범위를 가집니다. 중첩된 anchor의 재시작은 부모 anchor를 재시작시키지 않습니다.

---

## yield — Anchor에서 값 반환

Anchor는 `yield`로 값을 반환할 수 있습니다:

```kyte
@compute(thread) {
    int result = heavy_calculation();
    yield result;
}
```

---

## 언제 Anchor를 쓰나요?

Anchor가 유용한 상황:

- **재시도 로직** — 연결 실패, 일시적 오류
- **상태 머신** — 매 재시작마다 상태가 진행
- **이벤트 핸들러** — 폴링 없이 시스템 이벤트에 반응
- **장기 실행 워커** — 한 번 실패해도 계속 실행

전통적인 `try/catch`는 호출 지점에서 오류를 처리합니다. Anchor는 구조 수준에서 처리합니다 — 전체 anchor가 재시작되는데, 이게 대부분 원하는 동작입니다.

---

## 팁

- 메시지 없는 `Kill`: `Kill;` — 메시지는 선택사항.
- `Kill`을 만나지 않는 anchor는 일반 함수처럼 한 번만 실행됩니다.
- 안정 상태에 절대 도달하지 못하면 무한 재시작이 발생합니다 — Vault 카운터로 보호막을 만드세요.
