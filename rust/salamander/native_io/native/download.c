/* SPDX-License-Identifier: MIT */
/* One POSIX call per leaf; Rust owns write loops and every failure decision. */
#ifndef ESP_PLATFORM
#define _POSIX_C_SOURCE 200809L
#endif
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
_Static_assert(ENOENT == 2 && EEXIST == 17, "raw errno ABI");
int32_t newt_download_mkdir(const char* path, uint32_t mode) {
    const int result = mkdir(path, (mode_t)mode);
    return result < 0 ? errno : 0;
}
int32_t newt_download_unlink(const char* path) {
    const int result = unlink(path);
    return result < 0 ? errno : 0;
}
int32_t newt_download_open(const char* path, uint32_t mode, int32_t* fd) {
    const int result = open(path, O_WRONLY | O_CREAT | O_TRUNC, (mode_t)mode);
    if (result < 0) return errno;
    *fd = result;
    return 0;
}
int32_t newt_download_write(int32_t fd, const uint8_t* bytes, size_t count, size_t* written) {
    const ssize_t result = write(fd, bytes, count);
    if (result < 0) return errno;
    *written = (size_t)result;
    return 0;
}
int32_t newt_download_sync(int32_t fd) {
    const int result = fsync(fd);
    return result < 0 ? errno : 0;
}
int32_t newt_download_close(int32_t fd) {
    const int result = close(fd);
    return result < 0 ? errno : 0;
}
uint32_t newt_download_millis(void) {
    struct timespec now = {0};
    clock_gettime(CLOCK_MONOTONIC, &now);
    return (uint32_t)(now.tv_sec * 1000 + now.tv_nsec / 1000000);
}
