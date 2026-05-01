# 모듈

코드가 커지면 파일 하나에 전부 넣는 건 금방 한계가 옵니다. Kyte는 두 가지 방법으로 코드를 정리합니다: **인라인 `mod` 블록**과 **`import`** 구문.

---

## 인라인 `mod` — 네임스페이스 분리

한 파일 안에서 관련 함수들을 이름 아래로 묶습니다:

```kyte
mod math {
    fn add(int a, int b) -> int {
        return a + b;
    }

    fn square(int n) -> int {
        return n * n;
    }

    fn abs(int n) -> int {
        if n < 0 { return -n; }
        return n;
    }
}

@main(main) {
    print(math.add(3, 7));    // 10
    print(math.square(5));    // 25
    print(math.abs(-12));     // 12
}
```

`math.add()`처럼 `모듈명.함수명()` 형식으로 호출합니다. 이름 충돌을 막고, `math.`만 입력해도 LSP 자동완성이 나옵니다.

`mod`는 **함수만** 담습니다. 상태, 변수, struct 정의는 모듈 밖에 두세요.

---

## `import` — 파일 분리

파일이 길어지면 여러 `.ky` 파일로 분리합니다:

```kyte
// utils.ky
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}

fn sign(int n) -> int {
    if n > 0 { return 1; }
    if n < 0 { return -1; }
    return 0;
}
```

```kyte
// main.ky
import "utils.ky";

@main(main) {
    int x = clamp(150, 0, 100);   // 100
    int s = sign(-42);             // -1
    print(x);
    print(s);
}
```

임포트된 파일의 모든 최상위 선언(함수, struct, enum, const)이 현재 파일에서 그대로 사용 가능해집니다 — 접두사 없이.

임포트 경로는 현재 파일 위치 기준 상대 경로입니다.

---

## 함께 쓰기

두 방식을 함께 쓸 수 있습니다:

```kyte
import "math_utils.ky";   // 파일에서 최상위 함수들 가져오기

mod fmt {
    fn pad_left(string s, int width) -> string {
        // 구현...
    }
}

@main(main) {
    int result = some_math_fn(5);       // import에서
    string r = fmt.pad_left("hi", 10); // mod에서
}
```

---

## 언제 뭘 쓰나요?

| 상황 | 권장 방법 |
|---|---|
| 한 파일 안에서 함수를 그룹으로 묶고 싶을 때 | `mod` 블록 |
| 파일이 너무 길어서 분리가 필요할 때 | `import` |
| 라이브러리 코드를 재사용하고 싶을 때 | `import` |

---

## 알아두기

- 순환 임포트는 지원하지 않습니다. (`a.ky`가 `b.ky`를 import하고 `b.ky`가 다시 `a.ky`를 import하는 것)
- 하나의 파일을 여러 곳에서 import해도 한 번만 처리됩니다.
- LSP가 `mod` 안의 함수도 자동완성합니다 — `math.`까지 치면 목록이 나타납니다.
