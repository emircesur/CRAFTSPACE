/* Minimal C library for the CraftSpace Doom add-on.
 *
 * The ArtCraft apps run plug-ins as WebAssembly modules with no imports: no files, clock or
 * system calls. This is the part of the C library Doom uses, built on the module's own memory:
 * an allocator, string and formatting functions, and "files" kept in memory (the WAD is one,
 * compiled into the module; save games are others).
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
#include <ctype.h>
#include <errno.h>
#include <math.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

#include "cs_libc.h"

int errno;

/* ---- Memory ---------------------------------------------------------------------------- */

/* A first-fit allocator with an address-ordered free list, so freed neighbours merge. Blocks
 * carry their size (header included) just before the pointer handed out. */
extern unsigned char __heap_base;

typedef struct block {
    size_t size;
    struct block *next; /* only while free */
} block_t;

#define ALIGN 16
#define HEADER ALIGN
#define MIN_BLOCK (HEADER + ALIGN)

static block_t *free_list;
static uintptr_t heap_end;

static size_t round_up(size_t n, size_t a) { return (n + a - 1) & ~(a - 1); }

static int grow(size_t need) {
    if (!heap_end) heap_end = round_up((uintptr_t)&__heap_base, ALIGN);
    size_t have = __builtin_wasm_memory_size(0) * 65536;
    size_t want = heap_end + need;
    if (want > have) {
        size_t pages = (want - have + 65535) / 65536;
        if (__builtin_wasm_memory_grow(0, pages) == (size_t)-1) return 0;
    }
    block_t *b = (block_t *)heap_end;
    b->size = need;
    heap_end += need;
    free((char *)b + HEADER);
    return 1;
}

void *malloc(size_t n) {
    if (n == 0) n = 1;
    size_t need = round_up(n + HEADER, ALIGN);
    if (need < MIN_BLOCK) need = MIN_BLOCK;
    for (int attempt = 0; attempt < 2; attempt++) {
        block_t **link = &free_list;
        for (block_t *b = free_list; b; link = &b->next, b = b->next) {
            if (b->size < need) continue;
            if (b->size - need >= MIN_BLOCK) {
                block_t *rest = (block_t *)((char *)b + need);
                rest->size = b->size - need;
                rest->next = b->next;
                *link = rest;
                b->size = need;
            } else {
                *link = b->next;
            }
            return (char *)b + HEADER;
        }
        if (!grow(need < (1 << 20) ? (1 << 20) : need)) break;
    }
    errno = ENOMEM;
    return NULL;
}

void free(void *p) {
    if (!p) return;
    block_t *b = (block_t *)((char *)p - HEADER);
    block_t *prev = NULL, *cur = free_list;
    while (cur && cur < b) {
        prev = cur;
        cur = cur->next;
    }
    b->next = cur;
    if (prev) prev->next = b; else free_list = b;
    if (cur && (char *)b + b->size == (char *)cur) {
        b->size += cur->size;
        b->next = cur->next;
    }
    if (prev && (char *)prev + prev->size == (char *)b) {
        prev->size += b->size;
        prev->next = b->next;
    }
}

void *calloc(size_t n, size_t size) {
    size_t total = n * size;
    if (size && total / size != n) return NULL;
    void *p = malloc(total);
    if (p) memset(p, 0, total);
    return p;
}

void *realloc(void *p, size_t n) {
    if (!p) return malloc(n);
    block_t *b = (block_t *)((char *)p - HEADER);
    size_t have = b->size - HEADER;
    if (n <= have) return p;
    void *q = malloc(n);
    if (!q) return NULL;
    memcpy(q, p, have);
    free(p);
    return q;
}

/* ---- Strings --------------------------------------------------------------------------- */

void *memcpy(void *d, const void *s, size_t n) { return __builtin_memcpy(d, s, n); }
void *memmove(void *d, const void *s, size_t n) { return __builtin_memmove(d, s, n); }
void *memset(void *d, int c, size_t n) { return __builtin_memset(d, c, n); }

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (; n; n--, x++, y++)
        if (*x != *y) return *x - *y;
    return 0;
}

void *memchr(const void *s, int c, size_t n) {
    const unsigned char *p = s;
    for (; n; n--, p++)
        if (*p == (unsigned char)c) return (void *)p;
    return NULL;
}

size_t strlen(const char *s) { const char *p = s; while (*p) p++; return p - s; }
size_t strnlen(const char *s, size_t n) { size_t i = 0; while (i < n && s[i]) i++; return i; }
char *strcpy(char *d, const char *s) { char *r = d; while ((*d++ = *s++)) {} return r; }

char *strncpy(char *d, const char *s, size_t n) {
    size_t i = 0;
    for (; i < n && s[i]; i++) d[i] = s[i];
    for (; i < n; i++) d[i] = 0;
    return d;
}

char *strcat(char *d, const char *s) { strcpy(d + strlen(d), s); return d; }

char *strncat(char *d, const char *s, size_t n) {
    char *e = d + strlen(d);
    while (n-- && *s) *e++ = *s++;
    *e = 0;
    return d;
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b) a++, b++;
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    for (; n; n--, a++, b++) {
        if (*a != *b || !*a) return (unsigned char)*a - (unsigned char)*b;
    }
    return 0;
}

int strcasecmp(const char *a, const char *b) {
    while (*a && tolower((unsigned char)*a) == tolower((unsigned char)*b)) a++, b++;
    return tolower((unsigned char)*a) - tolower((unsigned char)*b);
}

int strncasecmp(const char *a, const char *b, size_t n) {
    for (; n; n--, a++, b++) {
        int x = tolower((unsigned char)*a), y = tolower((unsigned char)*b);
        if (x != y || !x) return x - y;
    }
    return 0;
}

char *strchr(const char *s, int c) {
    for (;; s++) {
        if (*s == (char)c) return (char *)s;
        if (!*s) return NULL;
    }
}

char *strrchr(const char *s, int c) {
    const char *r = NULL;
    for (;; s++) {
        if (*s == (char)c) r = s;
        if (!*s) return (char *)r;
    }
}

char *strstr(const char *h, const char *n) {
    size_t len = strlen(n);
    if (!len) return (char *)h;
    for (; *h; h++)
        if (*h == *n && !strncmp(h, n, len)) return (char *)h;
    return NULL;
}

char *strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *d = malloc(n);
    if (d) memcpy(d, s, n);
    return d;
}

char *strerror(int e) { (void)e; return "error"; }

size_t strspn(const char *s, const char *accept) {
    size_t i = 0;
    while (s[i] && strchr(accept, s[i])) i++;
    return i;
}

size_t strcspn(const char *s, const char *reject) {
    size_t i = 0;
    while (s[i] && !strchr(reject, s[i])) i++;
    return i;
}

char *strtok(char *s, const char *delim) {
    static char *next;
    if (!s) s = next;
    if (!s) return NULL;
    s += strspn(s, delim);
    if (!*s) return next = NULL;
    char *end = s + strcspn(s, delim);
    if (*end) *end++ = 0; else end = NULL;
    next = end;
    return s;
}

int isspace(int c) { return c == ' ' || (c >= '\t' && c <= '\r'); }
int isdigit(int c) { return c >= '0' && c <= '9'; }
int isxdigit(int c) { return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
int isupper(int c) { return c >= 'A' && c <= 'Z'; }
int islower(int c) { return c >= 'a' && c <= 'z'; }
int isalpha(int c) { return isupper(c) || islower(c); }
int isalnum(int c) { return isalpha(c) || isdigit(c); }
int isprint(int c) { return c >= 0x20 && c < 0x7f; }
int iscntrl(int c) { return (c >= 0 && c < 0x20) || c == 0x7f; }
int ispunct(int c) { return isprint(c) && !isalnum(c) && c != ' '; }
int toupper(int c) { return islower(c) ? c - 32 : c; }
int tolower(int c) { return isupper(c) ? c + 32 : c; }

/* ---- Numbers --------------------------------------------------------------------------- */

int abs(int x) { return x < 0 ? -x : x; }
long labs(long x) { return x < 0 ? -x : x; }

unsigned long strtoul(const char *s, char **end, int base) {
    while (isspace((unsigned char)*s)) s++;
    int neg = 0;
    if (*s == '+' || *s == '-') neg = *s++ == '-';
    if ((base == 0 || base == 16) && s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
        s += 2;
        base = 16;
    } else if (base == 0) {
        base = s[0] == '0' ? 8 : 10;
    }
    unsigned long v = 0;
    for (;; s++) {
        int d;
        if (isdigit((unsigned char)*s)) d = *s - '0';
        else if (isalpha((unsigned char)*s)) d = tolower((unsigned char)*s) - 'a' + 10;
        else break;
        if (d >= base) break;
        v = v * base + d;
    }
    if (end) *end = (char *)s;
    return neg ? -v : v;
}

long strtol(const char *s, char **end, int base) { return (long)strtoul(s, end, base); }
int atoi(const char *s) { return (int)strtol(s, NULL, 10); }
long atol(const char *s) { return strtol(s, NULL, 10); }

double strtod(const char *s, char **end) {
    while (isspace((unsigned char)*s)) s++;
    int neg = 0;
    if (*s == '+' || *s == '-') neg = *s++ == '-';
    double v = 0;
    while (isdigit((unsigned char)*s)) v = v * 10 + (*s++ - '0');
    if (*s == '.') {
        double f = 0.1;
        for (s++; isdigit((unsigned char)*s); s++, f /= 10) v += (*s - '0') * f;
    }
    if (end) *end = (char *)s;
    return neg ? -v : v;
}

double atof(const char *s) { return strtod(s, NULL); }

static unsigned rand_state = 1;
int rand(void) { rand_state = rand_state * 1103515245 + 12345; return (rand_state >> 16) & RAND_MAX; }
void srand(unsigned seed) { rand_state = seed; }

void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *)) {
    /* Insertion sort: Doom sorts short lists. */
    char *a = base;
    char tmp[256];
    if (size > sizeof tmp) __builtin_trap();
    for (size_t i = 1; i < n; i++) {
        size_t j = i;
        memcpy(tmp, a + i * size, size);
        while (j > 0 && cmp(a + (j - 1) * size, tmp) > 0) {
            memcpy(a + j * size, a + (j - 1) * size, size);
            j--;
        }
        memcpy(a + j * size, tmp, size);
    }
}

/* ---- Math (only a few start-up tables use these) --------------------------------------- */

double fabs(double x) { return x < 0 ? -x : x; }
double sqrt(double x) { return __builtin_sqrt(x); }
double floor(double x) { return __builtin_floor(x); }
double ceil(double x) { return __builtin_ceil(x); }

double fmod(double x, double y) {
    if (y == 0) return 0;
    double q = x / y;
    q = q < 0 ? ceil(q) : floor(q);
    return x - q * y;
}

double sin(double x) {
    x = fmod(x, 2 * M_PI);
    if (x > M_PI) x -= 2 * M_PI;
    if (x < -M_PI) x += 2 * M_PI;
    double term = x, sum = x, x2 = x * x;
    for (int i = 1; i < 20; i++) {
        term *= -x2 / ((2 * i) * (2 * i + 1));
        sum += term;
    }
    return sum;
}

double cos(double x) { return sin(x + M_PI / 2); }
double tan(double x) { return sin(x) / cos(x); }

double atan(double x) {
    /* Reduce to |x| <= 0.5, then the series. */
    if (x < 0) return -atan(-x);
    if (x > 1) return M_PI / 2 - atan(1 / x);
    if (x > 0.5) return M_PI / 4 + atan((x - 1) / (x + 1));
    double term = x, sum = x, x2 = x * x;
    for (int i = 1; i < 40; i++) {
        term *= -x2;
        sum += term / (2 * i + 1);
    }
    return sum;
}

double atan2(double y, double x) {
    if (x > 0) return atan(y / x);
    if (x < 0) return y >= 0 ? atan(y / x) + M_PI : atan(y / x) - M_PI;
    return y > 0 ? M_PI / 2 : y < 0 ? -M_PI / 2 : 0;
}

double pow(double x, double y) {
    int n = (int)y;
    if (n != y) return 0; /* not needed */
    double r = 1;
    for (int i = 0; i < (n < 0 ? -n : n); i++) r *= x;
    return n < 0 ? 1 / r : r;
}

/* ---- Formatting ------------------------------------------------------------------------ */

typedef struct {
    char *buf;
    size_t cap, len;
} out_t;

static void put(out_t *o, char c) {
    if (o->len + 1 < o->cap) o->buf[o->len] = c;
    o->len++;
}

static void put_padded(out_t *o, const char *s, size_t n, int width, int left, char pad) {
    int fill = width > (int)n ? width - (int)n : 0;
    if (!left)
        while (fill--) put(o, pad);
    for (size_t i = 0; i < n; i++) put(o, s[i]);
    if (left)
        while (fill-- > 0) put(o, ' ');
}

int vsnprintf(char *buf, size_t cap, const char *fmt, va_list ap) {
    out_t o = {buf, cap, 0};
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            put(&o, *fmt);
            continue;
        }
        fmt++;
        int left = 0, plus = 0, space = 0, zero = 0, alt = 0;
        for (;; fmt++) {
            if (*fmt == '-') left = 1;
            else if (*fmt == '+') plus = 1;
            else if (*fmt == ' ') space = 1;
            else if (*fmt == '0') zero = 1;
            else if (*fmt == '#') alt = 1;
            else break;
        }
        int width = 0, prec = -1;
        if (*fmt == '*') { width = va_arg(ap, int); fmt++; if (width < 0) { left = 1; width = -width; } }
        else while (isdigit((unsigned char)*fmt)) width = width * 10 + (*fmt++ - '0');
        if (*fmt == '.') {
            fmt++;
            prec = 0;
            if (*fmt == '*') { prec = va_arg(ap, int); fmt++; }
            else while (isdigit((unsigned char)*fmt)) prec = prec * 10 + (*fmt++ - '0');
        }
        int lng = 0;
        while (*fmt == 'l' || *fmt == 'h' || *fmt == 'z' || *fmt == 'j' || *fmt == 't') {
            if (*fmt == 'l' || *fmt == 'z' || *fmt == 'j' || *fmt == 't') lng++;
            fmt++;
        }
        char tmp[64];
        char *p = tmp + sizeof tmp;
        size_t n;
        switch (*fmt) {
        case 'd': case 'i': case 'u': case 'x': case 'X': case 'o': case 'p': {
            unsigned long long v;
            int neg = 0;
            int base = *fmt == 'x' || *fmt == 'X' || *fmt == 'p' ? 16 : *fmt == 'o' ? 8 : 10;
            if (*fmt == 'p') v = (uintptr_t)va_arg(ap, void *);
            else if (*fmt == 'd' || *fmt == 'i') {
                long long s = lng >= 2 ? va_arg(ap, long long) : lng ? va_arg(ap, long) : va_arg(ap, int);
                neg = s < 0;
                v = neg ? -(unsigned long long)s : (unsigned long long)s;
            } else v = lng >= 2 ? va_arg(ap, unsigned long long) : lng ? va_arg(ap, unsigned long) : va_arg(ap, unsigned);
            const char *digits = *fmt == 'X' ? "0123456789ABCDEF" : "0123456789abcdef";
            do { *--p = digits[v % base]; v /= base; } while (v);
            while (prec > 0 && (tmp + sizeof tmp - p) < prec) *--p = '0';
            if (alt && base == 16 && *fmt != 'p') { *--p = *fmt; *--p = '0'; }
            n = tmp + sizeof tmp - p;
            char sign = neg ? '-' : plus ? '+' : space ? ' ' : 0;
            if (sign && zero && !left) {
                put(&o, sign);
                put_padded(&o, p, n, width - 1, 0, '0');
            } else {
                if (sign) { *--p = sign; n++; }
                put_padded(&o, p, n, width, left, zero && !left && prec < 0 ? '0' : ' ');
            }
            break;
        }
        case 'f': case 'g': case 'e': {
            double v = va_arg(ap, double);
            if (prec < 0) prec = 6;
            int neg = v < 0;
            if (neg) v = -v;
            unsigned long long ip = (unsigned long long)v;
            double frac = v - (double)ip;
            char *q = tmp;
            if (neg) *q++ = '-';
            char ib[24];
            int k = 0;
            do { ib[k++] = '0' + ip % 10; ip /= 10; } while (ip && k < 20);
            while (k) *q++ = ib[--k];
            if (prec > 0) {
                *q++ = '.';
                for (int i = 0; i < prec && i < 20; i++) {
                    frac *= 10;
                    int d = (int)frac;
                    *q++ = '0' + d;
                    frac -= d;
                }
            }
            put_padded(&o, tmp, q - tmp, width, left, ' ');
            break;
        }
        case 's': {
            const char *s = va_arg(ap, const char *);
            if (!s) s = "(null)";
            n = prec >= 0 ? strnlen(s, prec) : strlen(s);
            put_padded(&o, s, n, width, left, ' ');
            break;
        }
        case 'c': {
            char c = (char)va_arg(ap, int);
            put_padded(&o, &c, 1, width, left, ' ');
            break;
        }
        case '%': put(&o, '%'); break;
        case 0: fmt--; break;
        default: put(&o, '%'); put(&o, *fmt); break;
        }
    }
    if (cap) buf[o.len < cap ? o.len : cap - 1] = 0;
    return (int)o.len;
}

int vsprintf(char *buf, const char *fmt, va_list ap) { return vsnprintf(buf, (size_t)-1 >> 1, fmt, ap); }

int snprintf(char *buf, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vsnprintf(buf, n, fmt, ap);
    va_end(ap);
    return r;
}

int sprintf(char *buf, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vsprintf(buf, fmt, ap);
    va_end(ap);
    return r;
}

/* Console output goes to a small log the host can read (for errors). */
static char log_buf[8192];
static size_t log_len;

static void log_write(const char *s, size_t n) {
    for (size_t i = 0; i < n; i++) {
        if (log_len == sizeof log_buf) {
            memmove(log_buf, log_buf + sizeof log_buf / 2, sizeof log_buf / 2);
            log_len = sizeof log_buf / 2;
        }
        log_buf[log_len++] = s[i];
    }
}

const char *cs_log(size_t *len) { *len = log_len; return log_buf; }

int vfprintf(FILE *f, const char *fmt, va_list ap) {
    char tmp[1024];
    int n = vsnprintf(tmp, sizeof tmp, fmt, ap);
    if (f == stdout || f == stderr) log_write(tmp, n < (int)sizeof tmp ? (size_t)n : sizeof tmp - 1);
    else fwrite(tmp, 1, n < (int)sizeof tmp ? (size_t)n : sizeof tmp - 1, f);
    return n;
}

int vprintf(const char *fmt, va_list ap) { return vfprintf(stdout, fmt, ap); }

int printf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfprintf(stdout, fmt, ap);
    va_end(ap);
    return r;
}

int fprintf(FILE *f, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfprintf(f, fmt, ap);
    va_end(ap);
    return r;
}

int puts(const char *s) { log_write(s, strlen(s)); log_write("\n", 1); return 0; }
int putchar(int c) { char ch = (char)c; log_write(&ch, 1); return c; }
void perror(const char *s) { puts(s); }

/* Enough of sscanf for Doom: %d %i %u %x %s %c and literal text. */
int sscanf(const char *s, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int count = 0;
    for (; *fmt; fmt++) {
        if (isspace((unsigned char)*fmt)) {
            while (isspace((unsigned char)*s)) s++;
            continue;
        }
        if (*fmt != '%') {
            if (*s != *fmt) break;
            s++;
            continue;
        }
        fmt++;
        int width = 0;
        while (isdigit((unsigned char)*fmt)) width = width * 10 + (*fmt++ - '0');
        while (*fmt == 'l' || *fmt == 'h') fmt++;
        if (*fmt != 'c')
            while (isspace((unsigned char)*s)) s++;
        if (!*s) break;
        char *end;
        if (*fmt == 'd' || *fmt == 'i' || *fmt == 'u' || *fmt == 'x') {
            long v = strtol(s, &end, *fmt == 'x' ? 16 : *fmt == 'i' ? 0 : 10);
            if (end == s) break;
            *va_arg(ap, int *) = (int)v;
            s = end;
        } else if (*fmt == 's') {
            char *d = va_arg(ap, char *);
            int n = 0;
            while (*s && !isspace((unsigned char)*s) && (!width || n < width)) d[n++] = *s++;
            d[n] = 0;
        } else if (*fmt == 'c') {
            *va_arg(ap, char *) = *s++;
        } else break;
        count++;
    }
    va_end(ap);
    return count;
}

/* ---- Files in memory ------------------------------------------------------------------- */

#define MAX_FILES 32

typedef struct {
    char name[96];
    unsigned char *data;
    size_t size, cap;
    int readonly, used;
} memfile_t;

struct cs_file {
    memfile_t *file;
    size_t pos;
    int write, eof, used, console;
};

static memfile_t files[MAX_FILES];
static struct cs_file handles[MAX_FILES];
static struct cs_file console_handles[3] = {{0, 0, 1, 0, 1, 1}, {0, 0, 1, 0, 1, 1}, {0, 0, 1, 0, 1, 1}};
FILE *stdin = &console_handles[0], *stdout = &console_handles[1], *stderr = &console_handles[2];

/* Names compare by their last path component: Doom builds paths from directories. */
static const char *base_name(const char *path) {
    const char *b = path;
    for (const char *p = path; *p; p++)
        if (*p == '/' || *p == '\\') b = p + 1;
    return b;
}

static memfile_t *find_file(const char *name) {
    name = base_name(name);
    for (int i = 0; i < MAX_FILES; i++)
        if (files[i].used && !strcasecmp(files[i].name, name)) return &files[i];
    return NULL;
}

static memfile_t *new_file(const char *name) {
    for (int i = 0; i < MAX_FILES; i++) {
        if (!files[i].used) {
            memset(&files[i], 0, sizeof files[i]);
            files[i].used = 1;
            strncpy(files[i].name, base_name(name), sizeof files[i].name - 1);
            return &files[i];
        }
    }
    return NULL;
}

void cs_fs_add(const char *name, const void *data, size_t size, int readonly) {
    memfile_t *f = find_file(name);
    if (!f) f = new_file(name);
    if (!f) __builtin_trap();
    if (!f->readonly && f->data) free(f->data);
    if (readonly) {
        f->data = (unsigned char *)data;
        f->cap = size;
    } else {
        f->data = malloc(size ? size : 1);
        f->cap = size;
        memcpy(f->data, data, size);
    }
    f->size = size;
    f->readonly = readonly;
}

const unsigned char *cs_fs_get(const char *name, size_t *size) {
    memfile_t *f = find_file(name);
    if (!f) return NULL;
    *size = f->size;
    return f->data;
}

FILE *fopen(const char *name, const char *mode) {
    int write = strchr(mode, 'w') || strchr(mode, 'a');
    memfile_t *f = find_file(name);
    if (!f && !write) { errno = ENOENT; return NULL; }
    if (write) {
        if (f && f->readonly) { errno = EINVAL; return NULL; }
        if (!f) f = new_file(name);
        if (!f) return NULL;
        if (strchr(mode, 'w')) f->size = 0;
    }
    for (int i = 0; i < MAX_FILES; i++) {
        if (!handles[i].used) {
            handles[i] = (struct cs_file){f, strchr(mode, 'a') ? f->size : 0, write, 0, 1, 0};
            return &handles[i];
        }
    }
    return NULL;
}

int fclose(FILE *h) {
    if (h && !h->console) h->used = 0;
    return 0;
}

size_t fread(void *p, size_t size, size_t n, FILE *h) {
    if (!h || h->console || !size) return 0;
    memfile_t *f = h->file;
    size_t avail = h->pos < f->size ? f->size - h->pos : 0;
    size_t want = size * n;
    if (want > avail) { want = avail - avail % size; h->eof = 1; }
    memcpy(p, f->data + h->pos, want);
    h->pos += want;
    return want / size;
}

size_t fwrite(const void *p, size_t size, size_t n, FILE *h) {
    if (!h || !size) return 0;
    size_t len = size * n;
    if (h->console) { log_write(p, len); return n; }
    memfile_t *f = h->file;
    if (!h->write || f->readonly) return 0;
    if (h->pos + len > f->cap) {
        size_t cap = f->cap ? f->cap * 2 : 4096;
        while (cap < h->pos + len) cap *= 2;
        unsigned char *d = realloc(f->data, cap);
        if (!d) return 0;
        f->data = d;
        f->cap = cap;
    }
    memcpy(f->data + h->pos, p, len);
    h->pos += len;
    if (h->pos > f->size) f->size = h->pos;
    return n;
}

int fseek(FILE *h, long off, int whence) {
    if (!h || h->console) return -1;
    long base = whence == SEEK_SET ? 0 : whence == SEEK_CUR ? (long)h->pos : (long)h->file->size;
    if (base + off < 0) return -1;
    h->pos = base + off;
    h->eof = 0;
    return 0;
}

long ftell(FILE *h) { return h && !h->console ? (long)h->pos : -1; }
int fflush(FILE *h) { (void)h; return 0; }
int feof(FILE *h) { return h ? h->eof : 1; }
int ferror(FILE *h) { (void)h; return 0; }
int fileno(FILE *h) { return h == stdin ? 0 : h == stdout ? 1 : h == stderr ? 2 : 3; }
int fputs(const char *s, FILE *h) { return fwrite(s, 1, strlen(s), h) ? 0 : EOF; }
int fputc(int c, FILE *h) { unsigned char ch = (unsigned char)c; return fwrite(&ch, 1, 1, h) ? c : EOF; }
int putc(int c, FILE *h) { return fputc(c, h); }
int fgetc(FILE *h) { unsigned char c; return fread(&c, 1, 1, h) ? c : EOF; }
int getc(FILE *h) { return fgetc(h); }

char *fgets(char *s, int n, FILE *h) {
    int i = 0;
    while (i < n - 1) {
        int c = fgetc(h);
        if (c == EOF) break;
        s[i++] = (char)c;
        if (c == '\n') break;
    }
    if (!i) return NULL;
    s[i] = 0;
    return s;
}

int remove(const char *name) {
    memfile_t *f = find_file(name);
    if (!f || f->readonly) return -1;
    free(f->data);
    f->used = 0;
    return 0;
}

int rename(const char *from, const char *to) {
    memfile_t *f = find_file(from);
    if (!f) return -1;
    if (find_file(to) && find_file(to) != f) remove(to);
    memset(f->name, 0, sizeof f->name);
    strncpy(f->name, base_name(to), sizeof f->name - 1);
    return 0;
}

int stat(const char *path, struct stat *st) {
    memfile_t *f = find_file(path);
    if (!f) return -1;
    st->st_size = f->size;
    st->st_mode = 0;
    return 0;
}

int access(const char *path, int mode) { (void)mode; return find_file(path) ? 0 : -1; }
int mkdir(const char *path, mode_t mode) { (void)path; (void)mode; return 0; }
int open(const char *path, int flags, ...) { (void)path; (void)flags; return -1; }
int close(int fd) { (void)fd; return 0; }
int read(int fd, void *buf, size_t n) { (void)fd; (void)buf; (void)n; return -1; }
int write(int fd, const void *buf, size_t n) { (void)fd; log_write(buf, n); return (int)n; }
char *getcwd(char *buf, size_t n) { if (n) buf[0] = 0; return buf; }
int isatty(int fd) { (void)fd; return 0; }

/* ---- The rest --------------------------------------------------------------------------- */

_Noreturn void exit(int code) { (void)code; __builtin_trap(); }
_Noreturn void abort(void) { __builtin_trap(); }
int atexit(void (*fn)(void)) { (void)fn; return 0; }
char *getenv(const char *name) { (void)name; return NULL; }
int system(const char *cmd) { (void)cmd; return -1; }
sighandler_t signal(int sig, sighandler_t h) { (void)sig; (void)h; return SIG_DFL; }
int usleep(unsigned usec) { (void)usec; return 0; }
unsigned sleep(unsigned sec) { (void)sec; return 0; }
time_t time(time_t *t) { if (t) *t = 0; return 0; }

int gettimeofday(struct timeval *tv, void *tz) {
    (void)tz;
    uint32_t ms = cs_clock_ms();
    tv->tv_sec = ms / 1000;
    tv->tv_usec = (ms % 1000) * 1000;
    return 0;
}
