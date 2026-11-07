/*
 * EXP-2's C stand-ins for PolkaVM (T-008-03): the same step interpreter as the Rust stand-ins
 * (../rust/src/lib.rs), one sandboxed and one outside program per provider category, built by ../../build.sh
 * with clang for riscv64emac and linked by the PolkaVM linker. It uses PolkaVM's C guest header,
 * capi/polkavm_guest.h at v0.38.0, vendored in vendor/polkavm/ and checked by build.sh.
 *
 * Compile with -DRUTENT_CATEGORY='"reserve"' (or exchange, ramp, verifier) and one of -DRUTENT_SANDBOXED or
 * -DRUTENT_OUTSIDE. -DRUTENT_HOSTILE=1 adds the outside part's secret lookup to a sandboxed part, and
 * -DRUTENT_HOSTILE=2 a clock: hostile parts admission must refuse (scoring sheet B-02).
 *
 * See the Rust stand-ins for the spec's text form and what each part does.
 */
#include <stddef.h>
#include <stdint.h>

#include "polkavm_guest.h"

#ifndef RUTENT_CATEGORY
#error "RUTENT_CATEGORY must name the stand-in's category"
#endif

#define SLOT_SIZE 4096
#define TEXT 20
#define NONE UINT64_MAX
#define MAX_OBTAINED 16

POLKAVM_MIN_STACK_SIZE(131072)

/* Writable data, filled with '.', so that its reads are never folded and every byte stays in the blob: the
 * 16-byte marker and 4080 dots, 4096 bytes with no terminating NUL. */
#define DOTS16 "................"
#define DOTS256 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16
#define DOTS4080 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 DOTS256 \
    DOTS256 DOTS256 DOTS256 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16 DOTS16
/* `nonstring` says the array holds no terminating NUL on purpose; clang 21 and later warn without it, and older
 * ones don't know it. */
#if defined(__has_attribute)
#if __has_attribute(nonstring)
#define RUTENT_NONSTRING __attribute__((nonstring))
#endif
#endif
#ifndef RUTENT_NONSTRING
#define RUTENT_NONSTRING
#endif
static volatile unsigned char SLOT[SLOT_SIZE] RUTENT_NONSTRING = "RUTENT-SPEC-SLOT" DOTS4080;

static void trap(void) { POLKAVM_TRAP(); }

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = dst;
    const unsigned char *s = src;
    while (n--) *d++ = *s++;
    return dst;
}

void *memset(void *dst, int c, size_t n) {
    unsigned char *d = dst;
    while (n--) *d++ = (unsigned char)c;
    return dst;
}

typedef struct { const unsigned char *p; size_t n; } span;

static int eq(span a, const char *b) {
    size_t i = 0;
    for (; b[i]; i++) if (i >= a.n || a.p[i] != (unsigned char)b[i]) return 0;
    return i == a.n;
}

/* ---- the spec ---- */

static unsigned char spec_bytes[SLOT_SIZE];
static size_t spec_len;

static void spec_load(void) {
    for (size_t i = 0; i < SLOT_SIZE; i++) spec_bytes[i] = SLOT[i];
    size_t len = (size_t)spec_bytes[16] | (size_t)spec_bytes[17] << 8 | (size_t)spec_bytes[18] << 16 |
                 (size_t)spec_bytes[19] << 24;
    spec_len = len <= SLOT_SIZE - TEXT ? len : 0;
}

/* The i-th non-empty line, or a span of length 0 with p == 0. */
static span spec_line(size_t want) {
    const unsigned char *t = spec_bytes + TEXT;
    size_t at = 0, n = 0;
    while (at < spec_len) {
        size_t end = at;
        while (end < spec_len && t[end] != '\n') end++;
        if (end > at) {
            if (n == want) return (span){t + at, end - at};
            n++;
        }
        at = end + 1;
    }
    return (span){0, 0};
}

/* The w-th space-separated word of a line. */
static span word(span line, size_t w) {
    size_t at = 0, n = 0;
    while (at <= line.n) {
        size_t end = at;
        while (end < line.n && line.p[end] != ' ') end++;
        if (n == w) return (span){line.p + at, end - at};
        n++;
        at = end + 1;
    }
    return (span){0, 0};
}

/* The rest of the first line whose first word is tag. */
static span field(const char *tag) {
    for (size_t i = 0;; i++) {
        span line = spec_line(i);
        if (!line.p) return (span){(const unsigned char *)"", 0};
        span first = word(line, 0);
        if (eq(first, tag)) {
            size_t skip = first.n + 1;
            return skip <= line.n ? (span){line.p + skip, line.n - skip} : (span){line.p + line.n, 0};
        }
    }
}

/* The i-th step's line, or p == 0. */
static span step_line(size_t want) {
    size_t n = 0;
    for (size_t i = 0;; i++) {
        span line = spec_line(i);
        if (!line.p) return line;
        if (eq(word(line, 0), "step")) {
            if (n == want) return line;
            n++;
        }
    }
}

static uint32_t start_part(void) {
    spec_load();
    if (!eq(field("k"), RUTENT_CATEGORY) || !eq(field("s"), "1")) trap();
    return 1;
}

/* ---- writing ---- */

typedef struct { unsigned char *buf; size_t cap, len; } writer;

static void raw(writer *w, const void *b, size_t n) {
    if (w->len + n > w->cap) trap();
    memcpy(w->buf + w->len, b, n);
    w->len += n;
}
static void lit(writer *w, const char *s) {
    size_t n = 0;
    while (s[n]) n++;
    raw(w, s, n);
}
static void sp(writer *w, span s) { raw(w, s.p, s.n); }
static void quoted(writer *w, span s) { lit(w, "\""); sp(w, s); lit(w, "\""); }
__attribute__((unused)) static void esc(writer *w, const unsigned char *b, size_t n) {
    for (size_t i = 0; i < n; i++) {
        unsigned char c = b[i];
        if (c == '"') lit(w, "\\\"");
        else if (c == '\\') lit(w, "\\\\");
        else if (c == '\n') lit(w, "\\n");
        else if (c < 0x20) lit(w, "?");
        else raw(w, &c, 1);
    }
}
__attribute__((unused)) static void num(writer *w, size_t v) {
    unsigned char d[20];
    size_t i = sizeof d;
    do { d[--i] = (unsigned char)('0' + v % 10); v /= 10; } while (v);
    raw(w, d + i, sizeof d - i);
}

/* ---- reading JSON the platform and the other part write ---- */

static long find(span hay, const char *needle) {
    size_t n = 0;
    while (needle[n]) n++;
    if (n > hay.n) return -1;
    for (size_t i = 0; i + n <= hay.n; i++) {
        size_t j = 0;
        while (j < n && hay.p[i + j] == (unsigned char)needle[j]) j++;
        if (j == n) return (long)i;
    }
    return -1;
}

static span string_at(span hay, size_t at) {
    size_t i = at;
    while (i < hay.n) {
        if (hay.p[i] == '\\') i += 2;
        else if (hay.p[i] == '"') return (span){hay.p + at, i - at};
        else i++;
    }
    return (span){0, 0};
}

/* The content of the first string "key":"..." in hay, still escaped; p == 0 if none. */
static span find_str(span hay, const char *key) {
    unsigned char pat[64];
    writer w = {pat, sizeof pat - 1, 0};
    lit(&w, "\"");
    lit(&w, key);
    lit(&w, "\":\"");
    pat[w.len] = 0;
    long at = find(hay, (const char *)pat);
    if (at < 0) return (span){0, 0};
    return string_at(hay, (size_t)at + w.len);
}

__attribute__((unused)) static long find_num(span hay, const char *key) {
    unsigned char pat[64];
    writer w = {pat, sizeof pat - 1, 0};
    lit(&w, "\"");
    lit(&w, key);
    lit(&w, "\":");
    pat[w.len] = 0;
    long at = find(hay, (const char *)pat);
    if (at < 0) return -1;
    size_t i = (size_t)at + w.len, start = i;
    long v = 0;
    while (i < hay.n && hay.p[i] >= '0' && hay.p[i] <= '9') v = v * 10 + (hay.p[i++] - '0');
    return i > start ? v : -1;
}

__attribute__((unused)) static void unescape(writer *w, span s) {
    for (size_t i = 0; i < s.n; i++) {
        if (s.p[i] == '\\' && i + 1 < s.n) {
            i++;
            if (s.p[i] == 'n') lit(w, "\n");
            else raw(w, s.p + i, 1);
        } else {
            raw(w, s.p + i, 1);
        }
    }
}

#ifdef RUTENT_SANDBOXED

POLKAVM_IMPORT(uint64_t, rt_input, uint64_t, uint64_t)
POLKAVM_IMPORT(uint64_t, rt_outside, uint64_t, uint64_t, uint64_t, uint64_t)
POLKAVM_IMPORT(void, rt_output, uint64_t, uint64_t)

#if RUTENT_HOSTILE == 1
POLKAVM_IMPORT(uint64_t, rt_secret, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)
static void reach(void) {
    unsigned char out[8];
    rt_secret((uint64_t)(uintptr_t)"x", 1, (uint64_t)(uintptr_t)"x", 1, (uint64_t)(uintptr_t)out, 8);
}
#elif RUTENT_HOSTILE == 2
POLKAVM_IMPORT(uint64_t, clock_now)
static void reach(void) { clock_now(); }
#else
static void reach(void) {}
#endif

static unsigned char input_buf[8192];
static span input;
static unsigned char obtained_buf[4096];
static span obtained[MAX_OBTAINED];
static size_t obtained_count, obtained_used;
static unsigned char req_buf[8192], resp_buf[8192], out_buf[16384];

static void push(span v) {
    if (obtained_count == MAX_OBTAINED || obtained_used + v.n > sizeof obtained_buf) trap();
    memcpy(obtained_buf + obtained_used, v.p, v.n);
    obtained[obtained_count++] = (span){obtained_buf + obtained_used, v.n};
    obtained_used += v.n;
}

static void honest(writer *w, const char *kind, int another_transaction, int extra, size_t pad) {
    span tx = find_str(input, "transaction"), amount = find_str(input, "amount"), asset = find_str(input, "asset");
    span provider = field("p");
    lit(w, "{");
    if (extra) lit(w, "\"extra\":true,");
    lit(w, "\"kind\":\"");
    lit(w, kind);
    lit(w, "\",\"outcome\":{\"amount_in\":");
    quoted(w, amount);
    lit(w, ",\"amount_out\":");
    quoted(w, amount);
    lit(w, ",\"evidence\":[{\"by\":");
    quoted(w, provider);
    lit(w, ",\"grade\":3,\"reference\":\"");
    sp(w, provider);
    lit(w, "-");
    sp(w, field("v"));
    for (size_t i = 0; i < obtained_count; i++) { lit(w, "-"); sp(w, obtained[i]); }
    for (size_t i = 0; i < pad; i++) lit(w, "x");
    lit(w, "\"}],\"fee\":{\"amount\":\"0\",\"asset\":");
    quoted(w, asset);
    lit(w, "},\"status\":\"succeeded\"},\"transaction\":\"");
    sp(w, tx);
    if (another_transaction) lit(w, "-another");
    lit(w, "\"}");
}

static size_t render(void) {
    span output = field("out");
    writer w = {out_buf, sizeof out_buf, 0};
    if (eq(output, "unparseable")) lit(&w, "not a result {");
    else if (eq(output, "kind")) honest(&w, "deposit", 0, 0, 0);
    else if (eq(output, "tx")) honest(&w, "withdrawal", 1, 0, 0);
    else if (eq(output, "field")) honest(&w, "withdrawal", 0, 1, 0);
    else {
        honest(&w, "withdrawal", 0, 0, 0);
        if (output.n > 4 && eq((span){output.p, 4}, "pad ")) {
            size_t size = 0;
            for (size_t i = 4; i < output.n; i++) size = size * 10 + (size_t)(output.p[i] - '0');
            if (size > w.len) {
                size_t pad = size - w.len;
                w.len = 0;
                honest(&w, "withdrawal", 0, 0, pad);
            }
        }
    }
    return w.len;
}

static uint32_t start(void) {
    if (SLOT[0] == 0) reach();
    return start_part();
}

static void handle(void) {
    spec_load();
    uint64_t n = rt_input((uint64_t)(uintptr_t)input_buf, sizeof input_buf);
    input = (span){input_buf, n < sizeof input_buf ? (size_t)n : sizeof input_buf};
    span tx = find_str(input, "transaction");
    obtained_count = obtained_used = 0;
    for (size_t i = 0;; i++) {
        span line = step_line(i);
        if (!line.p) break;
        span kind = word(line, 1);
        writer w = {req_buf, sizeof req_buf, 0};
        lit(&w, "{\"transaction\":");
        quoted(&w, tx);
        lit(&w, ",\"operation\":");
        if (eq(kind, "L") && eq(word(line, 2), "s")) {
            quoted(&w, word(line, 3));
            lit(&w, ",\"arguments\":{},\"secret\":");
            quoted(&w, word(line, 4));
            lit(&w, "}");
        } else {
            span operation = eq(kind, "L") ? word(line, 3) : eq(kind, "C") ? word(line, 2) : field("o");
            /* "x <operation>", which the suite never writes, is a hostile spec: every request names that operation,
             * while the outside part acts under the step's own (the binding's own test, T-008-03). */
            span hostile = field("x");
            if (hostile.n) operation = hostile;
            quoted(&w, operation);
            lit(&w, ",\"arguments\":{\"step\":");
            num(&w, i);
            lit(&w, ",\"carry\":[");
            for (size_t j = 0; j < obtained_count; j++) {
                if (j) lit(&w, ",");
                quoted(&w, obtained[j]);
            }
            lit(&w, "]}}");
        }
        uint64_t r = rt_outside((uint64_t)(uintptr_t)req_buf, w.len, (uint64_t)(uintptr_t)resp_buf, sizeof resp_buf);
        if (r != NONE) {
            span resp = {resp_buf, r < sizeof resp_buf ? (size_t)r : sizeof resp_buf};
            span v = find_str(resp, "obtained");
            if (v.p) push(v);
        }
    }
    size_t len = render();
    rt_output((uint64_t)(uintptr_t)out_buf, len);
}

POLKAVM_EXPORT(uint32_t, start)
POLKAVM_EXPORT(void, handle)

#elif defined(RUTENT_OUTSIDE)

POLKAVM_IMPORT(uint64_t, rt_request, uint64_t, uint64_t)
POLKAVM_IMPORT(void, rt_respond, uint64_t, uint64_t)
POLKAVM_IMPORT(uint64_t, rt_secret, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)
POLKAVM_IMPORT(uint64_t, rt_state, uint64_t, uint64_t, uint64_t, uint64_t)
POLKAVM_IMPORT(uint64_t, rt_egress, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t)
POLKAVM_IMPORT(uint64_t, rt_call, uint64_t, uint64_t, uint64_t, uint64_t)

static unsigned char req_buf[8192], resp_buf[8192], value_buf[2048], body_buf[4096];

#define P(x) ((uint64_t)(uintptr_t)(x))

static uint32_t start(void) { return start_part(); }

static void serve(void) {
    spec_load();
    uint64_t n = rt_request(P(req_buf), sizeof req_buf);
    span req = {req_buf, n < sizeof req_buf ? (size_t)n : sizeof req_buf};
    span tx = find_str(req, "transaction"), op = find_str(req, "operation");
    long got = -1;
    long i = find_num(req, "step");
    span line = i >= 0 ? step_line((size_t)i) : (span){0, 0};
    if (line.p) {
        span kind = word(line, 1);
        if (eq(kind, "L")) {
            span operation = word(line, 3), name = word(line, 4);
            uint64_t r = rt_secret(P(operation.p), operation.n, P(name.p), name.n, P(value_buf), sizeof value_buf);
            if (r != NONE) got = (long)(r < sizeof value_buf ? r : sizeof value_buf);
        } else if (eq(kind, "R")) {
            span key = word(line, 3);
            uint64_t r = rt_state(P(key.p), key.n, P(value_buf), sizeof value_buf);
            if (r != NONE) got = (long)(r < sizeof value_buf ? r : sizeof value_buf);
        } else if (eq(kind, "C")) {
            span operation = word(line, 2), url = word(line, 3);
            writer b = {body_buf, sizeof body_buf, 0};
            long at = find(req, "\"carry\":[");
            if (at >= 0) {
                size_t k = (size_t)at + 9;
                int first = 1;
                while (k < req.n && req.p[k] == '"') {
                    span s = string_at(req, k + 1);
                    if (!s.p) break;
                    if (!first) lit(&b, "\n");
                    unescape(&b, s);
                    first = 0;
                    k += s.n + 2;
                    if (k < req.n && req.p[k] == ',') k++;
                }
            }
            rt_egress(P(operation.p), operation.n, P(url.p), url.n, P(body_buf), b.len);
        } else if (eq(kind, "M")) {
            span callee = word(line, 2), what = word(line, 3);
            rt_call(P(callee.p), callee.n, P(what.p), what.n);
        }
    }
    writer w = {resp_buf, sizeof resp_buf, 0};
    lit(&w, "{\"transaction\":");
    quoted(&w, tx);
    lit(&w, ",\"operation\":");
    quoted(&w, op);
    lit(&w, ",\"outcome\":{\"status\":\"completed\",\"body\":{");
    if (got >= 0) {
        lit(&w, "\"obtained\":\"");
        esc(&w, value_buf, (size_t)got);
        lit(&w, "\"");
    }
    lit(&w, "}}}");
    rt_respond(P(resp_buf), w.len);
}

POLKAVM_EXPORT(uint32_t, start)
POLKAVM_EXPORT(void, serve)

#else
#error "define RUTENT_SANDBOXED or RUTENT_OUTSIDE"
#endif

