/* The C stand-ins build freestanding, with no C library: these are the few functions wit-bindgen's C bindings and
 * the stand-ins use, defined in rt.c. */
#ifndef RUTENT_SHIM_STDLIB_H
#define RUTENT_SHIM_STDLIB_H
#include <stddef.h>
void *malloc(size_t size);
void *calloc(size_t count, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
_Noreturn void abort(void);
#endif
