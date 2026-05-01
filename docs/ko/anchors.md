# Anchor

Anchor는 Kyte에서 가장 독특한 기능입니다. 단순한 함수가 아니라 **구조적 복원력**을 가진 진입점입니다.

보통 뭔가 잘못되면 두 가지 선택지가 있습니다: 예외를 던지거나 (try/catch 지옥) 아니면 그냥 죽거나. Anchor는 세 번째 선택지입니다 — **처음부터 다시 시작**.

---

## 첫 번째 Anchor

모든 Kyte 프로그램은 `@main(main)` anchor로 시작합니다:

```kyte
@main(main) {
    print("Hello, Kyte!");
}
```

`@` 접두사가 anchor임을 나타냅니다. `main`은 anchor의 **종류(kind)**입니다.

---

## Anchor 종류

| 문법 | 설명 |
|---|---|
| `@이름(main)` | 프로그램 진입점. 딱 하나. |
| `@이름(plain)` | 부모 anchor 안에서 독립적인 재시도 범위 |
| `@이름(thread)` | 별도 OS 스레드에서 실행 |
| `@이름(event(이름))` | `emit()`으로 트리거되는 이벤트 핸들러 |

---

## Kill — 재시작 트리거

`Kill`을 만나면 anchor가 처음부터 **재시작**합니다:

```kyte
@main(main) {
    Vault int attempts = 0;
    attempts += 1;

    print(attempts);

    if attempts < 3 {
        Kill "아직 준비 안 됨";
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

`Vault int attempts`가 핵심입니다. 재시작해도 힙에 있는 값은 유지됩니다. 일반 스택 변수(`int attempts`)를 쓰면 매번 0으로 리셋됩니다.

이것이 Kyte의 재시도 패턴입니다: **Vault로 카운터를 힙에 두고, Kill로 재시작.**

---

## 스택 변수 vs Vault 변수

이 차이가 anchor를 이해하는 핵심입니다:

```kyte
@main(main) {
    int stack_val = 0;        // 재시작 시 0으로 리셋
    Vault int heap_val = 0;   // 재시작을 넘어 유지

    stack_val += 1;
    heap_val += 1;

    print(f"스택: {stack_val}, 힙: {heap_val}");

    if heap_val < 3 {
        Kill "재시도";
    }
}
```

출력:
```
스택: 1, 힙: 1
스택: 1, 힙: 2
스택: 1, 힙: 3
```

`stack_val`은 항상 1입니다 — 매 재시작마다 `int stack_val = 0`으로 초기화됩니다. `heap_val`은 힙에 있어서 살아남습니다.

---

## catch — Kill 가로채기

기본적으로 Kill은 anchor를 재시작시킵니다. `catch` 블록을 추가하면 재시작 **전에** Kill을 가로챌 수 있습니다:

```kyte
@main(main) {
    Vault int tries = 0;
    tries += 1;

    if tries < 3 {
        Kill "아직 준비 안 됨";
    }

    print("정상 동작");
} catch (string reason) {
    print(f"Kill 발생: {reason}");
    // break 없음 → 계속 재시작
}
```

출력:
```
Kill 발생: 아직 준비 안 됨
Kill 발생: 아직 준비 안 됨
정상 동작
```

**`catch`에서 `break`** — anchor를 즉시 종료합니다 (재시작 안 함):

```kyte
@main(main) {
    do_something_risky();
} catch (string why) {
    print(f"실패: {why}");
    break;   // 여기서 멈춤
}
```

| `catch` 동작 | 결과 |
|---|---|
| `break` 있음 | anchor 종료 |
| `break` 없음 | anchor 재시작 |

---

## 중첩 Anchor

Anchor 안에 anchor를 넣을 수 있습니다. 내부 anchor의 재시작은 외부에 영향을 주지 않습니다:

```kyte
@main(main) {
    Vault int outer = 0;
    outer += 1;

    @inner(plain) {
        Vault int count = 0;
        count += 1;
        if count < 2 { Kill "내부 재시도"; }
        print(f"내부 완료: {count}번");
    }

    if outer < 2 { Kill "외부 재시도"; }
    print(f"외부 완료: {outer}번");
}
```

출력:
```
내부 완료: 2번
내부 완료: 2번
외부 완료: 2번
```

`@inner(plain)`이 재시작하는 동안 `outer`는 변하지 않습니다. 재시도 범위를 격리할 때 유용합니다.

---

## Thread Anchor

별도 OS 스레드에서 실행됩니다. 부모 anchor와 동시에 실행됩니다:

```kyte
@main(main) {
    @worker(thread) {
        Vault int count = 0;
        count += 1;
        print(f"워커: {count}번");
        if count < 3 { Kill "워커 재시도"; }
        print("워커 완료");
    }

    print("메인은 계속 실행됩니다");
}
```

워커 스레드가 Kill을 만나도 메인은 영향 받지 않습니다. CPU 집약적 작업이나 블로킹 I/O를 메인 흐름에서 분리할 때 씁니다.

---

## Event Anchor

이름으로 트리거되는 핸들러입니다:

```kyte
@main(main) {
    @on_error(event(error)) {
        print(f"[ERROR] {_payload}");
    }

    emit("error", "연결 실패");   // on_error 실행
    emit("error", "타임아웃");    // on_error 다시 실행
    print("이후 로직 계속");
}
```

출력:
```
[ERROR] 연결 실패
[ERROR] 타임아웃
이후 로직 계속
```

### emit()

```kyte
emit("이벤트명");                  // 페이로드 없이
emit("이벤트명", "페이로드 문자열");  // 문자열 페이로드와 함께
```

`emit()`은 **동기**입니다 — 핸들러가 모두 실행 완료될 때까지 다음 줄로 넘어가지 않습니다.

### _payload

이벤트 핸들러 안에서 `_payload`는 `emit()`에 넘긴 두 번째 인자입니다. 항상 `string`입니다. 페이로드 없이 emit하면 빈 문자열입니다.

### 같은 이벤트에 여러 핸들러

```kyte
@main(main) {
    @logger(event(error)) {
        print(f"[로그] {_payload}");
    }
    @notifier(event(error)) {
        print(f"[알림] {_payload}");
    }

    emit("error", "디스크 꽉 참");
}
```

출력:
```
[로그] 디스크 꽉 참
[알림] 디스크 꽉 참
```

등록 순서대로 모두 실행됩니다.

---

## 언제 어떤 Anchor를 쓰나요?

| 상황 | Anchor 종류 |
|---|---|
| 프로그램 진입점 | `main` |
| "이 부분만" 독립적으로 재시도하고 싶을 때 | `plain` (중첩) |
| CPU 집약 작업, 블로킹 I/O를 분리할 때 | `thread` |
| 분산된 핸들러가 이름으로 반응해야 할 때 | `event` |

---

## 알아두기

- `Kill`에 메시지 없이도 됩니다: `Kill;`
- Kill을 만나지 않는 anchor는 일반 함수처럼 한 번만 실행됩니다.
- `catch` 없이도 Kill은 재시작합니다. 단, 기본적으로 일정 횟수 초과 시 상위로 에스컬레이션합니다.
- Thread anchor 안에서 공유 상태(`Vault`)를 쓸 때는 동시성에 주의하세요.
