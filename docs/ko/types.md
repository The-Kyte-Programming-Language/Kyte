# 타입 & 변수

Kyte는 정적 타입 언어입니다. 모든 값에 타입이 있고, 컴파일러가 타입 불일치를 잡아냅니다. 하지만 `auto`가 있어서 명백한 경우에는 타입을 직접 안 써도 됩니다.

---

## 기본 타입

| 타입 | 설명 | 리터럴 예시 |
|---|---|---|
| `int` | 64비트 부호 정수 | `42`, `-7`, `0` |
| `float` | 64비트 부동소수점 | `3.14`, `-0.5`, `1.0` |
| `string` | UTF-8 문자열 | `"hello"`, `""` |
| `bool` | 참/거짓 | `true`, `false` |

```kyte
int score = 100;
float ratio = 0.75;
string label = "Kyte";
bool done = false;
```

---

## 고정 크기 정수

`int`는 64비트라 대부분의 경우에 적합합니다. 메모리 레이아웃이 중요하거나 C FFI를 다루는 경우 정확한 크기를 지정하세요:

| 부호 있음 | 부호 없음 | 범위 (부호 있음 기준) |
|---|---|---|
| `i8` | `u8` | -128 ~ 127 |
| `i16` | `u16` | -32,768 ~ 32,767 |
| `i32` | `u32` | -2억 ~ 2억 |
| `i64` | `u64` | int와 동일 |

```kyte
u8 byte_val = 255;
i32 pixel_x = -400;
```

---

## 타입 추론 `auto`

타입이 우변에서 명확하면 `auto`를 쓰세요:

```kyte
auto x = 42;         // int
auto pi = 3.14;      // float
auto name = "kyte";  // string
auto ok = true;      // bool
```

`auto`가 왜 필요한가요? 나중에 타입을 바꿀 때 한 군데만 수정하면 됩니다. 또, 긴 타입 이름 (struct, enum 등)을 반복하지 않아도 됩니다.

**`auto`가 안 되는 곳:** 함수 매개변수, struct 필드, 반환 타입. 거기엔 명시적 타입이 필요합니다.

---

## 타입 캐스팅 `as`

`as`로 타입을 명시적으로 변환합니다:

```kyte
int x = 42;
float y = x as float;   // 42.0

float f = 3.99;
int i = f as int;       // 3 (소수점 버림, 반올림 아님)

bool b = true;
int n = b as int;       // 1
```

캐스팅은 명시적이어야 합니다. Kyte는 숫자 타입을 조용히 자동 변환하지 않습니다. `int + float`를 하면 컴파일 에러가 납니다 — 의도한 타입을 `as`로 명확히 하세요.

---

## 배열

```kyte
int[] nums = [1, 2, 3, 4, 5];
string[] tags = ["kyte", "fast", "native"];
float[] coords = [1.0, 2.5, -0.3];
```

인덱스 접근과 변경:

```kyte
int first = nums[0];    // 1
nums[2] = 99;           // [1, 2, 99, 4, 5]
int count = len(nums);  // 5
```

범위를 벗어난 인덱스는 런타임에서 잡아냅니다. 조용히 쓰레기 값을 읽는 대신 명확한 오류를 냅니다.

**배열을 for로 순회할 때는 `len()`과 인덱스를 조합하세요:**

```kyte
for i in 0..len(nums) {
    print(nums[i]);
}
```

---

## 문자열 & F-string

일반 문자열 리터럴:

```kyte
string greeting = "Hello, World!";
string empty = "";
```

**F-string** — 표현식을 `{}`로 직접 삽입합니다:

```kyte
string name = "Kyte";
int version = 2;
string msg = f"버전 {version}에 오신 것을 환영합니다, {name}!";
print(msg);
// 버전 2에 오신 것을 환영합니다, Kyte!
```

F-string 안에는 변수뿐 아니라 일반 표현식도 쓸 수 있습니다:

```kyte
int a = 3;
int b = 4;
print(f"{a} + {b} = {a + b}");
// 3 + 4 = 7
```

F-string에서 float는 불필요한 후행 0을 제거해서 출력합니다 (`3.14`). `print(float_val)` 직접 출력은 6자리 고정 (`3.140000`).

이스케이프 시퀀스: `\n`, `\t`, `\r`, `\\`, `\"`, `\0`.

---

## Vault — 힙 할당

기본적으로 모든 변수는 **스택에 살고**, 범위를 벗어나면 자동으로 사라집니다. 두 가지 상황에서 힙이 필요합니다:

1. **Anchor 재시작을 넘어서 상태를 유지해야 할 때**
2. **스택에 담기에 너무 큰 데이터 (대용량 배열 등)**

그럴 때 `Vault`를 앞에 붙입니다:

```kyte
Vault int counter = 0;
Vault int[] buffer = [1, 2, 3, 4, 5];
```

Vault 변수는 **컴파일러가 자동으로 해제**합니다 — `free()`를 직접 쓸 필요 없습니다. 컴파일러가 마지막 사용 지점을 분석해서 거기 바로 뒤에 해제 코드를 삽입합니다.

자세한 내용은 [메모리](memory.md) 참고.

---

## 상수

파일 최상위에 선언하는 변경 불가 값입니다. 모든 곳에서 접근 가능합니다:

```kyte
const int MAX_RETRIES = 5;
const float PI = 3.14159;
const string APP_NAME = "MyApp";
const bool DEBUG = false;
```

상수는 왜 쓰나요? 매직 넘버를 없애기 위해서입니다. 코드 여러 곳에서 `3`이나 `100`이 등장하면 그게 무슨 의미인지 알 수 없습니다. 상수로 이름을 붙이면 의도가 명확해지고, 값을 바꿀 때 한 곳만 수정하면 됩니다.

상수에는 타입을 반드시 명시해야 합니다 — `auto`는 안 됩니다.
