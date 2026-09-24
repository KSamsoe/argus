//! The eight WASI calls wasi-libc makes, answered inside the module, so the
//! page supplies nothing: no environment, a clock, stdout and stderr to the
//! console, and nothing else.
#![allow(clippy::missing_safety_doc)]

const EBADF: i32 = 8;

#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_sizes_get(
    count: *mut u32,
    size: *mut u32,
) -> i32 {
    *count = 0;
    *size = 0;
    0
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_environ_get(
    _: *mut u32,
    _: *mut u8,
) -> i32 {
    0
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_clock_time_get(
    _id: u32,
    _precision: u64,
    time: *mut u64,
) -> i32 {
    *time = (js_sys::Date::now() * 1e6) as u64;
    0
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_close(_: i32) -> i32 {
    EBADF
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_fdstat_get(
    _: i32,
    _: *mut u8,
) -> i32 {
    EBADF
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_seek(
    _: i32,
    _: i64,
    _: u32,
    _: *mut u64,
) -> i32 {
    EBADF
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_fd_write(
    fd: i32,
    iovs: *const [u32; 2],
    n: u32,
    written: *mut u32,
) -> i32 {
    let mut out = Vec::new();
    for i in 0..n as usize {
        let [ptr, len] = *iovs.add(i);
        out.extend_from_slice(std::slice::from_raw_parts(ptr as *const u8, len as usize));
    }
    *written = out.len() as u32;
    let s = String::from_utf8_lossy(&out);
    if fd == 2 {
        web_sys::console::error_1(&s.as_ref().into());
    } else {
        web_sys::console::log_1(&s.as_ref().into());
    }
    0
}
#[no_mangle]
pub unsafe extern "C" fn __imported_wasi_snapshot_preview1_proc_exit(code: i32) -> ! {
    panic!("proc_exit({code})")
}
