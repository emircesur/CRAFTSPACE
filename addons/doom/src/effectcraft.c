/* Doom as an EffectCraft effect (plug-in API v1).
 *
 * The effect draws a Doom game over its layer. The game is the moves in the effect's move log
 * (the Doom panel adds one per key press): "Latest move" shows the game after all of them,
 * "Replay over time" plays them back on the timeline, five tics (a seventh of a second) a move.
 *
 * EffectCraft may render any frame, in any order, on several threads, and the result must
 * depend only on the parameters and the time. Each frame therefore starts from a checkpoint (a
 * save game taken every five seconds of play, made once and kept while the moves before it stay
 * the same) and plays on from there, so it never depends on what was drawn before.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include <stdlib.h>
#include <string.h>

#include "doomdef.h"
#include "moves.h"
#include "session.h"
#include "state.h"

#define EXPORT(name) __attribute__((export_name(#name)))

#define LOG_SLIDERS 48
#define MOVES_PER_SLIDER 13 /* 4 bits each: 52 bits, exact in a double */
#define MAX_MOVES (LOG_SLIDERS * MOVES_PER_SLIDER)
#define CHECK_EVERY (5 * TICRATE)
#define MAX_CHECKS (MAX_MOVES * CS_TICS_PER_MOVE / CHECK_EVERY + 2)

enum { SHOW_LATEST, SHOW_REPLAY };
enum { P_SHOW, P_SKILL, P_COUNT, P_LOG, NUM_PARAMS = P_LOG + LOG_SLIDERS };

static char manifest[8192];
static size_t manifest_len;

static void build_manifest(void)
{
    char *p = manifest;
    p += sprintf(p,
                 "{\"api\":1,\"id\":\"org.craftspace.doom\",\"name\":\"Doom\",\"category\":\"Generate\","
                 "\"version\":\"1.0.0\",\"author\":\"CraftSpace\","
                 "\"description\":\"Plays Doom (Freedoom: Phase 1) on the layer. Window \\u203a Doom.jsx adds moves "
                 "as you type W A S D, Q E, F, Space; Replay over Time plays the run on the timeline.\","
                 "\"params\":["
                 "{\"id\":\"show\",\"name\":\"Show\",\"type\":\"popup\",\"options\":[\"Latest Move\",\"Replay over Time\"],\"default\":0},"
                 "{\"id\":\"skill\",\"name\":\"Skill\",\"type\":\"popup\",\"options\":[\"I'm too young to die\","
                 "\"Hey, not too rough\",\"Hurt me plenty\",\"Ultra-Violence\",\"Nightmare!\"],\"default\":2},"
                 "{\"id\":\"moves\",\"name\":\"Moves\",\"type\":\"slider\",\"default\":0,\"min\":0,\"max\":%d,\"decimals\":0}",
                 MAX_MOVES);
    for (int i = 0; i < LOG_SLIDERS; i++)
        p += sprintf(p,
                     ",{\"id\":\"log%d\",\"name\":\"Move Log %d\",\"type\":\"slider\",\"default\":0,\"min\":0,"
                     "\"max\":4503599627370495,\"decimals\":0}",
                     i + 1, i + 1);
    p += sprintf(p, "]}");
    manifest_len = (size_t)(p - manifest);
}

EXPORT(ec_api_version) int ec_api_version(void) { return 1; }

EXPORT(ec_manifest_ptr) const char *ec_manifest_ptr(void)
{
    if (!manifest_len)
        build_manifest();
    return manifest;
}

EXPORT(ec_manifest_len) int ec_manifest_len(void)
{
    if (!manifest_len)
        build_manifest();
    return (int)manifest_len;
}

static void *io_buf;
static size_t io_cap;

EXPORT(ec_alloc) void *ec_alloc(int bytes)
{
    size_t n = bytes > 0 ? (size_t)bytes : 1;
    if (n > io_cap) {
        free(io_buf);
        io_buf = malloc(n + 64);
        io_cap = io_buf ? n : 0;
    }
    /* 8-byte aligned: malloc's blocks are 16-byte aligned. */
    return io_buf;
}

/* ---- The game, from checkpoints -------------------------------------------------------- */

static unsigned char known_moves[MAX_MOVES]; /* the moves the checkpoints were made with */
static int known_count;
static int known_skill = -1;
static unsigned char *checks[MAX_CHECKS]; /* checks[k]: the game at tic k * CHECK_EVERY */
static size_t check_len[MAX_CHECKS];

static int move_at(const unsigned char *moves, int count, int index)
{
    return index < count ? moves[index] : CS_WAIT;
}

static void drop_checks_from(int k)
{
    for (; k < MAX_CHECKS; k++) {
        free(checks[k]);
        checks[k] = NULL;
    }
}

/* Plays tics [from, to) of the run; makes the checkpoints it passes when `keep` is set. */
static void play(const unsigned char *moves, int count, int skill, int from, int to, int keep)
{
    for (int p = from; p < to; p++) {
        if (cs_state() == GS_LEVEL) {
            ticcmd_t cmd = cs_move_cmd(move_at(moves, count, p / CS_TICS_PER_MOVE), p % CS_TICS_PER_MOVE,
                                       CS_TICS_PER_MOVE);
            cs_tic(&cmd);
        } else {
            cs_tic(NULL);
        }
        if (cs_state() == GS_INTERMISSION)
            cs_world_done(); /* straight on to the next map */
        if (keep && (p + 1) % CHECK_EVERY == 0) {
            int k = (p + 1) / CHECK_EVERY;
            if (k < MAX_CHECKS && !checks[k]) {
                size_t len;
                unsigned char *state = cs_state_pack(0, skill, 0, &len); /* NULL in the finale */
                checks[k] = state;
                check_len[k] = state ? len : 0;
                /* Go on from the checkpoint as loaded, so a checkpoint is the same however it
                 * was reached (anything a save game leaves out starts afresh at each). */
                if (state)
                    cs_state_load(state);
            }
        }
    }
}

static int start_from(int k)
{
    return checks[k] && cs_state_load(checks[k]) == 0;
}

/* The game at tic `target` of the run, ready to draw. */
static void game_at(const unsigned char *moves, int count, int skill, int target)
{
    if (skill != known_skill) {
        drop_checks_from(0);
        known_skill = skill;
    }
    /* Checkpoints made before the first changed move still hold. */
    int same = 0;
    int longest = count > known_count ? count : known_count;
    while (same < longest && move_at(moves, count, same) == move_at(known_moves, known_count, same))
        same++;
    if (same < longest)
        drop_checks_from(same * CS_TICS_PER_MOVE / CHECK_EVERY + 1);
    memcpy(known_moves, moves, (size_t)count);
    known_count = count;

    if (!checks[0]) {
        /* The run starts a tic in: Doom draws a level from its first tic on. */
        cs_new_game(skill, 1, 1);
        cs_tic(NULL);
        size_t len;
        checks[0] = cs_state_pack(0, skill, 0, &len);
        check_len[0] = len;
    }
    /* Make the checkpoints up to the target, from the last one there is. */
    int want = target / CHECK_EVERY;
    if (want >= MAX_CHECKS)
        want = MAX_CHECKS - 1;
    int k = want;
    while (k > 0 && !checks[k])
        k--;
    if (k < want) {
        start_from(k);
        play(moves, count, skill, k * CHECK_EVERY, want * CHECK_EVERY, 1);
        while (want > k && !checks[want]) /* the finale can't be kept */
            want--;
    }
    /* Then the frame itself, always from a checkpoint. */
    start_from(want);
    play(moves, count, skill, want * CHECK_EVERY, target, 0);
}

EXPORT(ec_render)
int ec_render(float *pixels, int width, int height, const double *params, int nparams, double time, double scale)
{
    (void)scale;
    if (nparams < NUM_PARAMS || width <= 0 || height <= 0)
        return 1;
    int show = (int)params[P_SHOW];
    int skill = (int)params[P_SKILL];
    int count = (int)params[P_COUNT];
    if (skill < 0 || skill > 4)
        skill = 2;
    if (count < 0)
        count = 0;
    if (count > MAX_MOVES)
        count = MAX_MOVES;
    static unsigned char moves[MAX_MOVES];
    for (int i = 0; i < count; i++) {
        double slider = params[P_LOG + i / MOVES_PER_SLIDER];
        unsigned long long bits = slider > 0 ? (unsigned long long)slider : 0;
        moves[i] = (bits >> (4 * (i % MOVES_PER_SLIDER))) & 15;
    }
    int end = count * CS_TICS_PER_MOVE;
    int target = end;
    if (show == SHOW_REPLAY) {
        target = time > 0 ? (int)(time * TICRATE) : 0;
        if (target > end)
            target = end;
    }

    cs_init();
    game_at(moves, count, skill, target);
    const uint32_t *frame = cs_frame();

    /* The frame, fitted to the layer (premultiplied RGBA; it's opaque). */
    double fit = (double)width / 320 < (double)height / 200 ? (double)width / 320 : (double)height / 200;
    int fw = (int)(320 * fit), fh = (int)(200 * fit);
    int ox = (width - fw) / 2, oy = (height - fh) / 2;
    for (int y = 0; y < height; y++) {
        float *row = pixels + (size_t)y * width * 4;
        int sy = fh > 0 ? (y - oy) * 200 / fh : -1;
        for (int x = 0; x < width; x++) {
            int sx = fw > 0 ? (x - ox) * 320 / fw : -1;
            uint32_t rgb = 0;
            if (sx >= 0 && sy >= 0 && sx < 320 && sy < 200 && x >= ox && y >= oy)
                rgb = frame[sy * 320 + sx];
            row[x * 4 + 0] = (float)((rgb >> 16) & 255) / 255.0f;
            row[x * 4 + 1] = (float)((rgb >> 8) & 255) / 255.0f;
            row[x * 4 + 2] = (float)(rgb & 255) / 255.0f;
            row[x * 4 + 3] = 1.0f;
        }
    }
    return 0;
}
