/* For testing the build from JavaScript (doomtools): the session, exported as it is. */
#include <stdlib.h>
#include "cs_libc.h"
#include "session.h"

#define EXPORT(name) __attribute__((export_name(#name)))

static const unsigned char *saved;
static size_t saved_len;

EXPORT(p_init) void p_init(void) { cs_init(); }
EXPORT(p_new) void p_new(int skill, int episode, int map) { cs_new_game(skill, episode, map); }
EXPORT(p_demo) void p_demo(void) { cs_play_demo("demo1"); }
EXPORT(p_tic) void p_tic(int forward, int side, int turn, int buttons)
{
    ticcmd_t cmd = {0};
    cmd.forwardmove = (signed char)forward;
    cmd.sidemove = (signed char)side;
    cmd.angleturn = (short)turn;
    cmd.buttons = (unsigned char)buttons;
    cs_tic(&cmd);
}
EXPORT(p_frame) const uint32_t *p_frame(void) { return cs_frame(); }
EXPORT(p_state) int p_state(void) { return cs_state(); }
EXPORT(p_save) int p_save(void) { return cs_save(&saved, &saved_len); }
EXPORT(p_saved) const unsigned char *p_saved(void) { return saved; }
EXPORT(p_saved_len) size_t p_saved_len(void) { return saved_len; }
EXPORT(p_load) int p_load(const unsigned char *data, size_t len) { return cs_load(data, len); }
EXPORT(p_alloc) void *p_alloc(size_t n) { return malloc(n); }
EXPORT(p_rnd) int p_rnd(void) { return cs_random_index(); }
EXPORT(p_set_rnd) void p_set_rnd(int i) { cs_set_random_index(i); }
EXPORT(p_gametic) int p_gametic(void) { return cs_gametic(); }
EXPORT(p_set_gametic) void p_set_gametic(int t) { cs_set_gametic(t); }
EXPORT(log_ptr) const char *log_ptr(void) { size_t n; return cs_log(&n); }
EXPORT(log_len) size_t log_len(void) { size_t n; cs_log(&n); return n; }
