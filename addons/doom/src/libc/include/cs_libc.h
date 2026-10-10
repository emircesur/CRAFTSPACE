/* Hooks between the minimal C library and the Doom session. */
#ifndef CS_LIBC_H
#define CS_LIBC_H
#include <stddef.h>
#include <stdint.h>
/* A file kept in memory: read-only data is used in place (the WAD), else copied. */
void cs_fs_add(const char *name, const void *data, size_t size, int readonly);
const unsigned char *cs_fs_get(const char *name, size_t *size);
/* What Doom printed (the end of it), for errors. */
const char *cs_log(size_t *len);
/* The session's clock, in milliseconds of game time. */
uint32_t cs_clock_ms(void);
#endif
