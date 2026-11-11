/* The C stand-ins of the Wasm binding (T-008-02): one module per provider category, each part its own component.
 * Built freestanding for wasm32 with no C library, once per part (-DPART_SANDBOXED or -DPART_OUTSIDE) and category
 * (-DCATEGORY="reserve", ...), against the bindings wit-bindgen's C generator writes for the part's world.
 *
 * Like the Rust stand-ins (guests/rust/src/lib.rs, which documents the program's lines), each carries the
 * module-boundary suite's ModuleSpec as a program the binding writes into its PROGRAM slot. The sandboxed part
 * drives it: its own steps go to the `denied` interface, each outside step to the outside part as an outside
 * request carrying the values obtained so far, and it renders the output the program names. The outside part runs
 * the one step it is asked for through the platform's proxy, state, egress and cross-module interfaces. */
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "json.h"
#include "shim/stdlib.h"
#include "shim/string.h"
#include "slot.h"

#if defined(PART_SANDBOXED)
#include "sandboxed.h"
typedef sandboxed_string_t wit_string;
#elif defined(PART_OUTSIDE)
#include "outside.h"
typedef outside_string_t wit_string;
#else
#error "define PART_SANDBOXED or PART_OUTSIDE"
#endif

#ifndef CATEGORY
#error "define CATEGORY"
#endif

static const char MAGIC[] = "RUTENT-PROGRAM-SLOT-V1\n";

/* The slot the binding writes the program into: `#` after the magic line, never zero, so that it sits in a data
 * segment of exactly this size, and volatile, so that the compiler can't fold the placeholder into the code. */
/* `nonstring` says the array holds no terminating NUL on purpose; newer clangs warn without it, and older ones,
 * clang 18 among them, don't know it. */
#if defined(__has_attribute)
#if __has_attribute(nonstring)
#define RUTENT_NONSTRING __attribute__((nonstring))
#endif
#endif
#ifndef RUTENT_NONSTRING
#define RUTENT_NONSTRING
#endif
__attribute__((used)) RUTENT_NONSTRING static const volatile char PROGRAM[RUTENT_SLOT_SIZE] = RUTENT_SLOT_INIT;

enum step_kind { SECRET, STATE, CALL, MODULE, SPIN, GROW, TRAP, FORGE, BURN, FETCH, REASON };

typedef struct {
    enum step_kind kind;
    bool sandboxed;
    char *a, *b, *c, *d; /* the step's words after its kind and part */
    long n;
    char *e, *f; /* a fetch's headers; its path is f */
} step;

typedef struct {
    char *provider, *version, *serves, *output, *output_arg;
    bool starts;
    step steps[64];
    size_t count;
} program;

static char *word_dup(const char *s, size_t n) {
    char *d = malloc(n + 1);
    memcpy(d, s, n);
    d[n] = 0;
    return d;
}

static bool eq(const char *a, const char *b) {
    size_t n = strlen(b);
    return strlen(a) == n && memcmp(a, b, n) == 0;
}

static long to_long(const char *s) {
    long n = 0;
    for (; *s >= '0' && *s <= '9'; s++) {
        n = n * 10 + (*s - '0');
    }
    return n;
}

static step mk(enum step_kind kind, bool sandboxed, char *a, char *b, char *c, char *d, long n) {
    step s;
    memset(&s, 0, sizeof(s));
    s.kind = kind;
    s.sandboxed = sandboxed;
    s.a = a;
    s.b = b;
    s.c = c;
    s.d = d;
    s.n = n;
    return s;
}

/* Reads the program from the slot; false if the slot holds none. */
static bool load(program *p) {
    static char copy[RUTENT_SLOT_SIZE + 1];
    for (size_t i = 0; i < RUTENT_SLOT_SIZE; i++) {
        copy[i] = PROGRAM[i];
    }
    copy[RUTENT_SLOT_SIZE] = 0;
    size_t magic = sizeof(MAGIC) - 1;
    if (memcmp(copy, MAGIC, magic) != 0) {
        return false;
    }
    memset(p, 0, sizeof(*p));
    char *line = copy + magic;
    for (;;) {
        char *end = line;
        while (*end && *end != '\n') {
            end++;
        }
        if (!*end) {
            return false;
        }
        char *words[8];
        size_t lens[8], count = 0;
        char *w = line;
        while (w < end && count < 8) {
            char *e = w;
            while (e < end && *e != ' ') {
                e++;
            }
            words[count] = w;
            lens[count] = (size_t)(e - w);
            count++;
            w = e + 1;
        }
        char *W[8];
        for (size_t i = 0; i < count; i++) {
            W[i] = word_dup(words[i], lens[i]);
        }
        if (count == 1 && eq(W[0], "end")) {
            return true;
        }
        step *s = &p->steps[p->count];
        if (count == 2 && eq(W[0], "provider")) {
            p->provider = W[1];
        } else if (count == 2 && eq(W[0], "version")) {
            p->version = W[1];
        } else if (count == 2 && eq(W[0], "starts")) {
            p->starts = eq(W[1], "1");
        } else if (count == 2 && eq(W[0], "serves")) {
            p->serves = W[1];
        } else if (count >= 2 && eq(W[0], "output")) {
            p->output = W[1];
            p->output_arg = count > 2 ? W[2] : "";
        } else if (p->count < 64) {
            if (count == 4 && eq(W[0], "secret")) {
                *s = mk(SECRET, eq(W[1], "sandboxed"), W[2], W[3], 0, 0, 0);
            } else if (count == 3 && eq(W[0], "state")) {
                *s = mk(STATE, eq(W[1], "sandboxed"), W[2], 0, 0, 0, 0);
            } else if (count == 5 && eq(W[0], "call")) {
                *s = mk(CALL, false, W[1], W[2], W[3], W[4], to_long(W[4]));
            } else if (count == 3 && eq(W[0], "module")) {
                *s = mk(MODULE, false, W[1], W[2], 0, 0, 0);
            } else if (count == 3 && eq(W[0], "ask")) {
                /* The module step whose outside part obtains the call's outcome. */
                *s = mk(MODULE, false, W[1], W[2], 0, 0, 1);
            } else if (count == 1 && eq(W[0], "spin")) {
                *s = mk(SPIN, true, 0, 0, 0, 0, 0);
            } else if (count == 2 && eq(W[0], "grow")) {
                *s = mk(GROW, true, 0, 0, 0, 0, to_long(W[1]));
            } else if (count == 1 && eq(W[0], "trap")) {
                *s = mk(TRAP, true, 0, 0, 0, 0, 0);
            } else if (count == 2 && eq(W[0], "burn")) {
                /* The outside part counts to W[1], then obtains `burned`: a probe of its fuel. */
                *s = mk(BURN, false, 0, 0, 0, 0, to_long(W[1]));
            } else if (count == 7 && eq(W[0], "fetch")) {
                /* The outside part sends a whole request: operation, host, port, method, path, headers. */
                *s = mk(FETCH, false, W[1], W[2], W[4], 0, to_long(W[3]));
                s->e = W[6];
                s->f = W[5];
            } else if (count == 3 && eq(W[0], "reason")) {
                *s = mk(REASON, false, W[1], W[2], 0, 0, 0);
            } else if (count == 6 && eq(W[0], "forge")) {
                /* Hostile: the outside part names W[1] to the platform, not the operation it serves. */
                *s = mk(FORGE, false, W[1], W[2], W[3], W[4], to_long(W[5]));
            } else {
                return false;
            }
            p->count++;
        } else {
            return false;
        }
        line = end + 1;
    }
}

static void set(wit_string *s, const char *text, size_t n) {
    s->ptr = (uint8_t *)word_dup(text, n);
    s->len = n;
}

static bool start_program(wit_string *err) {
    program p;
    const char *why = NULL;
    if (!load(&p)) {
        why = "the program slot holds no program";
    } else if (!p.starts) {
        why = "this release does not start";
    }
    if (why) {
        set(err, why, strlen(why));
        return false;
    }
    return true;
}

#if defined(PART_SANDBOXED)

/* The operation kind of this category's role that the stand-in answers besides the suite's withdrawal. */
static const char *category_kind(void) {
    if (eq(CATEGORY, "reserve")) {
        return "deposit";
    }
    if (eq(CATEGORY, "exchange")) {
        return "conversion";
    }
    if (eq(CATEGORY, "ramp")) {
        return "transfer";
    }
    return "judgement";
}

typedef struct {
    char *v[64];
    size_t n[64];
    size_t count;
} values;

static char *member(span obj, const char *key, size_t *len) {
    span v;
    char *s = NULL;
    *len = 0;
    if (json_get(obj, key, &v) && json_string(v, &s, len)) {
        return s;
    }
    s = malloc(1);
    s[0] = 0;
    return s;
}

static void put_s(text *t, const char *s) { put_string(t, s, strlen(s)); }

static void put_piece(text *t, program *p, size_t length) {
    puts_(t, ",{\"grade\":3,\"reference\":\"");
    for (size_t i = 0; i < length; i++) {
        puts_(t, "x");
    }
    puts_(t, "\",\"by\":");
    put_s(t, p->provider);
    puts_(t, "}");
}

/* The honest withdrawal result, compact, as the suite pins it: its one evidence reference is
 * `{provider}-{version}`, then `-{value}` for each value obtained, then `pad` more bytes of result in
 * extra evidence pieces of at most 512 characters each, as the result schema allows (`B-19`). */
static text honest(program *p, const char *kind, const char *transaction, const char *amount, const char *asset,
                   values *got, size_t pad) {
    text reference = {0};
    puts_(&reference, p->provider);
    puts_(&reference, "-");
    puts_(&reference, p->version);
    for (size_t i = 0; i < got->count; i++) {
        puts_(&reference, "-");
        put(&reference, got->v[i], got->n[i]);
    }
    text empty = {0};
    put_piece(&empty, p, 0);
    size_t fixed = empty.len;
    if (pad <= fixed) {
        for (size_t i = 0; i < pad; i++) {
            puts_(&reference, "x");
        }
        pad = 0;
    }
    text t = {0};
    puts_(&t, "{\"kind\":");
    put_s(&t, kind);
    puts_(&t, ",\"transaction\":");
    put_s(&t, transaction);
    puts_(&t, ",\"outcome\":{\"status\":\"succeeded\",\"amount_in\":");
    put_s(&t, amount);
    puts_(&t, ",\"amount_out\":");
    put_s(&t, amount);
    puts_(&t, ",\"fee\":{\"asset\":");
    put_s(&t, asset);
    puts_(&t, ",\"amount\":\"0\"},\"evidence\":[{\"grade\":3,\"reference\":");
    put_string(&t, reference.p ? reference.p : "", reference.len);
    puts_(&t, ",\"by\":");
    put_s(&t, p->provider);
    puts_(&t, "}");
    while (pad > 0) {
        size_t length = pad - fixed < 512 ? pad - fixed : 512;
        size_t rest = pad - fixed - length;
        if (rest > 0 && rest <= fixed) {
            /* Leave a remainder a piece can hold. */
            length -= fixed + 1 - rest;
        }
        put_piece(&t, p, length);
        pad -= fixed + length;
    }
    puts_(&t, "]}}");
    return t;
}

static void spin(void) {
    volatile unsigned long n = 0;
    for (;;) {
        n = n + 1;
    }
}

/* Runs the steps in order: its own through `denied`, each outside one as an outside request. */
static void run(program *p, const char *transaction, values *got) {
    for (size_t i = 0; i < p->count; i++) {
        step *s = &p->steps[i];
        bool outside = (s->kind == SECRET || s->kind == STATE) ? !s->sandboxed
                                                               : (s->kind == CALL || s->kind == MODULE || s->kind == FORGE || s->kind == BURN ||
                                                                s->kind == FETCH || s->kind == REASON);
        if (outside) {
            const char *operation = (s->kind == SECRET || s->kind == CALL || s->kind == FETCH || s->kind == REASON) ? s->a : p->serves;
            text request = {0};
            puts_(&request, "{\"transaction\":");
            put_s(&request, transaction);
            puts_(&request, ",\"operation\":");
            put_s(&request, operation);
            puts_(&request, ",\"arguments\":{\"step\":");
            put_number(&request, (long)i);
            puts_(&request, ",\"carried\":[");
            for (size_t k = 0; k < got->count; k++) {
                if (k > 0) {
                    puts_(&request, ",");
                }
                put_string(&request, got->v[k], got->n[k]);
            }
            puts_(&request, "]}}");
            sandboxed_string_t arg = {(uint8_t *)request.p, request.len}, ret = {0};
            rutent_binding_outside_requests_request(&arg, &ret);
            span response = span_of((const char *)ret.ptr, ret.len), outcome, status, body, obtained, item;
            if (json_get(response, "outcome", &outcome) && json_get(outcome, "status", &status) &&
                json_is(status, "completed") && json_get(outcome, "body", &body) &&
                json_get(body, "obtained", &obtained)) {
                size_t cursor = 0;
                while (got->count < 64 && json_next(obtained, &cursor, &item)) {
                    char *v;
                    size_t n;
                    if (json_string(item, &v, &n)) {
                        got->v[got->count] = v;
                        got->n[got->count] = n;
                        got->count++;
                    }
                }
            }
            continue;
        }
        switch (s->kind) {
            case SECRET: {
                sandboxed_string_t name = {(uint8_t *)s->b, strlen(s->b)};
                rutent_binding_denied_look_up_secret(&name);
                break;
            }
            case STATE: {
                sandboxed_string_t key = {(uint8_t *)s->a, strlen(s->a)};
                rutent_binding_denied_read_state(&key);
                break;
            }
            case SPIN:
                spin();
                break;
            case GROW:
                if (__builtin_wasm_memory_grow(0, (size_t)s->n) == (size_t)-1) {
                    __builtin_trap();
                }
                break;
            case TRAP:
                __builtin_trap();
            default:
                break;
        }
    }
}

bool exports_sandboxed_start(sandboxed_string_t *err) { return start_program(err); }

void exports_sandboxed_handle(sandboxed_string_t *operation, sandboxed_list_u8_t *ret) {
    program p;
    text out = {0};
    if (!load(&p)) {
        puts_(&out, "no program");
        ret->ptr = (uint8_t *)out.p;
        ret->len = out.len;
        return;
    }
    span envelope = span_of((const char *)operation->ptr, operation->len), statement = {0};
    json_get(envelope, "statement", &statement);
    size_t kn, tn, an, sn;
    char *kind = member(statement, "kind", &kn);
    char *transaction = member(statement, "transaction", &tn);
    char *amount = member(statement, "amount", &an);
    char *asset = member(statement, "asset", &sn);
    if (eq(kind, category_kind()) && !eq(kind, "withdrawal")) {
        /* The category's own operation, answered with a final refusal: a valid result of its kind. */
        puts_(&out, "{\"kind\":");
        put_s(&out, kind);
        span has;
        if (!eq(kind, "judgement") && json_get(statement, "transaction", &has)) {
            puts_(&out, ",\"transaction\":");
            put_s(&out, transaction);
        }
        puts_(&out, ",\"outcome\":{\"status\":\"refused\",\"refusal\":{\"class\":\"final\"}}}");
    } else {
        values got = {0};
        run(&p, transaction, &got);
        if (eq(p.output, "padded")) {
            size_t size = (size_t)to_long(p.output_arg);
            text plain = honest(&p, kind, transaction, amount, asset, &got, 0);
            out = honest(&p, kind, transaction, amount, asset, &got, size > plain.len ? size - plain.len : 0);
        } else if (eq(p.output, "unparseable")) {
            puts_(&out, "not a result {");
        } else if (eq(p.output, "another-kind")) {
            out = honest(&p, "deposit", transaction, amount, asset, &got, 0);
        } else if (eq(p.output, "another-transaction")) {
            text other = {0};
            puts_(&other, transaction);
            puts_(&other, "-another");
            out = honest(&p, kind, other.p, amount, asset, &got, 0);
        } else if (eq(p.output, "unknown-field")) {
            out = honest(&p, kind, transaction, amount, asset, &got, 0);
            out.len -= 1; /* drop the closing brace */
            puts_(&out, ",\"extra\":true}");
        } else {
            out = honest(&p, kind, transaction, amount, asset, &got, 0);
        }
    }
    ret->ptr = (uint8_t *)out.p;
    ret->len = out.len;
}

#else /* PART_OUTSIDE */

static outside_string_t str_of(const char *s) {
    outside_string_t r = {(uint8_t *)s, strlen(s)};
    return r;
}

/* `text` with a trailing `*N` replaced by N bytes of `x`. */
static const char *expand(const char *text, size_t len, size_t *out_len) {
    size_t star = len;
    while (star > 0 && text[star - 1] >= '0' && text[star - 1] <= '9') {
        star--;
    }
    if (star == len || star == 0 || text[star - 1] != '*') {
        *out_len = len;
        return word_dup(text, len);
    }
    size_t head = star - 1, n = (size_t)to_long(text + star);
    char *r = malloc(head + n + 1);
    memcpy(r, text, head);
    for (size_t i = 0; i < n; i++) {
        r[head + i] = 'x';
    }
    r[head + n] = 0;
    *out_len = head + n;
    return r;
}

/* A request with the method named (POST for anything but the other four), the path and the body. */
static rutent_binding_egress_request_t plain_request(const char *method, const char *path, const text *body) {
    rutent_binding_egress_request_t r;
    memset(&r, 0, sizeof(r));
    r.method = eq(method, "GET")      ? RUTENT_BINDING_EGRESS_METHOD_GET
               : eq(method, "PUT")    ? RUTENT_BINDING_EGRESS_METHOD_PUT
               : eq(method, "PATCH")  ? RUTENT_BINDING_EGRESS_METHOD_PATCH
               : eq(method, "DELETE") ? RUTENT_BINDING_EGRESS_METHOD_DELETE
                                      : RUTENT_BINDING_EGRESS_METHOD_POST;
    size_t path_len;
    r.path.ptr = (uint8_t *)expand(path, strlen(path), &path_len);
    r.path.len = path_len;
    r.body.ptr = (uint8_t *)(body->p ? body->p : "");
    r.body.len = body->len;
    return r;
}

/* Reads `name=value` items joined by commas into the request's headers; a `!` before a name flags the value as a
 * credential, and `-` is none. */
static void read_headers(rutent_binding_egress_request_t *r, const char *list) {
    static rutent_binding_egress_header_t headers[256];
    size_t count = 0;
    if (!eq(list, "-")) {
        const char *at = list;
        while (*at && count < 256) {
            const char *end = at;
            while (*end && *end != ',') {
                end++;
            }
            bool sensitive = *at == '!';
            const char *name = sensitive ? at + 1 : at;
            const char *eqs = name;
            while (eqs < end && *eqs != '=') {
                eqs++;
            }
            headers[count].name.ptr = (uint8_t *)word_dup(name, (size_t)(eqs - name));
            headers[count].name.len = (size_t)(eqs - name);
            const char *value = eqs < end ? eqs + 1 : end;
            size_t value_len;
            headers[count].value.ptr = (uint8_t *)expand(value, (size_t)(end - value), &value_len);
            headers[count].value.len = value_len;
            headers[count].sensitive = sensitive;
            count++;
            at = *end ? end + 1 : end;
        }
    }
    r->headers.ptr = headers;
    r->headers.len = count;
}

bool exports_outside_start(outside_string_t *err) { return start_program(err); }

void exports_outside_perform(outside_string_t *outside_request, outside_string_t *ret) {
    span request = span_of((const char *)outside_request->ptr, outside_request->len), v, arguments = {0};
    char *transaction = "", *operation = "";
    size_t tn = 0, on = 0;
    if (json_get(request, "transaction", &v)) {
        json_string(v, &transaction, &tn);
    }
    if (json_get(request, "operation", &v)) {
        json_string(v, &operation, &on);
    }
    json_get(request, "arguments", &arguments);
    text carried = {0};
    span list;
    if (json_get(arguments, "carried", &list)) {
        size_t cursor = 0, k = 0;
        span item;
        while (json_next(list, &cursor, &item)) {
            char *s;
            size_t n;
            if (json_string(item, &s, &n)) {
                if (k++ > 0) {
                    puts_(&carried, "\n");
                }
                put(&carried, s, n);
            }
        }
    }
    long index = -1;
    if (json_get(arguments, "step", &v)) {
        json_number(v, &index);
    }
    program p;
    text obtained = {0};
    puts_(&obtained, "[");
    if (load(&p) && index >= 0 && (size_t)index < p.count) {
        step *s = &p.steps[index];
        outside_string_t t = {(uint8_t *)transaction, tn}, o = {(uint8_t *)operation, on}, value = {0};
        switch (s->kind) {
            case SECRET: {
                outside_string_t name = {(uint8_t *)s->b, strlen(s->b)};
                rutent_binding_proxy_secret_refusal_t refusal;
                if (rutent_binding_proxy_look_up(&t, &o, &name, &value, &refusal)) {
                    put_string(&obtained, (const char *)value.ptr, value.len);
                }
                break;
            }
            case STATE: {
                outside_string_t key = {(uint8_t *)s->a, strlen(s->a)};
                if (rutent_binding_state_read(&key, &value)) {
                    put_string(&obtained, (const char *)value.ptr, value.len);
                }
                break;
            }
            case CALL: {
                outside_string_t host = {(uint8_t *)s->c, strlen(s->c)};
                rutent_binding_egress_request_t request = plain_request("POST", "/", &carried);
                rutent_binding_egress_scheme_t scheme =
                    eq(s->b, "wss") ? RUTENT_BINDING_EGRESS_SCHEME_WSS : RUTENT_BINDING_EGRESS_SCHEME_HTTPS;
                rutent_binding_egress_call(&o, scheme, &host, (uint16_t)s->n, &request);
                break;
            }
            case FETCH: {
                outside_string_t host = str_of(s->b);
                rutent_binding_egress_request_t request = plain_request(s->c, s->f, &carried);
                read_headers(&request, s->e);
                rutent_binding_egress_call(&o, RUTENT_BINDING_EGRESS_SCHEME_HTTPS, &host, (uint16_t)s->n, &request);
                break;
            }
            case REASON: {
                outside_string_t name = str_of(s->b);
                rutent_binding_proxy_secret_refusal_t refusal;
                if (!rutent_binding_proxy_look_up(&t, &o, &name, &value, &refusal)) {
                    const char *why = refusal == RUTENT_BINDING_PROXY_SECRET_REFUSAL_NOT_FOUND ? "not-found"
                                      : refusal == RUTENT_BINDING_PROXY_SECRET_REFUSAL_STORE_UNAVAILABLE
                                          ? "store-unavailable"
                                          : "undeclared-secret";
                    put_string(&obtained, why, strlen(why));
                }
                break;
            }
            case FORGE: {
                outside_string_t claimed = {(uint8_t *)s->a, strlen(s->a)};
                outside_string_t name = {(uint8_t *)s->b, strlen(s->b)};
                rutent_binding_proxy_secret_refusal_t refusal;
                if (rutent_binding_proxy_look_up(&t, &claimed, &name, &value, &refusal)) {
                    put_string(&obtained, (const char *)value.ptr, value.len);
                }
                outside_string_t host = {(uint8_t *)s->d, strlen(s->d)};
                rutent_binding_egress_request_t request = plain_request("POST", "/", &carried);
                rutent_binding_egress_scheme_t scheme =
                    eq(s->c, "wss") ? RUTENT_BINDING_EGRESS_SCHEME_WSS : RUTENT_BINDING_EGRESS_SCHEME_HTTPS;
                rutent_binding_egress_call(&claimed, scheme, &host, (uint16_t)s->n, &request);
                break;
            }
            case MODULE: {
                outside_string_t callee = {(uint8_t *)s->a, strlen(s->a)};
                outside_string_t kind = {(uint8_t *)s->b, strlen(s->b)};
                rutent_binding_modules_call_outcome_t outcome = rutent_binding_modules_call(&callee, &kind);
                if (s->n) {
                    const char *told = outcome == RUTENT_BINDING_MODULES_CALL_OUTCOME_DELIVERED     ? "delivered"
                                       : outcome == RUTENT_BINDING_MODULES_CALL_OUTCOME_UNANSWERED ? "unanswered"
                                                                                                   : "refused";
                    put_string(&obtained, told, strlen(told));
                }
                break;
            }
            case BURN: {
                volatile unsigned long n = 0;
                for (long i = 0; i < s->n; i++) {
                    n = n + 1;
                }
                put_string(&obtained, "burned", 6);
                break;
            }
            default:
                break;
        }
    }
    puts_(&obtained, "]");
    text response = {0};
    puts_(&response, "{\"transaction\":");
    put_string(&response, transaction, tn);
    puts_(&response, ",\"operation\":");
    put_string(&response, operation, on);
    puts_(&response, ",\"outcome\":{\"status\":\"completed\",\"body\":{\"obtained\":");
    put(&response, obtained.p, obtained.len);
    puts_(&response, "}}}");
    ret->ptr = (uint8_t *)response.p;
    ret->len = response.len;
}

#endif
