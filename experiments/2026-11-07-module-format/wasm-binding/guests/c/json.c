/* See json.h. A reader for well-formed JSON as the platform writes it; on anything else it finds nothing. */
#include "json.h"

#include "shim/stdlib.h"
#include "shim/string.h"

span span_of(const char *s, size_t n) {
    span v = {s, n};
    return v;
}

static size_t ws(const char *s, size_t n, size_t i) {
    while (i < n && (s[i] == ' ' || s[i] == '\n' || s[i] == '\r' || s[i] == '\t')) {
        i++;
    }
    return i;
}

/* The index just past the string starting at `i`, or `n`. */
static size_t skip_string(const char *s, size_t n, size_t i) {
    if (i >= n || s[i] != '"') {
        return n;
    }
    for (i++; i < n; i++) {
        if (s[i] == '\\') {
            i++;
        } else if (s[i] == '"') {
            return i + 1;
        }
    }
    return n;
}

/* The index just past the value starting at `i`, or `n`. */
static size_t skip_value(const char *s, size_t n, size_t i) {
    i = ws(s, n, i);
    if (i >= n) {
        return n;
    }
    if (s[i] == '"') {
        return skip_string(s, n, i);
    }
    if (s[i] == '{' || s[i] == '[') {
        int depth = 0;
        while (i < n) {
            char c = s[i];
            if (c == '"') {
                i = skip_string(s, n, i);
                continue;
            }
            if (c == '{' || c == '[') {
                depth++;
            } else if (c == '}' || c == ']') {
                depth--;
                if (depth == 0) {
                    return i + 1;
                }
            }
            i++;
        }
        return n;
    }
    while (i < n && s[i] != ',' && s[i] != '}' && s[i] != ']' && s[i] != ' ' && s[i] != '\n') {
        i++;
    }
    return i;
}

bool json_is(span v, const char *s) {
    size_t len = strlen(s);
    return v.n == len + 2 && v.p[0] == '"' && memcmp(v.p + 1, s, len) == 0 && v.p[len + 1] == '"';
}

bool json_get(span obj, const char *key, span *out) {
    const char *s = obj.p;
    size_t n = obj.n, i = ws(s, n, 0);
    if (i >= n || s[i] != '{') {
        return false;
    }
    i++;
    for (;;) {
        i = ws(s, n, i);
        if (i >= n || s[i] == '}') {
            return false;
        }
        size_t key_start = i;
        i = skip_string(s, n, i);
        if (i >= n) {
            return false;
        }
        span name = {s + key_start, i - key_start};
        i = ws(s, n, i);
        if (i >= n || s[i] != ':') {
            return false;
        }
        i = ws(s, n, i + 1);
        size_t value_start = i;
        i = skip_value(s, n, i);
        if (json_is(name, key)) {
            out->p = s + value_start;
            out->n = i - value_start;
            return i <= n && i > value_start;
        }
        i = ws(s, n, i);
        if (i < n && s[i] == ',') {
            i++;
        }
    }
}

bool json_next(span arr, size_t *cursor, span *item) {
    const char *s = arr.p;
    size_t n = arr.n, i = *cursor;
    if (i == 0) {
        i = ws(s, n, 0);
        if (i >= n || s[i] != '[') {
            return false;
        }
        i++;
    }
    i = ws(s, n, i);
    if (i < n && s[i] == ',') {
        i = ws(s, n, i + 1);
    }
    if (i >= n || s[i] == ']') {
        return false;
    }
    size_t start = i;
    i = skip_value(s, n, i);
    if (i > n || i == start) {
        return false;
    }
    item->p = s + start;
    item->n = i - start;
    *cursor = i;
    return true;
}

bool json_string(span v, char **out, size_t *len) {
    if (v.n < 2 || v.p[0] != '"' || v.p[v.n - 1] != '"') {
        return false;
    }
    char *d = malloc(v.n);
    if (!d) {
        return false;
    }
    size_t k = 0;
    for (size_t i = 1; i + 1 < v.n; i++) {
        char c = v.p[i];
        if (c != '\\') {
            d[k++] = c;
            continue;
        }
        i++;
        switch (v.p[i]) {
            case 'n': d[k++] = '\n'; break;
            case 't': d[k++] = '\t'; break;
            case 'r': d[k++] = '\r'; break;
            case 'b': d[k++] = '\b'; break;
            case 'f': d[k++] = '\f'; break;
            case 'u': {
                /* Only the ASCII range is decoded; the stand-ins' values are ASCII. */
                unsigned code = 0;
                for (int j = 1; j <= 4 && i + j < v.n; j++) {
                    char h = v.p[i + j];
                    code = code * 16 + (unsigned)(h >= 'a' ? h - 'a' + 10 : h >= 'A' ? h - 'A' + 10 : h - '0');
                }
                d[k++] = code < 128 ? (char)code : '?';
                i += 4;
                break;
            }
            default: d[k++] = v.p[i]; break;
        }
    }
    d[k] = 0;
    *out = d;
    *len = k;
    return true;
}

bool json_number(span v, long *out) {
    long n = 0;
    size_t i = 0;
    if (v.n == 0) {
        return false;
    }
    for (; i < v.n; i++) {
        if (v.p[i] < '0' || v.p[i] > '9') {
            return false;
        }
        n = n * 10 + (v.p[i] - '0');
    }
    *out = n;
    return true;
}

void put(text *t, const char *s, size_t n) {
    if (t->len + n + 1 > t->cap) {
        size_t cap = (t->cap + n + 1) * 2;
        t->p = realloc(t->p, cap);
        if (!t->p) {
            abort();
        }
        t->cap = cap;
    }
    memcpy(t->p + t->len, s, n);
    t->len += n;
    t->p[t->len] = 0;
}

void puts_(text *t, const char *s) { put(t, s, strlen(s)); }

void put_string(text *t, const char *s, size_t n) {
    static const char hex[] = "0123456789abcdef";
    put(t, "\"", 1);
    for (size_t i = 0; i < n; i++) {
        unsigned char c = (unsigned char)s[i];
        if (c == '"' || c == '\\') {
            char e[2] = {'\\', (char)c};
            put(t, e, 2);
        } else if (c < 0x20) {
            char e[6] = {'\\', 'u', '0', '0', hex[c >> 4], hex[c & 15]};
            put(t, e, 6);
        } else {
            put(t, (const char *)&s[i], 1);
        }
    }
    put(t, "\"", 1);
}

void put_number(text *t, long n) {
    char digits[24];
    int k = 0;
    if (n == 0) {
        put(t, "0", 1);
        return;
    }
    if (n < 0) {
        put(t, "-", 1);
        n = -n;
    }
    while (n > 0) {
        digits[k++] = (char)('0' + n % 10);
        n /= 10;
    }
    while (k > 0) {
        put(t, &digits[--k], 1);
    }
}
