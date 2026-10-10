/* A Doom session the plug-ins drive one tic at a time: start a game, feed it moves, draw a
 * frame, save and load it in memory. Doom's own main loop (timers, input events, the menu)
 * isn't used.
 *
 * This file includes d_main.c to reach the start-up steps it keeps to itself.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include "d_main.c"

#include "cs_libc.h"
#include "doomgeneric.h"
#include "session.h"

void G_DoSaveGame(void);
void G_DoLoadGame(void);
void P_UpdateAnimations(void);

extern int prndindex;
extern int vanilla_savegame_limit;
extern char savename[256];

/* The WAD, compiled into the module (wad.s). */
extern const unsigned char cs_wad_data[];
extern const unsigned char cs_wad_end[];

pixel_t *DG_ScreenBuffer = NULL;
static uint32_t clock_ms;
static ticcmd_t cs_cmds[MAXPLAYERS];

uint32_t cs_clock_ms(void) { return clock_ms; }

/* doomgeneric's platform hooks: nothing to do, the host draws and gives the moves. */
void DG_Init(void) {}
void DG_DrawFrame(void) {}
void DG_SleepMs(uint32_t ms) { clock_ms += ms; }
uint32_t DG_GetTicksMs(void) { return clock_ms; }
int DG_GetKey(int *pressed, unsigned char *key) { (void)pressed; (void)key; return 0; }
void DG_SetWindowTitle(const char *title) { (void)title; }

void cs_init(void)
{
    static char *args[] = {"doom", NULL};
    static int done;

    if (done)
        return;
    done = 1;
    myargc = 1;
    myargv = args;
    cs_fs_add("freedoom1.wad", cs_wad_data, cs_wad_end - cs_wad_data, 1);
    DG_ScreenBuffer = malloc(DOOMGENERIC_RESX * DOOMGENERIC_RESY * 4);

    /* What D_DoomMain and D_DoomLoop do before the loop, without the parts for
     * files, the network, the command line, the timer and sound. */
    Z_Init();
    V_Init();
    savegamedir = "";
    iwadfile = "freedoom1.wad";
    gamemission = doom;
    D_AddFile(iwadfile);
    W_CheckCorrectIWAD(doom);
    D_IdentifyVersion();
    InitGameVersion();
    W_GenerateHashTable();
    D_SetGameDescription();
    M_Init();
    R_Init();
    P_Init();
    S_Init(sfxVolume * 8, musicVolume * 8);
    D_CheckNetGame();
    HU_Init();
    ST_Init();
    I_InitGraphics();
    V_RestoreBuffer();
    R_ExecuteSetViewSize();
    netcmds = cs_cmds;
    vanilla_savegame_limit = 0;
}

void cs_new_game(int skill, int episode, int map)
{
    demoplayback = false;
    G_InitNew((skill_t)skill, episode, map);
    gameaction = ga_nothing;
}

void cs_play_demo(const char *lump)
{
    G_DeferedPlayDemo((char *)lump);
    G_Ticker(); /* starts it (ga_playdemo) */
}

void cs_tic(const ticcmd_t *cmd)
{
    if (cmd)
        cs_cmds[consoleplayer] = *cmd;
    else
        memset(&cs_cmds[consoleplayer], 0, sizeof(ticcmd_t));
    G_Ticker();
    gametic++;
    clock_ms = gametic * 1000 / TICRATE;
}

const uint32_t *cs_frame(void)
{
    wipegamestate = gamestate; /* no screen melt */
    D_Display();
    return DG_ScreenBuffer;
}

int cs_state(void)
{
    return gamestate;
}

int cs_save(const unsigned char **data, size_t *len)
{
    if (gamestate != GS_LEVEL)
        return -1;
    /* Sets the slot and description; its request to save goes out with the next move
     * G_BuildTiccmd makes, which this session doesn't use. */
    G_SaveGame(0, "CraftSpace");
    G_DoSaveGame();
    players[consoleplayer].message = NULL;
    *data = cs_fs_get(P_SaveGameFile(0), len);
    return *data ? 0 : -1;
}

int cs_load(const unsigned char *data, size_t len)
{
    cs_fs_add(P_SaveGameFile(0), data, len, 0);
    M_StringCopy(savename, P_SaveGameFile(0), sizeof(savename));
    gamestate = GS_DEMOSCREEN;
    G_DoLoadGame();
    if (gamestate != GS_LEVEL)
        return -1;
    P_UpdateAnimations(); /* else they show as they were before the load until the next tic */
    return 0;
}

int cs_random_index(void) { return prndindex; }
void cs_set_random_index(int i) { prndindex = i & 0xff; }
int cs_gametic(void) { return gametic; }
void cs_set_gametic(int t) { gametic = t; clock_ms = gametic * 1000 / TICRATE; }

void cs_world_done(void)
{
    G_WorldDone();
    cs_tic(NULL);
}

/* Display state outside the save game (the status bar face, the menu's random numbers, the
 * spectre blur), put back as at start-up after a load so a frame depends only on the game. */
extern int fuzzpos;
void M_ClearRandom(void);
void ST_ResetFace(void);

void cs_reset_view(void)
{
    M_ClearRandom();
    fuzzpos = 0;
    ST_ResetFace();
}
