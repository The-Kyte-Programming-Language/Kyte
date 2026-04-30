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

조건에 괄호 불필요. 중괄호는 필수.

---

## for — 범위 루프

```kyte
for i in 0..5 {
    print(i);   // 0 1 2 3 4
}
```

`0..5` 범위는 오른쪽이 배타적 (Rust와 동일).

역방향은 while 루프로:

```kyte
int i = 5;
while i > 0 {
    i -= 1;
    print(i);   // 4 3 2 1 0
}
```

---

## while

```kyte
int n = 1;
while n < 100 {
    n *= 2;
}
print(n);   // 128
```

---

## loop

무한 루프. `break` 또는 `return`으로 탈출:

```kyte
int count = 0;
loop {
    count += 1;
    if count >= 5 { break; }
}
print(count);   // 5
```

---

## break

가장 안쪽 루프 탈출:

```kyte
for i in 0..100 {
    if i == 7 { break; }
    print(i);
}
```

---

## match

패턴 매칭. 긴 if/else 체인의 깔끔한 대안.

### 정수 매칭

```kyte
int x = 2;
match x {
    1 => { print("하나"); }
    2 => { print("둘"); }
    3 => { print("셋"); }
    _ => { print("기타"); }   // 와일드카드 — 나머지 모두 처리
}
```

### 열거형 매칭

```kyte
enum Direction { North, South, East, West }

Direction d = Direction.North;

match d {
    Direction.North => { print("북쪽으로"); }
    Direction.South => { print("남쪽으로"); }
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
    Result.Ok(n) => { print(n); }   // 42 출력
    Result.Err   => { print("오류"); }
}
```

`_` 와일드카드는 반드시 마지막에. 모든 매치 암 본문은 `{ }` 블록.

---

## assert

런타임 검증 — 조건이 false면 패닉:

```kyte
int x = 42;
assert(x > 0);       // 통과
assert(x == 0);      // 런타임에서 패닉
```

불변식 체크와 디버깅용. 에러 처리 대체재 아님.

---

## yield

Anchor에서 값 반환 (자세한 내용은 [Anchor](anchors.md) 참고):

```kyte
@worker(thread) {
    int result = compute();
    yield result;
}
```

---

## Exit

프로그램 즉시 종료:

```kyte
if fatal_error {
    Exit;
}
```

`exit(0)` 같은 것 — 정리도, 언와인딩도 없이 그냥 끝.
