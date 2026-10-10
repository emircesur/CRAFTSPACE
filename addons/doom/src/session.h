/* The Doom session the plug-ins drive (session.c). */
#ifndef CS_SESSION_H
#define CS_SESSION_H
#include <stddef.h>
#include <stdint.h>
#include "d_ticcmd.h"

void cs_init(void);
void cs_new_game(int skill, int episode, int map);
void cs_play_demo(const char *lump);
/* One tic with this move (NULL: none). */
void cs_tic(const ticcmd_t *cmd);
/* Draws the screen: 320 x 200 pixels, 0xAARRGGBB. */
const uint32_t *cs_frame(void);
/* gamestate_t: 0 level, 1 intermission, 2 finale, 3 demo screen. */
int cs_state(void);
/* Leaves the end-of-level tally for the next map (or the finale). */
void cs_world_done(void);
int cs_save(const unsigned char **data, size_t *len);
int cs_load(const unsigned char *data, size_t len);
/* After a load: display state outside the save game back as at start-up. */
void cs_reset_view(void);
int cs_random_index(void);
void cs_set_random_index(int i);
int cs_gametic(void);
void cs_set_gametic(int t);
#endif
