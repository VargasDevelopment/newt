/* MIT. Private wire ABI. Never export native struct stat. */
#ifndef NEWT_NATIVE_IO_H
#define NEWT_NATIVE_IO_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
enum { N_OK, N_INTERRUPTED, N_MISSING, N_EXISTS, N_CANCELLED, N_OTHER, N_UNAVAILABLE };
struct newt_io_stat {
    int64_t size;
    uint64_t inode, device;
    int64_t seconds;
    uint32_t nanos, kind; /* kind: 0 other, 1 regular, 2 directory */
};
int32_t newt_io_open(const char *, uint32_t, int32_t *);
/* 0 success, positive raw errno, -1 failure without errno. Read-only views
   preserve ENOTDIR; no retry or import policy runs in these leaves. */
int32_t newt_io_readonly_open_errno(const char *, int32_t *);
int32_t newt_io_readonly_stat_errno(const char *, struct newt_io_stat *);
int32_t newt_io_readonly_opendir_errno(const char *, void **);
int32_t newt_io_close(int32_t);
int32_t newt_io_read(int32_t, uint8_t *, size_t, size_t *);
int32_t newt_io_write(int32_t, const uint8_t *, size_t, size_t *);
int32_t newt_io_seek(int32_t, uint32_t, uint64_t *);
/* LibraryFiles::size: raw native-long result and actual SEEK_END cursor. */
int64_t newt_io_seek_end(int32_t);
/* Raw lseek and the native off_t operand; comparison remains in Rust. */
int64_t newt_io_seek_set_raw(int32_t, uint32_t, int64_t *);
int32_t newt_io_truncate(int32_t, uint32_t);
int32_t newt_io_sync(int32_t);
int32_t newt_io_fstat(int32_t, struct newt_io_stat *);
int32_t newt_io_stat(const char *, uint32_t, struct newt_io_stat *);
int32_t newt_io_mkdir(const char *);
int32_t newt_io_unlink(const char *);
int32_t newt_io_rmdir(const char *);
int32_t newt_io_rename(const char *, const char *);
/* Opaque DIR*, owned by the caller until newt_io_closedir. */
int32_t newt_io_opendir(const char *, void **);
/* N_OK with *more 0 at the end; else *length is the full name length and
   min(length, capacity) bytes were copied, without a terminator. */
int32_t newt_io_readdir(void *, char *, size_t, size_t *, uint32_t *);
/* Borrow the actual dirent name until this same DIR's next read or close.
   Zero with a null name means EOF; nonzero is the actual read failure. */
int32_t newt_io_readdir_name(void *, const char **);
/* Same dirent loan; kind 0 unknown, 1 directory, 2 other. */
int32_t newt_io_readdir_entry(void *, const char **, uint32_t *);
int32_t newt_io_closedir(void *);
/* One synchronous borrow; NULL name with zero length ends the directory.
   Visitor 0 declines; 1 continues. No callback or name survives this call. */
typedef uint32_t (*newt_io_page_visit)(void *, const uint8_t *, size_t,
                                     uint64_t, int64_t, uint32_t);
int32_t newt_io_walk_pages(const char *, uint32_t, newt_io_page_visit, void *);
/* ESP32 FAT Library facts. Native state and names live only inside this
   synchronous callback. Rust owns traversal and each explicit close. */
struct newt_io_library_entry {
    const uint8_t *name;
    size_t length;
    uint32_t bytes, folder, hidden;
};
typedef uint32_t (*newt_io_library_scope_callback)(void *, void *);
uint32_t newt_io_library_scope(uint32_t, newt_io_library_scope_callback, void *);
uint32_t newt_io_library_open(void *, uint32_t, const char *);
/* 0 failure, 1 end, 2 entry, with the full name extent in FILINFO. */
uint32_t newt_io_library_next(void *, uint32_t, struct newt_io_library_entry *);
uint32_t newt_io_library_close(void *, uint32_t);
void newt_io_nonce_enter(void);
void newt_io_nonce_exit(void);
#ifdef __cplusplus
}
#endif
#endif
