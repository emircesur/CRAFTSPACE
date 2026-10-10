/* Doom as a PhotoCraft filter plug-in (plug-in ABI v1).
 *
 * PhotoCraft runs a filter in a fresh instance each time, with no memory of earlier runs, so the
 * game lives in the picture: the low two bits of each pixel's red, green and blue hold the save
 * game (state.c), and the rest of the pixel shows the screen. Each run reads the game from the
 * canvas, plays one move for a few tics, draws the new frame and writes the game back. Bound to
 * keys through the Actions panel, that's Doom, a move per key press; Undo takes a move back.
 *
 * PhotoCraft hands a filter the canvas in bands; with an overlap of 256 pixels every band sees
 * the whole canvas when it is at most 1024 x 512, so each band reads the same game, plays the
 * same move and writes its part of the result.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include <stdlib.h>
#include <string.h>

#include "d_event.h"
#include "doomdef.h"
#include "moves.h"
#include "session.h"
#include "state.h"

#define EXPORT(name) __attribute__((export_name(#name)))

static const char manifest[] =
    "{\"id\":\"org.craftspace.doom\",\"name\":\"Doom\\u2026\",\"version\":\"1.0.0\",\"kind\":\"filter\","
    "\"author\":\"CraftSpace\","
    "\"description\":\"Plays Doom (Freedoom: Phase 1) on the canvas, one move per run: use a 640 x 400 RGB document "
    "and the Doom actions (Alt+Shift+W/A/S/D, Q/E strafe, F fire, Space use, 1-7 weapons). Undo takes a move back. "
    "The game is kept in the picture.\","
    "\"params\":{"
    "\"move\":{\"type\":\"choice\",\"options\":[\"Forward\",\"Back\",\"Turn left\",\"Turn right\",\"Strafe left\","
    "\"Strafe right\",\"Fire\",\"Use\",\"Forward and fire\",\"Turn around\",\"Wait\",\"Weapon 1\",\"Weapon 2\",\"Weapon 3\","
    "\"Weapon 4\",\"Weapon 5\",\"Weapon 6\",\"Weapon 7\",\"New game\"],\"default\":\"Forward\"},"
    "\"tics\":{\"type\":\"int\",\"min\":1,\"max\":35,\"default\":5},"
    "\"skill\":{\"type\":\"choice\",\"options\":[\"I'm too young to die\",\"Hey, not too rough\",\"Hurt me plenty\","
    "\"Ultra-Violence\",\"Nightmare!\"],\"default\":\"Hurt me plenty\"},"
    "\"map\":{\"type\":\"int\",\"min\":1,\"max\":9,\"default\":1}"
    "},\"overlap\":256,\"area\":\"canvas\"}";

EXPORT(pc_abi_version) int pc_abi_version(void) { return 1; }

EXPORT(pc_manifest) long long pc_manifest(void)
{
    return (long long)(sizeof(manifest) - 1) << 32 | (unsigned)(uintptr_t)manifest;
}

EXPORT(pc_alloc) void *pc_alloc(int size) { return malloc(size > 0 ? (size_t)size : 1); }

/* ---- The parameters (JSON the host validated) ------------------------------------------ */

/* The value after "key": in json (top level or in _image: the keys don't repeat). */
static const char *json_value(const char *json, size_t len, const char *key)
{
    size_t klen = strlen(key);
    for (size_t i = 0; i + klen + 3 <= len; i++) {
        if (json[i] == '"' && !memcmp(json + i + 1, key, klen) && json[i + 1 + klen] == '"') {
            const char *p = json + i + klen + 2;
            while (p < json + len && (*p == ' ' || *p == ':'))
                p++;
            return p;
        }
    }
    return NULL;
}

static long json_int(const char *json, size_t len, const char *key, long fallback)
{
    const char *p = json_value(json, len, key);
    if (!p)
        return fallback;
    return strtol(p, NULL, 10);
}

/* Copies a string value; "" when missing. */
static void json_string(const char *json, size_t len, const char *key, char *out, size_t cap)
{
    const char *p = json_value(json, len, key);
    size_t n = 0;
    if (p && *p == '"') {
        for (p++; p < json + len && *p != '"' && n + 1 < cap; p++)
            out[n++] = *p;
    }
    out[n] = 0;
}

static int choice_index(const char *value, const char *const *options, int count, int fallback)
{
    for (int i = 0; i < count; i++)
        if (!strcmp(value, options[i]))
            return i;
    return fallback;
}

/* ---- Moves ----------------------------------------------------------------------------- */

#define MOVE_NEW_GAME 100

/* The "move" options, in the manifest's order, and the move each plays. */
static const struct { const char *name; int move; } moves[] = {
    {"Forward", CS_FORWARD}, {"Back", CS_BACK}, {"Turn left", CS_TURN_LEFT}, {"Turn right", CS_TURN_RIGHT},
    {"Strafe left", CS_STRAFE_LEFT}, {"Strafe right", CS_STRAFE_RIGHT}, {"Fire", CS_FIRE}, {"Use", CS_USE},
    {"Forward and fire", CS_FORWARD_FIRE}, {"Turn around", CS_TURN_AROUND}, {"Wait", CS_WAIT},
    {"Weapon 1", CS_WEAPON1}, {"Weapon 2", CS_WEAPON2}, {"Weapon 3", CS_WEAPON3}, {"Weapon 4", CS_WEAPON4},
    {"Weapon 5", CS_WEAPON5}, {"Weapon 6", CS_WEAPON6}, {"Weapon 7", CS_WEAPON7}, {"New game", MOVE_NEW_GAME},
};

static const char *const skill_names[] = {
    "I'm too young to die", "Hey, not too rough", "Hurt me plenty", "Ultra-Violence", "Nightmare!",
};

/* ---- Pixels ---------------------------------------------------------------------------- */

static unsigned sample8(float v)
{
    if (!(v > 0))
        return 0;
    if (v >= 1)
        return 255;
    return (unsigned)(v * 255.0f + 0.5f);
}

/* The bits kept in canvas pixel i: 2 each in red, green and blue. */
static unsigned stream_bits(const unsigned char *stream, size_t len, size_t i)
{
    size_t bit = i * 6, byte = bit >> 3, shift = bit & 7;
    unsigned v = (byte < len ? stream[byte] : 0) << 8 | (byte + 1 < len ? stream[byte + 1] : 0);
    return (v >> (10 - shift)) & 63;
}

EXPORT(pc_filter) int pc_filter(float *buf, int buf_len, int width, int height, int channels, int format,
                                const char *params, int params_len)
{
    (void)buf_len;
    size_t plen = (size_t)params_len;
    int mode = (format >> 16) & 0xff;
    if (mode != 3 || channels < 3)
        return 10; /* not an RGB document */
    long x0 = json_int(params, plen, "x", 0), y0 = json_int(params, plen, "y", 0);
    long cw = json_int(params, plen, "canvasWidth", width), ch = json_int(params, plen, "canvasHeight", height);
    if (x0 > 0 || y0 > 0 || x0 + width < cw || y0 + height < ch)
        return 11; /* the canvas is larger than 1024 x 512: the band doesn't see all of it */

    char value[48];
    json_string(params, plen, "move", value, sizeof value);
    int move = CS_WAIT;
    for (size_t i = 0; i < sizeof moves / sizeof moves[0]; i++)
        if (!strcmp(value, moves[i].name))
            move = moves[i].move;
    json_string(params, plen, "skill", value, sizeof value);
    int skill = choice_index(value, skill_names, 5, 2);
    int map = (int)json_int(params, plen, "map", 1);
    int tics = (int)json_int(params, plen, "tics", 5);

    /* The game kept in the canvas. */
    size_t capacity = (size_t)cw * (size_t)ch * 6 / 8;
    unsigned char *stream = calloc(capacity + 1, 1);
    if (!stream)
        return 12;
    for (long cy = 0; cy < ch; cy++) {
        for (long cx = 0; cx < cw; cx++) {
            const float *px = buf + ((size_t)(cy - y0) * width + (size_t)(cx - x0)) * channels;
            unsigned bits = (sample8(px[0]) & 3) << 4 | (sample8(px[1]) & 3) << 2 | (sample8(px[2]) & 3);
            size_t bit = ((size_t)cy * cw + cx) * 6, byte = bit >> 3, shift = bit & 7;
            unsigned v = bits << (10 - shift);
            if (byte < capacity) stream[byte] |= v >> 8;
            if (byte + 1 < capacity) stream[byte + 1] |= v & 0xff;
        }
    }

    cs_init();
    size_t total;
    int have_game = cs_state_check(stream, capacity, &total) == 0;
    if (have_game && move != MOVE_NEW_GAME) {
        skill = cs_state_skill(stream);
        have_game = cs_state_load(stream) == 0;
    } else {
        have_game = 0;
    }
    free(stream);
    if (!have_game) {
        cs_new_game(skill, 1, map);
        cs_tic(NULL);
    } else {
        for (int t = 0; t < tics && cs_state() == GS_LEVEL; t++) {
            ticcmd_t cmd = cs_move_cmd(move, t, tics);
            cs_tic(&cmd);
        }
    }

    /* The end of a level: show the tally (counted to the end), then go on to the next map so
     * the game can be kept. After the last map the finale shows, and a move starts again. */
    int flags = 0;
    const uint32_t *frame;
    if (cs_state() == GS_INTERMISSION) {
        ticcmd_t press;
        memset(&press, 0, sizeof press);
        press.buttons = BT_ATTACK;
        cs_tic(&press);
        for (int t = 0; t < 4; t++)
            cs_tic(NULL);
        frame = cs_frame();
        uint32_t *copy = malloc(320 * 200 * 4);
        if (!copy)
            return 12;
        memcpy(copy, frame, 320 * 200 * 4);
        frame = copy;
        cs_world_done();
    } else {
        if (cs_state() == GS_FINALE) {
            for (int t = 0; t < 35 * 60; t++) /* let the text type itself out */
                cs_tic(NULL);
            flags = CS_STATE_FINISHED;
        }
        frame = cs_frame();
    }
    size_t state_len = 0;
    unsigned char *state = cs_state_pack(flags, skill, 1, &state_len);
    if (!state || state_len > capacity) {
        free(state);
        return 13; /* the canvas is too small to keep this game: use 640 x 400 */
    }

    /* The frame, scaled to fit the canvas, with the game in the low bits. */
    long scale = cw / 320 < ch / 200 ? cw / 320 : ch / 200;
    if (scale < 1)
        scale = 1;
    long ox = (cw - 320 * scale) / 2, oy = (ch - 200 * scale) / 2;
    int alpha = (format >> 8) & 1;
    for (int by = 0; by < height; by++) {
        long cy = y0 + by;
        for (int bx = 0; bx < width; bx++) {
            long cx = x0 + bx;
            float *px = buf + ((size_t)by * width + bx) * channels;
            uint32_t rgb = 0;
            long fx = cx - ox, fy = cy - oy;
            if (fx >= 0 && fy >= 0 && fx < 320 * scale && fy < 200 * scale)
                rgb = frame[(fy / scale) * 320 + fx / scale];
            unsigned bits = 0;
            if (cx >= 0 && cy >= 0 && cx < cw && cy < ch)
                bits = stream_bits(state, state_len, (size_t)cy * cw + cx);
            px[0] = (float)(((rgb >> 16) & 0xfc) | (bits >> 4)) / 255.0f;
            px[1] = (float)(((rgb >> 8) & 0xfc) | ((bits >> 2) & 3)) / 255.0f;
            px[2] = (float)((rgb & 0xfc) | (bits & 3)) / 255.0f;
            if (alpha)
                px[channels - 1] = 1.0f;
        }
    }
    free(state);
    return 0;
}
