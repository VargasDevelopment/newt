// SPDX-License-Identifier: MIT
//! SDK leaves only; HTTPS and frame-loan decisions remain in Rust.
use core::ffi::{c_char, c_void};

// SAFETY: these callbacks use the SDK C ABI. Allocator inputs follow calloc/
// free ownership; timer context is a deadline encoded by value, never a loan.
pub type Calloc = unsafe extern "C" fn(usize, usize) -> *mut c_void;
// SAFETY: only the matching allocator's original pointer is passed to free.
pub type Free = unsafe extern "C" fn(*mut c_void);
// SAFETY: the timer receives only an integer deadline, not an owner pointer.
pub type Sweep = unsafe extern "C" fn(*mut c_void);

// SAFETY: the matching native file forwards these exact SDK ABIs. Client and
// timer handles are uniquely owned; buffer/context loans last one synchronous
// call. The opaque TLS header lives exclusively inside the lent logical frame.
unsafe extern "C" {
    pub fn nh_http_init(url: *const c_char, timeout: u32) -> *mut c_void;
    pub fn nh_http_set_header(client: *mut c_void, key: *const c_char, value: *const c_char) -> bool;
    pub fn nh_http_open(client: *mut c_void) -> bool;
    pub fn nh_http_set_context(client: *mut c_void, context: *mut c_void) -> bool;
    pub fn nh_http_headers(client: *mut c_void) -> i64;
    pub fn nh_http_status(client: *mut c_void) -> i32;
    pub fn nh_http_read(client: *mut c_void, bytes: *mut u8, count: i32) -> i32;
    pub fn nh_http_complete(client: *mut c_void) -> bool;
    pub fn nh_http_close(client: *mut c_void);
    pub fn nh_http_cleanup(client: *mut c_void);
    pub fn nh_timer_create(callback: Sweep, deadline: *mut c_void) -> *mut c_void;
    pub fn nh_timer_start(timer: *mut c_void, interval: u64) -> bool;
    pub fn nh_timer_stop(timer: *mut c_void);
    pub fn nh_timer_delete(timer: *mut c_void);
    pub fn nh_socket_first() -> i32;
    pub fn nh_socket_count() -> i32;
    pub fn nh_socket_shutdown(fd: i32);
    pub fn nh_socket_sem_deinit();
    pub fn nb_millis() -> u32;
    pub fn nh_tls_header_bytes() -> usize;
    pub fn nh_tls_construct(header: *mut c_void);
    pub fn nh_tls_register(header: *mut c_void, start: *mut u8, bytes: usize) -> bool;
    pub fn nh_tls_bind_task_lock(header: *mut c_void);
    pub fn nh_tls_owner_is_current(header: *mut c_void) -> bool;
    pub fn nh_tls_malloc(header: *mut c_void, bytes: usize) -> *mut c_void;
    pub fn nh_tls_free(header: *mut c_void, bytes: *mut c_void);
    pub fn nh_tls_info(header: *mut c_void, allocated: *mut usize, lowest: *mut usize);
    pub fn nh_tls_install(calloc: Calloc, free: Free) -> i32;
    pub fn nh_tls_restore() -> i32;
    pub fn nh_tls_default_calloc(count: usize, size: usize) -> *mut c_void;
    pub fn nh_tls_default_free(bytes: *mut c_void);
}
