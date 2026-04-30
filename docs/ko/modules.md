# 모듈

모듈은 관련 함수를 이름 있는 네임스페이스로 묶습니다. 사용 방법은 두 가지: 인라인 `mod` 블록과 `import` 구문.

---

## 인라인 모듈

`mod`로 모듈을 정의하고 `모듈명.함수명()`으로 호출:

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
    int sum = math.add(3, 7);     // 10
    int sq  = math.square(5);     // 25
    int a   = math.abs(-12);      // 12
    print(sum);
}
```

모듈에는 함수만 들어갑니다. 상태도 없고 인스턴스도 없는 순수 네임스페이스입니다.

---

## 파일 임포트

`import`로 여러 `.ky` 파일로 코드를 분리:

```kyte
// helpers.ky
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}
```

```kyte
// main.ky
import "helpers.ky";

@main(main) {
    int x = clamp(150, 0, 100);   // 100
    print(x);
}
```

임포트는 현재 파일 디렉터리 기준으로 해석됩니다. 임포트된 파일의 최상위 선언이 직접 사용 가능해집니다 — 접두사 불필요.

---

## 조합 사용

```kyte
import "math_utils.ky";

mod string_utils {
    fn repeat(string s, int n) -> string {
        // ...
    }
}

@main(main) {
    int result = math_utils_fn(5);            // import에서
    string r = string_utils.repeat("hi", 3);  // mod에서
}
```

---

## 팁

- `mod`는 파일 내 논리적 그룹화에. `import`는 파일 분리에.
- 순환 임포트는 지원하지 않습니다.
- LSP가 모듈 함수 호출 자동완성을 지원합니다 — `math.`만 입력하면 목록이 나타납니다.
