/* A game's state as bytes: a header (with a checksum) and the save game, compressed, so a host
 * can keep it somewhere (PhotoCraft: in the low bits of the image) between runs.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include <stdlib.h>
#include <string.h>

#include "session.h"
#include "state.h"

uint32_t cs_crc32(const unsigned char *p, size_t n)
{
    uint32_t c = 0xffffffffu;
    while (n--) {
        c ^= *p++;
        for (int k = 0; k < 8; k++)
            c = c & 1 ? 0xedb88320u ^ (c >> 1) : c >> 1;
    }
    return ~c;
}

/* LZ77, byte-oriented: a token t < 128 is followed by t + 1 literal bytes; t >= 128 copies
 * (t & 127) + 3 bytes from a 16-bit distance back. */
size_t cs_lz_pack(const unsigned char *in, size_t n, unsigned char *out, size_t cap)
{
    enum { HASH_BITS = 14 };
    static int32_t table[1 << HASH_BITS];
    size_t i = 0, o = 0, lit = 0; /* start of the pending literal run */

    for (size_t k = 0; k < (1u << HASH_BITS); k++)
        table[k] = -1;
#define FLUSH_LITERALS(upto)                                  \
    while (lit < (upto)) {                                    \
        size_t run = (upto) - lit > 128 ? 128 : (upto) - lit; \
        if (o + 1 + run > cap)                                \
            return 0;                                         \
        out[o++] = (unsigned char)(run - 1);                  \
        memcpy(out + o, in + lit, run);                       \
        o += run;                                             \
        lit += run;                                           \
    }
    while (i + 3 <= n) {
        uint32_t h = ((in[i] << 16 | in[i + 1] << 8 | in[i + 2]) * 2654435761u) >> (32 - HASH_BITS);
        int32_t cand = table[h];
        table[h] = (int32_t)i;
        if (cand >= 0 && i - cand <= 65535 && !memcmp(in + cand, in + i, 3)) {
            size_t len = 3;
            while (len < 130 && i + len < n && in[cand + len] == in[i + len])
                len++;
            FLUSH_LITERALS(i);
            if (o + 3 > cap)
                return 0;
            out[o++] = (unsigned char)(128 + len - 3);
            out[o++] = (unsigned char)((i - cand) >> 8);
            out[o++] = (unsigned char)(i - cand);
            i += len;
            lit = i;
        } else {
            i++;
        }
    }
    FLUSH_LITERALS(n);
#undef FLUSH_LITERALS
    return o;
}

size_t cs_lz_unpack(const unsigned char *in, size_t n, unsigned char *out, size_t cap)
{
    size_t i = 0, o = 0;
    while (i < n) {
        unsigned t = in[i++];
        if (t < 128) {
            size_t run = t + 1;
            if (i + run > n || o + run > cap)
                return 0;
            memcpy(out + o, in + i, run);
            i += run;
            o += run;
        } else {
            if (i + 2 > n)
                return 0;
            size_t len = t - 128 + 3, dist = (size_t)in[i] << 8 | in[i + 1];
            i += 2;
            if (!dist || dist > o || o + len > cap)
                return 0;
            for (size_t k = 0; k < len; k++, o++)
                out[o] = out[o - dist];
        }
    }
    return o;
}

static void put32(unsigned char *p, uint32_t v) { p[0] = v >> 24; p[1] = v >> 16; p[2] = v >> 8; p[3] = v; }
static uint32_t get32(const unsigned char *p) { return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

unsigned char *cs_state_pack(int flags, int skill, int compress, size_t *len)
{
    const unsigned char *save = NULL;
    size_t save_len = 0;
    if (!(flags & CS_STATE_FINISHED) && cs_save(&save, &save_len) != 0)
        return NULL;
    unsigned char *out = malloc(CS_STATE_HEADER + save_len + save_len / 64 + 16);
    if (!out)
        return NULL;
    size_t packed;
    if (!compress) {
        flags |= CS_STATE_RAW;
        memcpy(out + CS_STATE_HEADER, save, save_len);
        packed = save_len;
    } else {
        packed = save_len ? cs_lz_pack(save, save_len, out + CS_STATE_HEADER, save_len + save_len / 64 + 16) : 0;
        if (save_len && !packed) {
            free(out);
            return NULL;
        }
    }
    memcpy(out, "CSD1", 4);
    out[4] = (unsigned char)flags;
    out[5] = (unsigned char)skill;
    out[6] = (unsigned char)cs_random_index();
    out[7] = 0;
    put32(out + 8, (uint32_t)cs_gametic());
    put32(out + 12, (uint32_t)save_len);
    put32(out + 16, (uint32_t)packed);
    put32(out + 20, cs_crc32(out + CS_STATE_HEADER, packed));
    *len = CS_STATE_HEADER + packed;
    return out;
}

int cs_state_check(const unsigned char *p, size_t n, size_t *total)
{
    if (n < CS_STATE_HEADER || memcmp(p, "CSD1", 4))
        return -1;
    size_t packed = get32(p + 16);
    if (packed > n - CS_STATE_HEADER || cs_crc32(p + CS_STATE_HEADER, packed) != get32(p + 20))
        return -1;
    *total = CS_STATE_HEADER + packed;
    return 0;
}

int cs_state_flags(const unsigned char *p) { return p[4]; }
int cs_state_skill(const unsigned char *p) { return p[5]; }

int cs_state_load(const unsigned char *p)
{
    size_t raw = get32(p + 12), packed = get32(p + 16);
    if (p[4] & CS_STATE_FINISHED)
        return -1;
    if (p[4] & CS_STATE_RAW)
        return raw == packed && cs_load(p + CS_STATE_HEADER, raw) == 0 ? (cs_reset_view(), cs_set_random_index(p[6]),
                                                                        cs_set_gametic((int)get32(p + 8)), 0)
                                                                     : -1;
    unsigned char *save = malloc(raw ? raw : 1);
    if (!save || cs_lz_unpack(p + CS_STATE_HEADER, packed, save, raw) != raw || cs_load(save, raw) != 0) {
        free(save);
        return -1;
    }
    free(save);
    cs_reset_view();
    cs_set_random_index(p[6]);
    cs_set_gametic((int)get32(p + 8));
    return 0;
}
