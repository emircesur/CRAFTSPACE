#ifndef CS_UNISTD_H
#define CS_UNISTD_H
#include <stddef.h>
#include <sys/types.h>
int usleep(unsigned usec);
unsigned sleep(unsigned sec);
int access(const char *path, int mode);
int isatty(int fd);
int close(int fd);
int read(int fd, void *buf, size_t n);
int write(int fd, const void *buf, size_t n);
char *getcwd(char *buf, size_t n);
#define F_OK 0
#define R_OK 4
#define W_OK 2
#endif
