#ifndef CS_SYS_STAT_H
#define CS_SYS_STAT_H
#include <sys/types.h>
struct stat { off_t st_size; mode_t st_mode; };
#define S_IFDIR 0040000
#define S_ISDIR(m) (((m) & S_IFDIR) != 0)
int stat(const char *path, struct stat *st);
int mkdir(const char *path, mode_t mode);
#endif
