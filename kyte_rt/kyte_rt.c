/*
 * kyte_rt.c — Kyte Language Runtime (cross-platform)
 *
 * Supported platforms
 *   Linux (x86-64, aarch64)     — Ubuntu, Debian, and any glibc distro
 *   macOS (x86-64, Apple Silicon / aarch64)
 *   Windows (x86-64, MSVC)
 *
 * Responsibilities
 *  1. Signal-based fault recovery
 *       POSIX : SIGFPE, SIGILL, SIGBUS, SIGABRT + SIGSEGV (via sigaltstack)
 *       Windows: SIGFPE, SIGILL
 *  2. Per-thread alternate signal stack (POSIX only)
 *       Enables safe SIGSEGV recovery even when the main stack is corrupted.
 *  3. Per-thread anchor setjmp/longjmp supervision stack
 *  4. Thread-based supervised anchor spawning (@anchor(thread))
 *  5. Logging helpers for Kill / recovery events
 */

/* ── Platform detection ─────────────────────────────────────────────────────── */
#if defined(_WIN32) || defined(_WIN64)
#  define KYTE_WINDOWS 1
#else
#  define KYTE_POSIX 1
#endif

/* ── Compiler-portable thread-local storage ─────────────────────────────────── */
#if defined(_MSC_VER)
#  define KYTE_TLS __declspec(thread)
#elif defined(__GNUC__) || defined(__clang__)
#  define KYTE_TLS __thread
#else
#  define KYTE_TLS  /* fallback: no TLS — single-thread only */
#endif

/* ── Compiler-portable noinline ─────────────────────────────────────────────── */
#if defined(_MSC_VER)
#  define KYTE_NOINLINE __declspec(noinline)
#elif defined(__GNUC__) || defined(__clang__)
#  define KYTE_NOINLINE __attribute__((noinline))
#else
#  define KYTE_NOINLINE
#endif

/* ── Includes ───────────────────────────────────────────────────────────────── */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <setjmp.h>

#if defined(KYTE_POSIX)
#  include <unistd.h>
#  include <pthread.h>
#  include <sys/types.h>
#endif

#if defined(KYTE_WINDOWS)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <process.h>  /* _beginthreadex */
#endif

/* ── Hard-exit helper (async-signal-safe, avoids stdio flush in signals) ─────── */
static void kyte_hard_exit(int code) {
#if defined(KYTE_WINDOWS)
    ExitProcess((UINT)code);
#else
    _exit(code);
#endif
}

/* ── Recovery stack ─────────────────────────────────────────────────────────── */
#define KYTE_MAX_DEPTH 128

typedef struct {
    jmp_buf      buf;      /* setjmp save point      */
    volatile int in_use;   /* 1 while slot is live   */
} KyteAnchorSlot;

/* Per-thread anchor stack (O(1) push / pop) */
static KYTE_TLS KyteAnchorSlot kyte_slots[KYTE_MAX_DEPTH];
static KYTE_TLS int            kyte_depth = 0;

/* ── POSIX: per-thread alternate signal stack ───────────────────────────────── */
/*
 * An alternate stack lets signal handlers run even when the main stack
 * is exhausted or corrupted — required for reliable SIGSEGV recovery.
 * Each thread allocates its own alt-stack on first call to kyte_install_signals().
 */
#if defined(KYTE_POSIX)

/* 64 KiB is the de-facto minimum; SIGSTKSZ can be as small as 2 KiB on some
   systems but may be as large as 8192 on others.  We always use 64 KiB. */
#define KYTE_ALT_STACK_SIZE (65536)

static KYTE_TLS void *kyte_altstack_buf = NULL;  /* malloc'd per thread     */
static KYTE_TLS int   kyte_altstack_ok  = 0;     /* 1 once sigaltstack set  */

static void kyte_install_altstack(void) {
    if (kyte_altstack_ok) return;
    void *mem = malloc(KYTE_ALT_STACK_SIZE);
    if (!mem) return;
    stack_t ss;
    memset(&ss, 0, sizeof(ss));
    ss.ss_sp    = mem;
    ss.ss_size  = KYTE_ALT_STACK_SIZE;
    ss.ss_flags = 0;
    if (sigaltstack(&ss, NULL) == 0) {
        kyte_altstack_buf = mem;
        kyte_altstack_ok  = 1;
    } else {
        free(mem);
    }
}

/* Called on thread exit to free the alt-stack memory. */
static void kyte_cleanup_altstack(void) {
    if (!kyte_altstack_ok) return;
    stack_t dis;
    memset(&dis, 0, sizeof(dis));
    dis.ss_flags = SS_DISABLE;
    sigaltstack(&dis, NULL);
    free(kyte_altstack_buf);
    kyte_altstack_buf = NULL;
    kyte_altstack_ok  = 0;
}

#endif /* KYTE_POSIX */

/* ── Signal handler ──────────────────────────────────────────────────────────── */

static void kyte_handle_signal(int sig) {
    /* O(1): the active anchor is always the top slot */
    if (kyte_depth > 0) {
        int top = kyte_depth - 1;
        if (kyte_slots[top].in_use) {
            longjmp(kyte_slots[top].buf, sig);
            /* unreachable */
        }
    }
    /* No recovery anchor available — hard exit */
    (void)fprintf(stderr, "[kyte] unhandled signal %d — no recovery anchor\n", sig);
    kyte_hard_exit(2);
}

/* ── Install signal handlers ─────────────────────────────────────────────────── */

void kyte_install_signals(void) {
#if defined(KYTE_POSIX)
    /* Install alternate stack first — needed before SA_ONSTACK takes effect */
    kyte_install_altstack();

    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = kyte_handle_signal;
    sigemptyset(&sa.sa_mask);
    /*
     * SA_RESTART: restart syscalls interrupted by signals (avoids EINTR).
     * SA_ONSTACK: deliver signals on the alternate stack when available.
     *             Required for safe SIGSEGV handling.
     */
    sa.sa_flags = SA_RESTART;
    if (kyte_altstack_ok) sa.sa_flags |= SA_ONSTACK;

    sigaction(SIGFPE,  &sa, NULL);  /* floating-point / integer arithmetic fault */
    sigaction(SIGILL,  &sa, NULL);  /* illegal instruction                        */
    sigaction(SIGBUS,  &sa, NULL);  /* bus error (alignment, mapped-file faults)  */
    sigaction(SIGABRT, &sa, NULL);  /* abort() — explicit panic                   */

    /*
     * SIGSEGV: null dereference, stack overflow, etc.
     * Recovery is only reliable when delivered on an alternate stack;
     * without SA_ONSTACK the handler itself may fault.
     */
    if (kyte_altstack_ok) {
        sigaction(SIGSEGV, &sa, NULL);
    }

#else  /* KYTE_WINDOWS */
    /*
     * Windows does not have SIGBUS or SIGABRT-as-recoverable.
     * Use the ANSI signal() API — the only portable option on MSVC.
     */
    signal(SIGFPE, kyte_handle_signal);
    signal(SIGILL, kyte_handle_signal);
#endif
}

/* ── Anchor entry / exit ─────────────────────────────────────────────────────── */

/*
 * kyte_anchor_enter() — register a setjmp recovery point.
 *
 * Returns:
 *   0    fresh entry (normal execution)
 *   > 0  signal number that triggered longjmp (recovery path)
 *
 * MUST NOT be inlined: setjmp captures the CALLER's frame, so longjmp
 * restores execution inside this function which then returns the signal
 * number to the generated Kyte code.
 */
KYTE_NOINLINE int kyte_anchor_enter(void) {
    int idx, r;
    if (kyte_depth >= KYTE_MAX_DEPTH) {
        (void)fprintf(stderr, "[kyte] anchor depth overflow (%d max)\n", KYTE_MAX_DEPTH);
        kyte_hard_exit(3);
        return -1;
    }
    idx = kyte_depth;
    kyte_slots[idx].in_use = 1;
    kyte_depth++;

    r = setjmp(kyte_slots[idx].buf);
    if (r != 0) {
        /* longjmp return path — pop this slot */
        kyte_slots[idx].in_use = 0;
        kyte_depth--;
    }
    return r;
}

void kyte_anchor_exit(void) {
    if (kyte_depth > 0) {
        kyte_depth--;
        kyte_slots[kyte_depth].in_use = 0;
    }
}

/* ── Logging helpers ──────────────────────────────────────────────────────────── */

void kyte_log_kill(const char *anchor_name, const char *message) {
    if (message && *message) {
        (void)fprintf(stderr, "[kyte:kill] %s: %s\n", anchor_name, message);
    } else {
        (void)fprintf(stderr, "[kyte:kill] %s\n", anchor_name);
    }
}

void kyte_log_escalate(const char *anchor_name) {
    (void)fprintf(stderr, "[kyte:escalate] %s — too many failures, escalating\n", anchor_name);
}

void kyte_log_restart(const char *anchor_name, int attempt) {
    (void)fprintf(stderr, "[kyte:restart] %s (attempt %d)\n", anchor_name, attempt);
}

/* ── Thread-supervised anchors (@anchor(thread)) ─────────────────────────────── */

typedef struct {
    void       (*body)(void *);  /* anchor body function              */
    void        *arg;            /* forwarded to body                 */
    int          max_restarts;   /* escalate after this many failures */
    const char  *name;           /* for logging                       */
} KyteThreadCtx;

/*
 * Shared supervision loop — installs signals, calls body up to
 * max_restarts+1 times, then escalates.
 */
static void kyte_thread_run(KyteThreadCtx *ctx) {
    int attempt = 0;
    kyte_install_signals();
    for (; attempt <= ctx->max_restarts; ++attempt) {
        if (attempt > 0) kyte_log_restart(ctx->name, attempt);
        ctx->body(ctx->arg);
    }
    kyte_log_escalate(ctx->name);
    free(ctx);
#if defined(KYTE_POSIX)
    kyte_cleanup_altstack();
#endif
}

static KyteThreadCtx *kyte_make_ctx(void (*body)(void *), void *arg,
                                     int max_restarts, const char *name) {
    KyteThreadCtx *ctx = (KyteThreadCtx *)malloc(sizeof(*ctx));
    if (!ctx) return NULL;
    ctx->body         = body;
    ctx->arg          = arg;
    ctx->max_restarts = (max_restarts > 0) ? max_restarts : 3;
    ctx->name         = name;
    return ctx;
}

/* ── POSIX thread entry ──────────────────────────────────────────────────────── */
#if defined(KYTE_POSIX)

static void *kyte_thread_entry(void *raw) {
    kyte_thread_run((KyteThreadCtx *)raw);
    return NULL;
}

int kyte_spawn_thread_anchor(void (*body)(void *), void *arg,
                              int max_restarts, const char *anchor_name) {
    KyteThreadCtx *ctx = kyte_make_ctx(body, arg, max_restarts, anchor_name);
    pthread_t tid;
    int rc;
    if (!ctx) return -1;
    rc = pthread_create(&tid, NULL, kyte_thread_entry, ctx);
    if (rc != 0) { free(ctx); return rc; }
    pthread_detach(tid);
    return 0;
}

/* ── Windows thread entry ────────────────────────────────────────────────────── */
#elif defined(KYTE_WINDOWS)

static unsigned __stdcall kyte_thread_entry(void *raw) {
    kyte_thread_run((KyteThreadCtx *)raw);
    return 0;
}

int kyte_spawn_thread_anchor(void (*body)(void *), void *arg,
                              int max_restarts, const char *anchor_name) {
    KyteThreadCtx *ctx = kyte_make_ctx(body, arg, max_restarts, anchor_name);
    uintptr_t h;
    if (!ctx) return -1;
    h = _beginthreadex(NULL, 0, kyte_thread_entry, ctx, 0, NULL);
    if (h == 0) { free(ctx); return -1; }
    CloseHandle((HANDLE)h);
    return 0;
}

#endif /* KYTE_POSIX / KYTE_WINDOWS */
