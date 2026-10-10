/* A game's state as bytes (state.c). */
#ifndef CS_STATE_H
#define CS_STATE_H
#include <stddef.h>
#include <stdint.h>

#define CS_STATE_HEADER 24
/* The episode is over (the finale is showing): there's no save game, a move starts again. */
#define CS_STATE_FINISHED 1
/* The save game is stored as it is, not compressed. */
#define CS_STATE_RAW 2

uint32_t cs_crc32(const unsigned char *p, size_t n);
size_t cs_lz_pack(const unsigned char *in, size_t n, unsigned char *out, size_t cap);
size_t cs_lz_unpack(const unsigned char *in, size_t n, unsigned char *out, size_t cap);
/* The current game (malloc'd), or NULL when it can't be saved. */
unsigned char *cs_state_pack(int flags, int skill, int compress, size_t *len);
/* 0 when p holds a state (its length in *total). */
int cs_state_check(const unsigned char *p, size_t n, size_t *total);
int cs_state_flags(const unsigned char *p);
int cs_state_skill(const unsigned char *p);
/* Loads the game in a checked state. */
int cs_state_load(const unsigned char *p);
#endif
