# Kyte Black-Box Test Runner
# Compiles each .ky file in test/blackbox/, links with kyte_rt, runs it,
# and compares stdout against the expected output defined in this script.

$ErrorActionPreference = "Stop"
$CLANG = "C:\llvm\bin\clang.exe"
# Prefer release binary if available (debug may be locked by rust-analyzer)
if (Test-Path "target\release\kyte.exe") {
    $KYTE = "target\release\kyte.exe"
} else {
    $KYTE = "target\debug\kyte.exe"
}
$RT    = "kyte_rt\kyte_rt.o"
$BBDIR = "test\blackbox"
$TMPDIR = "test\blackbox\.build"

# Expected outputs — lines joined by newline.
# float values: print(float) uses %f format (6 decimal places)
$Expected = @{
    "01_arithmetic.ky"     = "7`n7`n12`n3`n1`n-5`n14`n20"
    "02_float.ky"          = "5.000000`n2.000000`n5.250000`n2.500000`n3.140000"
    "03_bool.ky"           = "true`nfalse`nfalse`ntrue`nfalse`ntrue`ntrue`ntrue`ntrue`ntrue`ntrue"
    "04_variables.ky"      = "30`n42`n50`n40`n80"
    "05_if_else.ky"        = "1`n-1`n0`n1`n0"
    "06_while.ky"          = "0`n1`n2`n3`n4`n55"
    "07_for.ky"            = "10`n1`n2`n3`n4`n5"
    "08_functions.ky"      = "7`n12`n20`n30`n7`n3"
    "09_recursion.ky"      = "1`n1`n120`n3628800`n0`n1`n55"
    "10_strings.ky"        = "hello`nworld`ntrue`nfalse`ntrue`nhello"
    "11_fstrings.ky"       = "x is 42`npi = 3.14`nflag = true`nsum of 10 and 20 is 30"
    "12_arrays.ky"         = "1`n5`n5`n99`n111`n2.200000`ntrue`nfalse"
    "13_cast.ky"           = "true`n3`n3`n1`ntrue`nfalse"
    "14_structs.ky"        = "3`n4`n50`n100"
    "15_closures.ky"       = "10`n16`n17"
    "16_break_continue.ky" = "10`n20`n3"
    "17_string_concat.ky"  = "hello world`nfoobar!"
    "18_constants.ky"      = "100`n3.140000`ntrue`nconstant`n101"
    "19_nested_loops.ky"   = "9`n14"
    "20_multiple_returns.ky" = "0`n1`n2`n3`n3"
    "21_pipe.ky"             = "10`n15`n5`n20"
}

# Setup
if (-not (Test-Path $RT)) {
    Write-Host "Compiling kyte_rt.c..." -ForegroundColor Cyan
    & $CLANG -O2 -c kyte_rt/kyte_rt.c -o $RT
    if ($LASTEXITCODE -ne 0) { Write-Error "Failed to compile kyte_rt.c"; exit 1 }
}
New-Item -ItemType Directory -Force -Path $TMPDIR | Out-Null

$passed = 0
$failed = 0
$errors = @()

$files = Get-ChildItem "$BBDIR\*.ky" | Sort-Object Name

foreach ($file in $files) {
    $name    = $file.Name
    $base    = [System.IO.Path]::GetFileNameWithoutExtension($name)
    $kyPath  = $file.FullName
    $objPath = "$BBDIR\$base.o"
    $exePath = "$TMPDIR\$base.exe"

    Write-Host "  [$name] " -NoNewline

    # 1. Compile .ky -> .o
    $compileOut = & $KYTE $kyPath --no-cache 2>&1
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $objPath)) {
        Write-Host "COMPILE FAIL" -ForegroundColor Red
        $msg = "${name} - compile failed: " + ($compileOut -join "; ")
        $errors += $msg
        $failed++
        continue
    }

    # 2. Link -> .exe
    $linkOut = & $CLANG $objPath $RT -o $exePath 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "LINK FAIL" -ForegroundColor Red
        $errors += "${name} - link failed: " + ($linkOut -join "; ")
        $failed++
        continue
    }

    # 3. Run and capture stdout
    $runOut = & $exePath 2>$null
    $actual = ($runOut -join "`n").TrimEnd()

    # 4. Compare against expected
    if ($Expected.ContainsKey($name)) {
        $exp = $Expected[$name]
        if ($actual -eq $exp) {
            Write-Host "PASS" -ForegroundColor Green
            $passed++
        } else {
            Write-Host "FAIL" -ForegroundColor Red
            $expDisplay = $exp -replace "`n", "\n"
            $actDisplay = $actual -replace "`n", "\n"
            $errors += "${name} - expected: $expDisplay | got: $actDisplay"
            $failed++
        }
    } else {
        Write-Host "RUN-OK (no expected)" -ForegroundColor Yellow
        $passed++
    }
}

Write-Host ""
Write-Host "  Results: $passed passed, $failed failed" -ForegroundColor White

if ($errors.Count -gt 0) {
    Write-Host ""
    Write-Host "  Failures:" -ForegroundColor Red
    foreach ($e in $errors) {
        Write-Host "    $e" -ForegroundColor Red
    }
}

if ($failed -gt 0) { exit 1 } else { exit 0 }
