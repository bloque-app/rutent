/* What the C stand-ins need of JSON: find a member of an object, read a string, a whole number or an array of
 * strings, and write compact JSON. Values are spans into the text they were read from. */
#ifndef RUTENT_JSON_H
#define RUTENT_JSON_H

#include <stdbool.h>
#include <stddef.h>

typedef struct {
    const char *p;
    size_t n;
} span;

/* A growable text. */
typedef struct {
    char *p;
    size_t len;
    size_t cap;
} text;

/* The member `key` of the object `obj`, if it is an object and has one. */
bool json_get(span obj, const char *key, span *out);
/* The string `v` decoded, NUL-terminated, if `v` is a string. */
bool json_string(span v, char **out, size_t *len);
/* The whole number `v`, if it is one. */
bool json_number(span v, long *out);
/* The next element of the array `arr` from `*cursor` (start at 0), if any. */
bool json_next(span arr, size_t *cursor, span *item);
/* Whether `v` is the string `s`. */
bool json_is(span v, const char *s);

void put(text *t, const char *s, size_t n);
void puts_(text *t, const char *s);
/* `s` as a JSON string, quoted and escaped. */
void put_string(text *t, const char *s, size_t n);
void put_number(text *t, long n);

span span_of(const char *s, size_t n);

#endif
