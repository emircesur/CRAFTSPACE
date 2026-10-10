/* Minimal C library for the CraftSpace Doom add-on (WebAssembly, no imports). */
#ifndef CS_STDIO_H
#define CS_STDIO_H
#include <stddef.h>
#include <stdarg.h>
typedef struct cs_file FILE;
#define EOF (-1)
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define BUFSIZ 1024
#define FILENAME_MAX 260
extern FILE *stdin, *stdout, *stderr;
int printf(const char *fmt, ...);
int fprintf(FILE *f, const char *fmt, ...);
int sprintf(char *buf, const char *fmt, ...);
int snprintf(char *buf, size_t n, const char *fmt, ...);
int vsnprintf(char *buf, size_t n, const char *fmt, va_list ap);
int vsprintf(char *buf, const char *fmt, va_list ap);
int vfprintf(FILE *f, const char *fmt, va_list ap);
int vprintf(const char *fmt, va_list ap);
int sscanf(const char *s, const char *fmt, ...);
int puts(const char *s);
int putchar(int c);
int fputs(const char *s, FILE *f);
int fputc(int c, FILE *f);
int putc(int c, FILE *f);
int fgetc(FILE *f);
int getc(FILE *f);
char *fgets(char *s, int n, FILE *f);
FILE *fopen(const char *name, const char *mode);
int fclose(FILE *f);
size_t fread(void *p, size_t size, size_t n, FILE *f);
size_t fwrite(const void *p, size_t size, size_t n, FILE *f);
int fseek(FILE *f, long off, int whence);
long ftell(FILE *f);
int fflush(FILE *f);
int feof(FILE *f);
int ferror(FILE *f);
int fileno(FILE *f);
int remove(const char *name);
int rename(const char *from, const char *to);
void perror(const char *s);
#endif
