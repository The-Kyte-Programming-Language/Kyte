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
| `@이름(thread)` | thread | 별도 OS 스레드에서 실행 |
| `@이름(event(error))` | event | `emit("error", ...)` 로 트리거됨 |

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

## catch — Kill 가로채기

기본적으로 `Kill`은 anchor를 최대 3번 재시작시키고 그 이후엔 상위로 에스컬레이션합니다. `catch` 블록을 쓰면 재시작 전에 Kill을 가로챌 수 있습니다:

```kyte
@main(main) {
    Vault int attempt = 0;
    attempt += 1;

    if attempt < 3 {
        Kill "아직 준비 안 됨";
    }

    print("정상");
} catch (string reason) {
    print(reason);
    // break 없음 → anchor 재시작
}
```

- **catch에 `break` 없으면** → anchor가 처음부터 재시작
- **catch에 `break` 있으면** → anchor 즉시 종료

```kyte
@main(main) {
    risky_call();
    Kill "중단";
} catch (string why) {
    print(why);
    break;   // 여기서 멈춤 — 재시작 안 함
}
```

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

## Thread Anchor

`@이름(thread)`는 감독받는 OS 스레드를 생성합니다. 본문은 부모 anchor와 동시에 실행됩니다:

```kyte
@main(main) {
    @worker(thread) {
        // 별도 스레드에서 실행
        Vault int count = 0;
        count += 1;
        print(count);
        if count < 5 {
            Kill "워커 재시도";
        }
    }

    print("부모는 여기서 계속");
}
```

Thread anchor는 자체 재시작 루프를 가집니다 — 스레드 본문이 `Kill`을 만나면 부모에게 영향 없이 스레드만 재시작됩니다.

---

## Event Anchor

`@이름(event(타입))`은 명명된 이벤트의 핸들러를 등록합니다. 프로그램 어디서든 `emit("타입", payload)`를 호출하면 핸들러가 실행됩니다:

```kyte
@main(main) {
    @on_error(event(error)) {
        print(_payload);   // emit()에서 전달된 암묵적 string 변수
    }

    emit("error", "연결 끊김");   // on_error 트리거
    emit("error", "타임아웃");    // on_error 다시 트리거
}
```

### emit()

```
emit("이벤트명")                  // 페이로드 없이 이벤트 발생
emit("이벤트명", "페이로드 문자열")  // 페이로드와 함께 이벤트 발생
```

`emit()`은 **동기(synchronous)**입니다 — 모든 매칭 핸들러가 실행 완료될 때까지 기다린 후 다음 코드로 넘어갑니다.

### _payload

이벤트 anchor 본문 안에서 `_payload`는 `emit()`에 전달된 페이로드를 담은 암묵적 `string` 변수입니다. 페이로드가 없으면 빈 문자열입니다.

```kyte
@main(main) {
    @on_alert(event(alert)) {
        print(f"알림: {_payload}");
    }

    emit("alert", "디스크 거의 꽉 참");
    // 출력: 알림: 디스크 거의 꽉 참
}
```

### 이벤트 anchor와 Kill

이벤트 핸들러도 `Kill`로 자기 자신을 재시작할 수 있습니다:

```kyte
@main(main) {
    @on_request(event(request)) {
        Vault int tries = 0;
        tries += 1;
        if tries < 3 {
            Kill "요청 재시도";
        }
        print(f"처리됨: {_payload}, {tries}번 시도");
    }

    emit("request", "fetch /api/data");
}
```

### 같은 이벤트에 여러 핸들러

같은 이벤트 이름으로 여러 핸들러를 등록할 수 있습니다 — 등록 순서대로 모두 실행됩니다:

```kyte
@main(main) {
    @logger(event(error)) {
        print(f"[로그] {_payload}");
    }
    @alerter(event(error)) {
        print(f"[알림] {_payload}");
    }

    emit("error", "디스크 꽉 참");
    // 출력:
    // [로그] 디스크 꽉 참
    // [알림] 디스크 꽉 참
}
```

---

## yield — Anchor에서 값 반환

Anchor는 `yield`로 값을 반환할 수 있습니다:

```kyte
@compute(plain) {
    int result = heavy_calculation();
    yield result;
}
```

---

## 언제 어떤 Anchor를 쓰나요?

| 종류 | 사용 시점 |
|---|---|
| `plain` | 부모 anchor 안에서 독립적인 재시도 범위가 필요할 때 |
| `thread` | CPU 집약적이거나 블로킹 작업을 동시에 실행할 때 |
| `event` | 이름으로 트리거되는 디커플된 핸들러가 필요할 때 |
| `main` | 프로그램 진입점 (항상 필요) |

전통적인 `try/catch`는 호출 지점에서 오류를 처리합니다. Anchor는 구조 수준에서 처리합니다 — 전체 anchor가 재시작되는데, 이게 대부분 원하는 동작입니다.

---

## 팁

- 메시지 없는 `Kill`: `Kill;` — 메시지는 선택사항.
- `Kill`을 만나지 않는 anchor는 일반 함수처럼 한 번만 실행됩니다.
- Thread anchor는 동시에 실행됩니다 — `Vault`로 공유 상태를 주의해서 다루세요.
- 이벤트 핸들러는 `emit()` 호출 시점에 동기적으로 실행됩니다.
- `_payload`는 이벤트 핸들러 안에서 항상 `string` 타입입니다; 필요하면 `as`로 캐스팅하세요.
