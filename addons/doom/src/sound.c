/* No sound: the ArtCraft apps' plug-ins can't play it. Stands in for s_sound.c.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include "s_sound.h"

int sfxVolume = 8;
int musicVolume = 8;
int snd_channels = 8;

void S_Init(int sfx_volume, int music_volume) { (void)sfx_volume; (void)music_volume; }
void S_Shutdown(void) {}
void S_Start(void) {}
void S_StartSound(void *origin, int sound_id) { (void)origin; (void)sound_id; }
void S_StopSound(mobj_t *origin) { (void)origin; }
void S_StartMusic(int music_id) { (void)music_id; }
void S_ChangeMusic(int music_id, int looping) { (void)music_id; (void)looping; }
boolean S_MusicPlaying(void) { return false; }
void S_StopMusic(void) {}
void S_PauseSound(void) {}
void S_ResumeSound(void) {}
void S_UpdateSounds(mobj_t *listener) { (void)listener; }
void S_SetMusicVolume(int volume) { (void)volume; }
void S_SetSfxVolume(int volume) { (void)volume; }
