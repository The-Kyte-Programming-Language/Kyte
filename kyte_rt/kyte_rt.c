/*
 * kyte_rt.c — Kyte Language Runtime (cross-platform)
 *
 * Responsibilities:
 *  1. Signal-based fault recovery (SIGFPE, SIGILL on all platforms;
 *     SIGBUS on POSIX)
 *  2. Per-thread anchor setjmp/longjmp supervision stack
 *  3. Thread-based supervised anchor spawning (@anchor(thread))
 *  4. Logging helpers for Kill/recovery events
 */

/* ── Platform detection ────────────────────────────────────────────────────── */
#if defined(_WIN32) || defined(_WIN64)
#  define KYTE_WINDOWS 1
#else
#  define KYTE_POSIX 1
#endif

/* ── Platform-appropriate thread-local storage ─────────────────────────────── */
#if defined(_MSC_VER)
#  define KYTE_TLS __declspec(thread)
#elif defined(__GNUC__) || defined(__clang__)
#  define KYTE_TLS __thread
#else
#  define KYTE_TLS   /* fallback: no TLS — single-thread only */
#endif

/* ── Includes ───────────────────────────────────────────────────────────────── */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <setjmp.h>    /* setjmp / longjmp — available everywhere */

#if defined(KYTE_POSIX)
#  include <pthread.h>
#  include <unistd.h>
#endif

#if defined(KYTE_WINDOWS)
#  define WIN32_LEAN_AND_MEAN
#  include <windows.h>
#  include <process.h>  /* _beginthreadex */
#endif

/* ── Hard-exit helper (avoids stdio flush issues during signals) ─────────────── */
static void kyte_hard_exit(int code) {
#if defined(KYTE_WINDOWS)
    ExitProcess((UINT)code);
#else
    _exit(code);
#endif
}

/* ── Recovery stack types ───────────────────────────────────────────────────── */
#define KYTE_MAX_DEPTH 128

typedef struct {
    jmp_buf buf;          /* setjmp save point                     */
    volatile int in_use;  /* 1 while this slot is live             */
} KyteAnchorSlot;

/* Per-thread recovery stack */
static KYTE_TLS KyteAnchorSlot kyte_slots[KYTE_MAX_DEPTH];
static KYTE_TLS int            kyte_depth = 0;

/* ── Signal handler ──────────────────────────────────────────────────────────── */

static void kyte_handle_signal(int sig) {
    int i;
    for (i = kyte_depth - 1; i >= 0; --i) {
        if (kyte_slots[i].in_use) {
            longjmp(kyte_slots[i].buf, sig);
            /* unreachable */
        }
    }
    fprintf(stderr, "[kyte] unhandled signal %d — no recovery anchor\n", sig);
    kyte_hard_exit(2);
}

/* ── Install signal handlers ─────────────────────────────────────────────────── */

void kyte_install_signals(void) {
#if defined(KYTE_POSIX)
    struct sigaction sa;
    memset(&sa, 0, sizeof(sa));
    sa.sa_handler = kyte_handle_signal;
    sigemptyset(&sa.sa_mask);
    sa.sa_flags = SA_RESTART;
    sigaction(SIGFPE, &sa, NULL);
    sigaction(SIGILL, &sa, NULL);
    sigaction(SIGBUS, &sa, NULL);
    /*
     * SIGSEGV recovery is unreliable without sigaltstack because the
     * fault may have corrupted the stack pointer.
     */
#else
    /* Windows: use ANSI signal() — no SIGBUS on Windows */
    signal(SIGFPE, kyte_handle_signal);
    signal(SIGILL, kyte_handle_signal);
#endif
}

/* ── Anchor entry / exit ─────────────────────────────────────────────────────── */

/*
 * kyte_anchor_enter() — register a setjmp recovery point for the current anchor.
 *
 * Returns:
 *   0   fresh entry
 *   >0  signal number that triggered recovery (longjmp return path)
 *
 * IMPORTANT: This function must NOT be inlined.  The setjmp save-point captures
 * the stack frame of the CALLER (generated Kyte main/thread function), not this
 * helper.  We achieve this by calling setjmp from inside this function:
 * longjmp will restore execution inside kyte_anchor_enter, which then returns
 * the non-zero signal number to the generated code.
 */
#if defined(_MSC_VER)
__declspec(noinline)
#elif defined(__GNUC__) || defined(__clang__)
__attribute__((noinline))
#endif
int kyte_anchor_enter(void) {
    int idx;
    int r;
    if (kyte_depth >= KYTE_MAX_DEPTH) {
        fprintf(stderr, "[kyte] anchor depth overflow (%d max)\n", KYTE_MAX_DEPTH);
        kyte_hard_exit(3);
        return -1;
    }
    idx = kyte_depth;
    kyte_slots[idx].in_use = 1;
    kyte_depth++;

    r = setjmp(kyte_slots[idx].buf);
    if (r != 0) {
        /* Recovered from signal — pop this slot and report the signal */
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
        fprintf(stderr, "[kyte:kill] %s: %s\n", anchor_name, message);
    } else {
        fprintf(stderr, "[kyte:kill] %s\n", anchor_name);
    }
}

void kyte_log_escalate(const char *anchor_name) {
    fprintf(stderr, "[kyte:escalate] %s — too many failures, escalating\n", anchor_name);
}

void kyte_log_restart(const char *anchor_name, int attempt) {
    fprintf(stderr, "[kyte:restart] %s (attempt %d)\n", anchor_name, attempt);
}

/* ── Thread-supervised anchors (@anchor(thread)) ─────────────────────────────── */

typedef struct {
    void  (*body)(void *);   /* anchor body function                 */
    void   *arg;             /* forwarded to body                    */
    int     max_restarts;    /* escalate after this many failures    */
    const char *name;        /* for logging                          */
} KyteThreadCtx;

#if defined(KYTE_POSIX)

static void *kyte_thread_worker(void *raw) {
    KyteThreadCtx *ctx = (KyteThreadCtx *)raw;
    int attempt = 0;
    kyte_install_signals();   /* per-thread signal handler           */
    while (attempt <= ctx->max_restarts) {
        if (attempt > 0) {
            kyte_log_restart(ctx->name, attempt);
        }
        ctx->body(ctx->arg);
        attempt++;
    }
    kyte_log_escalate(ctx->name);
    free(ctx);
    return NULL;
}

int kyte_spawn_thread_anchor(void (*body)(void *),
                              void  *arg,
                              int    max_restarts,
                              const char *anchor_name) {
    KyteThreadCtx *ctx;
    pthread_t tid;
    int rc;
    ctx = (KyteThreadCtx *)malloc(sizeof(*ctx));
    if (!ctx) return -1;
    ctx->body         = body;
    ctx->arg          = arg;
    ctx->max_restarts = (max_restarts > 0) ? max_restarts : 3;
    ctx->name         = anchor_name;
    rc = pthread_create(&tid, NULL, kyte_thread_worker, ctx);
    if (rc != 0) { free(ctx); return rc; }
    pthread_detach(tid);
    return 0;
}

#elif defined(KYTE_WINDOWS)

static unsigned __stdcall kyte_thread_worker(void *raw) {
    KyteThreadCtx *ctx = (KyteThreadCtx *)raw;
    int attempt = 0;
    kyte_install_signals();
    while (attempt <= ctx->max_restarts) {
        if (attempt > 0) kyte_log_restart(ctx->name, attempt);
        ctx->body(ctx->arg);
        attempt++;
    }
    kyte_log_escalate(ctx->name);
    free(ctx);
    return 0;
}

int kyte_spawn_thread_anchor(void (*body)(void *),
                              void  *arg,
                              int    max_restarts,
                              const char *anchor_name) {
    KyteThreadCtx *ctx;
    uintptr_t h;
    ctx = (KyteThreadCtx *)malloc(sizeof(*ctx));
    if (!ctx) return -1;
    ctx->body         = body;
    ctx->arg          = arg;
    ctx->max_restarts = (max_restarts > 0) ? max_restarts : 3;
    ctx->name         = anchor_name;
    h = _beginthreadex(NULL, 0, kyte_thread_worker, ctx, 0, NULL);
    if (h == 0) { free(ctx); return -1; }
    CloseHandle((HANDLE)h);
    return 0;
}

#endif /* KYTE_POSIX / KYTE_WINDOWS */
