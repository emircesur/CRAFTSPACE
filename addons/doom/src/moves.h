/* The moves both plug-ins play, as the commands Doom gets each tic (moves.c). */
#ifndef CS_MOVES_H
#define CS_MOVES_H
#include "d_ticcmd.h"

/* 0-15 fit in the 4 bits a move takes in EffectCraft's move log. */
enum {
    CS_WAIT, CS_FORWARD, CS_BACK, CS_TURN_LEFT, CS_TURN_RIGHT, CS_STRAFE_LEFT, CS_STRAFE_RIGHT,
    CS_FIRE, CS_USE, CS_FORWARD_FIRE, CS_WEAPON1, CS_WEAPON2, CS_WEAPON3, CS_WEAPON4, CS_WEAPON5,
    CS_TURN_AROUND, CS_WEAPON6, CS_WEAPON7,
};

#define CS_TICS_PER_MOVE 5

/* The command for tic t of a move lasting `tics` tics (running speeds, as with "always run"). */
ticcmd_t cs_move_cmd(int move, int t, int tics);
#endif
