# 제어 흐름

---

## if / else

```kyte
int score = 85;

if score >= 90 {
    print("A");
} else if score >= 80 {
    print("B");
} else {
    print("C 이하");
}
```

조건에 괄호가 **없습니다** — Kyte는 조건이 명확할 때 노이즈를 줄입니다. 중괄호는 한 줄짜리라도 **필수**입니다.

---

## for — 범위 루프

인덱스가 0부터 N-1까지 필요할 때:

```kyte
for i in 0..5 {
    print(i);
}
// 0 1 2 3 4
```

`0..5`는 0 이상 5 **미만** (Rust와 동일). 배열을 순회할 때:

```kyte
int[] nums = [10, 20, 30, 40, 50];
for i in 0..len(nums) {
    print(nums[i]);
}
```

역방향이 필요하면 while을 쓰세요:

```kyte
int i = 4;
while i >= 0 {
    print(i);
    i -= 1;
}
// 4 3 2 1 0
```

---

## while

조건이 참인 동안 반복합니다:

```kyte
int n = 1;
while n < 1000 {
    n *= 2;
}
print(n);  // 1024
```

조건에 `()`를 써도 되지만 없애는 게 더 깔끔합니다.

---

## loop

명시적으로 탈출할 때까지 무한히 반복합니다:

```kyte
int count = 0;
loop {
    count += 1;
    if count >= 5 { break; }
}
print(count);  // 5
```

`while true`보다 의도가 명확합니다 — "탈출 조건이 본문에 있음"을 선언하는 것입니다.

---

## break

가장 안쪽 루프에서 즉시 빠져나옵니다:

```kyte
for i in 0..100 {
    if i == 7 { break; }
    print(i);
}
// 0 1 2 3 4 5 6
```

중첩 루프에서 `break`는 **바로 안쪽 루프만** 종료합니다. 바깥 루프까지 탈출하려면 플래그 변수나 함수로 감싸세요.

---

## continue

현재 반복만 건너뛰고 다음 반복을 시작합니다:

```kyte
// 짝수만 출력
for i in 0..10 {
    if i % 2 != 0 { continue; }
    print(i);
}
// 0 2 4 6 8
```

```kyte
// 음수 건너뛰기
int[] vals = [3, -1, 7, -2, 5];
int sum = 0;
for i in 0..len(vals) {
    if vals[i] < 0 { continue; }
    sum = sum + vals[i];
}
print(sum);  // 15
```

`break`는 루프를 **끝내고**, `continue`는 루프를 **건너뜁니다**.

---

## match

패턴 매칭. 긴 `if/else if` 체인이 지저분해질 때 쓰세요:

### 정수 매칭

```kyte
int code = 404;

match code {
    200 => { print("OK"); }
    404 => { print("Not Found"); }
    500 => { print("Server Error"); }
    _   => { print(f"알 수 없는 코드: {code}"); }
}
```

`_`는 와일드카드입니다 — 앞선 패턴에 매칭되지 않은 모든 값을 처리합니다. 항상 마지막에 오세요.

### 문자열 매칭

```kyte
string cmd = "quit";

match cmd {
    "start" => { print("시작!"); }
    "stop"  => { print("중지!"); }
    "quit"  => { print("종료!"); }
    _       => { print("알 수 없는 명령"); }
}
```

### 열거형 매칭

enum과 함께 쓸 때 match가 가장 빛납니다:

```kyte
enum Direction { North, South, East, West }

Direction d = Direction.North;

match d {
    Direction.North => { print("북쪽으로 이동"); }
    Direction.South => { print("남쪽으로 이동"); }
    _               => { print("동쪽 또는 서쪽"); }
}
```

### 페이로드가 있는 열거형 매칭

```kyte
enum Result {
    Ok(int),
    Err,
}

Result r = Result.Ok(42);

match r {
    Result.Ok(n) => { print(f"성공: {n}"); }
    Result.Err   => { print("실패"); }
}
```

`n`은 패턴에서 페이로드에 바인딩된 변수입니다. 바인딩 이름은 자유롭게 지을 수 있습니다.

---

## assert

불변식을 검증합니다. 조건이 false면 런타임에서 즉시 중단합니다:

```kyte
assert(x > 0);
assert(len(arr) > 0, "배열이 비어있습니다");
```

디버깅용입니다. 에러 처리 대신 쓰지 마세요 — 실패하면 프로그램이 죽습니다. 사용자 입력 검증에는 if/Kill을 쓰세요.

---

## Exit

프로그램을 즉시 종료합니다. 정리도, 언와인딩도 없음:

```kyte
if config_missing {
    print("설정 파일이 없습니다.");
    Exit;
}
```

`exit(0)`와 동일합니다. 복구 불가능한 초기화 실패 등 정말 끝내야 할 때만 쓰세요.
