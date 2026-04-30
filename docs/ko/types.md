# 타입 & 변수

Kyte는 정적 타입 언어입니다. 모든 변수에는 타입이 있고, 컴파일러가 검사합니다. 하지만 `auto` 덕분에 항상 직접 타입을 쓸 필요는 없습니다.

---

## 기본 타입

| 타입 | 설명 | 예시 |
|---|---|---|
| `int` | 64비트 부호 정수 | `int x = 42;` |
| `float` | 64비트 부동소수점 | `float pi = 3.14;` |
| `string` | UTF-8 문자열 | `string s = "hello";` |
| `bool` | 불리언 | `bool flag = true;` |

### 고정 크기 정수

비트 연산이나 외부 인터페이스가 필요할 때:

| 부호 있음 | 부호 없음 |
|---|---|
| `i8` | `u8` |
| `i16` | `u16` |
| `i32` | `u32` |
| `i64` | `u64` |

```kyte
i32 counter = 0;
u8 byte = 255;
```

---

## 타입 추론 `auto`

타입 쓰기 귀찮으면 Kyte한테 맡기세요:

```kyte
auto x = 42;          // int로 추론
auto pi = 3.14;       // float로 추론
auto name = "kyte";   // string으로 추론
auto ok = true;       // bool로 추론
```

`auto`는 우변에서 타입이 명확할 때만 씁니다. 함수 매개변수나 struct 필드에는 명시적 타입이 필요합니다.

---

## 타입 캐스팅 `as`

```kyte
int x = 42;
float y = x as float;   // 42.0

float f = 3.99;
int i = f as int;        // 3  (소수점 버림)
```

---

## 배열

```kyte
int[] nums = [1, 2, 3, 4, 5];
string[] names = ["alice", "bob"];
```

인덱스로 접근:

```kyte
int first = nums[0];
nums[1] = 99;
```

---

## 문자열 & F-string

일반 문자열은 그냥 `"..."`:

```kyte
string greeting = "Hello, World!";
```

F-string은 표현식을 인라인으로 삽입 (malloc 없이 스택 할당):

```kyte
string name = "Kyte";
int version = 1;
string msg = f"Kyte v{version}에 오신 걸 환영합니다!";
print(msg);  // Kyte v1에 오신 걸 환영합니다!
```

이스케이프 시퀀스: `\n`, `\t`, `\r`, `\\`, `\"`, `\0`, `\xHH` (16진수), `\uHHHH` (유니코드).

---

## Vault — 힙 할당

기본적으로 모든 변수는 스택에 살아있습니다. 힙이 필요하면 `Vault`를 앞에 붙이세요:

```kyte
Vault int x = 42;            // 힙에 할당된 int
Vault int[] arr = [1, 2, 3]; // 힙에 할당된 배열

free(x);    // 수동으로 해제
free(arr);
```

전체 내용은 [메모리](memory.md)를 참고하세요.

---

## 상수

파일 최상위에 선언하는 변경 불가 값:

```kyte
const int MAX_RETRIES = 3;
const string APP_NAME = "Kyte";
```

상수는 타입을 명시해야 합니다. 파일 어디서든 사용 가능합니다.
