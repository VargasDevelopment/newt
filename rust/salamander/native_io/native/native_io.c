/* MIT. Stateless forwarding; one POSIX/VFS call, never a retry. */
#ifndef ESP_PLATFORM
#define _POSIX_C_SOURCE 200809L
#endif
#include "native_io.h"
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
_Static_assert(sizeof(int) == 4, "fd ABI");
_Static_assert(sizeof(struct newt_io_stat) == 40, "wire size");
_Static_assert(offsetof(struct newt_io_stat, size) == 0, "size offset");
_Static_assert(offsetof(struct newt_io_stat, inode) == 8, "inode offset");
_Static_assert(offsetof(struct newt_io_stat, device) == 16, "device offset");
_Static_assert(offsetof(struct newt_io_stat, seconds) == 24, "seconds offset");
_Static_assert(offsetof(struct newt_io_stat, nanos) == 32, "nanos offset");
_Static_assert(offsetof(struct newt_io_stat, kind) == 36, "kind offset");
static int32_t failure(int error) {
    switch (error) {
    case EINTR: return N_INTERRUPTED;
    case ENOENT: return N_MISSING;
    case EEXIST: return N_EXISTS;
    case ECANCELED: return N_CANCELLED;
    /* The reading-state store reports a missing or failed medium. */
    case EIO: case ENODEV: case ENXIO: return N_UNAVAILABLE;
    default: return N_OTHER;
    }
}
static int32_t status(int result) { return result < 0 ? failure(errno) : N_OK; }
int32_t newt_io_open(const char *path, uint32_t mode, int32_t *out) {
    int flags;
    switch (mode) {
    case 0: flags = O_RDONLY; break;
    case 1: flags = O_RDONLY | O_NONBLOCK; break;
    case 2: flags = O_RDWR | O_NONBLOCK; break;
    case 3: flags = O_RDWR | O_CREAT | O_EXCL; break;
    case 4: flags = O_RDWR; break;
    case 5: flags = O_WRONLY | O_CREAT | O_EXCL; break;
    case 6: flags = O_WRONLY | O_CREAT | O_TRUNC; break;
    default: return N_OTHER;
    }
    const int fd = open(path, flags, 0600);
    if (fd < 0) return failure(errno);
    *out = fd;
    return N_OK;
}
int32_t newt_io_close(int32_t fd) { return status(close(fd)); }
int32_t newt_io_read(int32_t fd, uint8_t *data, size_t bytes, size_t *out) {
    const ssize_t result = read(fd, data, bytes);
    if (result < 0) return failure(errno);
    *out = (size_t)result;
    return N_OK;
}
int32_t newt_io_write(int32_t fd, const uint8_t *data, size_t bytes, size_t *out) {
    const ssize_t result = write(fd, data, bytes);
    if (result < 0) return failure(errno);
    *out = (size_t)result;
    return N_OK;
}
int32_t newt_io_seek(int32_t fd, uint32_t at, uint64_t *out) {
    /* Match pinned explicit off_t conversion, including RV32 signed range. */
    const off_t result = lseek(fd, (off_t)at, SEEK_SET);
    if (result < 0) return failure(errno);
    *out = (uint64_t)result;
    return N_OK;
}
int64_t newt_io_seek_end(int32_t fd) {
    return (int64_t)(long)lseek(fd, 0, SEEK_END);
}
int64_t newt_io_seek_set_raw(int32_t fd, uint32_t at, int64_t *expected) {
    *expected = (int64_t)(off_t)at;
    return (int64_t)lseek(fd, (off_t)at, SEEK_SET);
}
int32_t newt_io_truncate(int32_t fd, uint32_t bytes) {
    return status(ftruncate(fd, (off_t)bytes));
}
int32_t newt_io_sync(int32_t fd) { return status(fsync(fd)); }
static void convert(const struct stat *in, struct newt_io_stat *out) {
    out->size = (int64_t)in->st_size;
    out->inode = (uint64_t)in->st_ino;
    out->device = (uint64_t)in->st_dev;
    out->seconds = (int64_t)in->st_mtime;
#ifdef ESP_PLATFORM
    out->nanos = 0;
#else
    out->nanos = (uint32_t)in->st_mtim.tv_nsec;
#endif
    out->kind = S_ISREG(in->st_mode) ? 1 : S_ISDIR(in->st_mode) ? 2 : 0;
}
int32_t newt_io_fstat(int32_t fd, struct newt_io_stat *out) {
    struct stat info = {0};
    if (fstat(fd, &info) < 0) return failure(errno);
    convert(&info, out);
    return N_OK;
}
int32_t newt_io_stat(const char *path, uint32_t no_follow, struct newt_io_stat *out) {
    struct stat info = {0};
#ifdef ESP_PLATFORM
    /* Pinned FAT VFS has no symlinks/lstat: sd_cache_syscalls.h. */
    (void)no_follow;
    const int result = stat(path, &info);
#else
    const int result = no_follow ? lstat(path, &info) : stat(path, &info);
#endif
    if (result < 0) return failure(errno);
    convert(&info, out);
    return N_OK;
}
int32_t newt_io_mkdir(const char *path) { return status(mkdir(path, 0700)); }
int32_t newt_io_unlink(const char *path) { return status(unlink(path)); }
int32_t newt_io_rmdir(const char *path) { return status(rmdir(path)); }
int32_t newt_io_rename(const char *from, const char *to) {
    return status(rename(from, to));
}
int32_t newt_io_opendir(const char *path, void **out) {
    DIR *dir = opendir(path);
    if (!dir) return failure(errno);
    *out = dir;
    return N_OK;
}
/* No intervening SDK call may replace the failing operation's errno. */
static int32_t raw_errno(int32_t result) {
    const int error = errno;
    return result == N_OK ? 0 : error ? error : -1;
}
int32_t newt_io_readonly_open_errno(const char *path, int32_t *out) {
    errno = 0;
    const int32_t result = newt_io_open(path, 0, out);
    return raw_errno(result);
}
int32_t newt_io_readonly_stat_errno(const char *path, struct newt_io_stat *out) {
    errno = 0;
    const int32_t result = newt_io_stat(path, 0, out);
    return raw_errno(result);
}
int32_t newt_io_readonly_opendir_errno(const char *path, void **out) {
    errno = 0;
    const int32_t result = newt_io_opendir(path, out);
    return raw_errno(result);
}
_Static_assert(ENOENT == 2 && ENOTDIR == 20 && EINTR == 4, "read-only errno ABI");
/* errno is cleared first: readdir returns NULL both at the end and on failure. */
int32_t newt_io_readdir(void *dir, char *name, size_t capacity, size_t *length, uint32_t *more) {
    errno = 0;
    const struct dirent *entry = readdir((DIR *)dir);
    if (!entry) {
        if (errno) return failure(errno);
        *more = 0;
        return N_OK;
    }
    const size_t n = strlen(entry->d_name);
    if (capacity) memcpy(name, entry->d_name, n < capacity ? n : capacity);
    *length = n;
    *more = 1;
    return N_OK;
}
int32_t newt_io_closedir(void *dir) { return status(closedir((DIR *)dir)); }
#ifdef ESP_PLATFORM
#include "sdkconfig.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#if !CONFIG_IDF_TARGET_ESP32C3 || !CONFIG_FREERTOS_UNICORE
#error "Nonce exclusion is reviewed only for single-core ESP32-C3"
#endif
void newt_io_nonce_enter(void) { taskENTER_CRITICAL(NULL); }
void newt_io_nonce_exit(void) { taskEXIT_CRITICAL(NULL); }
#endif
