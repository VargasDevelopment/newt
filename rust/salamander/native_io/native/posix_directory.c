/* SPDX-License-Identifier: MIT
 * One native directory read. All traversal and name policy stay in Rust. */
#ifndef ESP_PLATFORM
#define _POSIX_C_SOURCE 200809L
#ifndef _DEFAULT_SOURCE
#define _DEFAULT_SOURCE 1
#endif
#endif
#include "native_io.h"
#include <dirent.h>
#include <errno.h>

int32_t newt_io_readdir_entry(void *directory,const char **name,uint32_t *kind) {
    errno=0;
    const struct dirent *entry=readdir((DIR *)directory);
    if (!entry && errno) return -1;
    *name=entry ? entry->d_name : NULL;
    if (kind) *kind=!entry || entry->d_type==DT_UNKNOWN ? 0 :
        entry->d_type==DT_DIR ? 1 : 2;
    return 0;
}
int32_t newt_io_readdir_name(void *directory,const char **name) {
    return newt_io_readdir_entry(directory,name,NULL);
}
