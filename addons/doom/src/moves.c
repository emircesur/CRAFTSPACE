/* The moves both plug-ins play (moves.h).
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include <string.h>

#include "d_event.h"
#include "moves.h"

ticcmd_t cs_move_cmd(int move, int t, int tics)
{
    ticcmd_t cmd;
    memset(&cmd, 0, sizeof cmd);
    switch (move) {
    case CS_FORWARD: cmd.forwardmove = 50; break;
    case CS_BACK: cmd.forwardmove = -50; break;
    case CS_TURN_LEFT: cmd.angleturn = 1280; break;
    case CS_TURN_RIGHT: cmd.angleturn = -1280; break;
    case CS_STRAFE_LEFT: cmd.sidemove = -40; break;
    case CS_STRAFE_RIGHT: cmd.sidemove = 40; break;
    case CS_FIRE: cmd.buttons = BT_ATTACK; break;
    case CS_FORWARD_FIRE: cmd.forwardmove = 50; cmd.buttons = BT_ATTACK; break;
    case CS_USE: if (t == 0) cmd.buttons = BT_USE; break;
    case CS_TURN_AROUND: {
        /* Half a turn, spread over the move's tics. */
        int step = 32768 / tics;
        cmd.angleturn = (short)(t == tics - 1 ? 32768 - step * (tics - 1) : step);
        break;
    }
    default:
        if (move >= CS_WEAPON1 && move <= CS_WEAPON5 && t == 0)
            cmd.buttons = BT_CHANGE | (move - CS_WEAPON1) << BT_WEAPONSHIFT;
        else if ((move == CS_WEAPON6 || move == CS_WEAPON7) && t == 0)
            cmd.buttons = BT_CHANGE | (move == CS_WEAPON6 ? 5 : 6) << BT_WEAPONSHIFT;
        break;
    }
    return cmd;
}
