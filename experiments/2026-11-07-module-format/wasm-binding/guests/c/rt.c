/* The C stand-ins' runtime: no C library is linked, so this file is all of it. A bump allocator over linear memory
 * past __heap_base, which grows the memory a page at a time and never gives memory back; each block keeps its size
 * before it, for realloc. A part handles a few operations in its life, and the platform bounds its memory. */
#include <stddef.h>
#include <stdint.h>

#include "shim/stdlib.h"
#include "shim/string.h"

extern unsigned char __heap_base;

static uintptr_t next;

void *malloc(size_t size) {
    if (next == 0) {
        next = (uintptr_t)&__heap_base;
    }
    uintptr_t start = (next + 15) & ~(uintptr_t)15;
    uintptr_t block = start + 16;
    uintptr_t end = block + size;
    uintptr_t have = __builtin_wasm_memory_size(0) * 65536;
    if (end > have) {
        size_t pages = (end - have + 65535) / 65536;
        if (__builtin_wasm_memory_grow(0, pages) == (size_t)-1) {
            return NULL;
        }
    }
    *(size_t *)start = size;
    next = end;
    return (void *)block;
}

void *calloc(size_t count, size_t size) {
    size_t total = count * size;
    void *p = malloc(total);
    if (p) {
        memset(p, 0, total);
    }
    return p;
}

void *realloc(void *ptr, size_t size) {
    if (ptr == NULL) {
        return malloc(size);
    }
    size_t old = *(size_t *)((uintptr_t)ptr - 16);
    void *p = malloc(size);
    if (p) {
        memcpy(p, ptr, old < size ? old : size);
    }
    return p;
}

void free(void *ptr) { (void)ptr; }

_Noreturn void abort(void) { __builtin_trap(); }

void *memcpy(void *dst, const void *src, size_t n) {
    unsigned char *d = dst;
    const unsigned char *s = src;
    while (n--) {
        *d++ = *s++;
    }
    return dst;
}

void *memmove(void *dst, const void *src, size_t n) {
    unsigned char *d = dst;
    const unsigned char *s = src;
    if (d < s) {
        while (n--) {
            *d++ = *s++;
        }
    } else {
        while (n--) {
            d[n] = s[n];
        }
    }
    return dst;
}

void *memset(void *dst, int c, size_t n) {
    unsigned char *d = dst;
    while (n--) {
        *d++ = (unsigned char)c;
    }
    return dst;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (size_t i = 0; i < n; i++) {
        if (x[i] != y[i]) {
            return x[i] - y[i];
        }
    }
    return 0;
}

size_t strlen(const char *s) {
    size_t n = 0;
    while (s[n]) {
        n++;
    }
    return n;
}
