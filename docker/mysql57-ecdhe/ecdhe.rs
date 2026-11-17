#![no_std]

use core::ffi::{c_char, c_int, c_long, c_void};
use core::ptr::null_mut;

const RTLD_NEXT: *mut c_void = usize::MAX as *mut c_void;
const SSL_CTRL_SET_ECDH_AUTO: c_int = 94;

type NewContext = unsafe extern "C" fn(*const c_void) -> *mut c_void;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn SSL_CTX_ctrl(ctx: *mut c_void, command: c_int, value: c_long, data: *mut c_void) -> c_long;
    fn SSL_CTX_free(ctx: *mut c_void);
    fn abort() -> !;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    // mysqld already loads libc. This library does not link a second runtime.
    unsafe { abort() }
}

/// Enables automatic ephemeral curve selection in the pinned OpenSSL 1.0.2 build.
///
/// # Safety
///
/// `method` must meet the original OpenSSL `SSL_CTX_new` contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn SSL_CTX_new(method: *const c_void) -> *mut c_void {
    // RTLD_NEXT resolves the original symbol in the server's OpenSSL library.
    let symbol = unsafe { dlsym(RTLD_NEXT, c"SSL_CTX_new".as_ptr()) };
    if symbol.is_null() {
        return null_mut();
    }
    // The pinned OpenSSL ABI provides this function signature.
    let original: NewContext = unsafe { core::mem::transmute(symbol) };
    let ctx = unsafe { original(method) };
    if ctx.is_null() || unsafe { SSL_CTX_ctrl(ctx, SSL_CTRL_SET_ECDH_AUTO, 1, null_mut()) } == 1 {
        return ctx;
    }
    // Reject the TLS context if the library could not enable ephemeral curves.
    unsafe { SSL_CTX_free(ctx) };
    null_mut()
}
