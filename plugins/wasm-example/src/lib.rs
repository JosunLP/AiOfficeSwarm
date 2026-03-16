//! # echo-plugin — example AiOfficeSwarm WASM plugin
//!
//! This is a minimal, self-contained plugin that demonstrates how to
//! implement the AiOfficeSwarm WASM ABI. It exports the six required symbols
//! so the host ([`swarm_plugin::wasm_loader::WasmPlugin`]) can drive it
//! through its lifecycle.
//!
//! ## Building
//!
//! ```sh
//! # Add the wasm32-unknown-unknown target once:
//! rustup target add wasm32-unknown-unknown
//!
//! # Build the plugin:
//! cargo build --target wasm32-unknown-unknown --release
//!
//! # Copy the binary next to the manifest:
//! cp target/wasm32-unknown-unknown/release/echo_plugin.wasm \
//!    plugins/wasm-example/echo-plugin.wasm
//! ```
//!
//! ## ABI implemented here
//!
//! | Export | Behaviour |
//! |--------|-----------|
//! | `memory` | Linear memory (auto-exported by Rust) |
//! | `swarm_alloc` | Delegates to the global allocator |
//! | `swarm_dealloc` | Delegates to the global allocator |
//! | `swarm_on_load` | No-op; returns 0 |
//! | `swarm_on_unload` | No-op; returns 0 |
//! | `swarm_health_check` | Always healthy; returns 0 |
//! | `swarm_invoke` | Handles `"echo"` and `"ping"` |

// No std — minimal binary size.
#![no_std]

extern crate alloc;

// ─── Panic handler ────────────────────────────────────────────────────────────

// `no_std` + `cdylib` requires a panic handler.  We simply trap (abort)
// because there is nowhere meaningful to unwind in a WASM sandbox.
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// ─── Global allocator ─────────────────────────────────────────────────────────

// On wasm32-unknown-unknown there is no default global allocator.
// We use `wee_alloc` for a minimal footprint (see Cargo.toml).
#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

// ─── WASM ABI exports ─────────────────────────────────────────────────────────

/// Allocate `size` bytes and return a pointer.
///
/// The host calls this before writing input data into WASM linear memory.
#[no_mangle]
pub unsafe extern "C" fn swarm_alloc(size: usize) -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(size, 1).unwrap();
    alloc::alloc::alloc(layout)
}

/// Free `len` bytes at `ptr`.
///
/// The host calls this after it has finished using a buffer it allocated.
#[no_mangle]
pub unsafe extern "C" fn swarm_dealloc(ptr: *mut u8, len: usize) {
    let layout = core::alloc::Layout::from_size_align(len, 1).unwrap();
    alloc::alloc::dealloc(ptr, layout);
}

/// Called once by the host after the module is instantiated.
///
/// Perform one-time setup here (connect to external services, etc.).
/// Return 0 on success.
#[no_mangle]
pub extern "C" fn swarm_on_load() -> i32 {
    0 // success
}

/// Called once by the host before unloading the module.
///
/// Release any resources acquired in `swarm_on_load`. Return 0 on success.
#[no_mangle]
pub extern "C" fn swarm_on_unload() -> i32 {
    0 // success
}

/// Return 0 if the plugin is healthy, non-zero otherwise.
#[no_mangle]
pub extern "C" fn swarm_health_check() -> i32 {
    0 // always healthy
}

/// Invoke a named action.
///
/// # Parameters
/// - `action_ptr` / `action_len`: pointer + length of the action name (UTF-8)
///   in linear memory.
/// - `params_ptr` / `params_len`: pointer + length of the JSON params string
///   (UTF-8) in linear memory.
/// - `result_ptr` / `result_cap`: pointer + capacity of the result buffer.
///
/// # Return value
/// - `n >= 0` → `n` bytes of JSON result written to `result_ptr` (success).
/// - `n < 0`  → `(-n)` bytes of error message written to `result_ptr`.
///   A return value of `-1` means error with no message.
#[no_mangle]
pub unsafe extern "C" fn swarm_invoke(
    action_ptr: *const u8,
    action_len: usize,
    params_ptr: *const u8,
    params_len: usize,
    result_ptr: *mut u8,
    result_cap: usize,
) -> i32 {
    let action = core::str::from_utf8(
        core::slice::from_raw_parts(action_ptr, action_len)
    ).unwrap_or("");
    let params_bytes = core::slice::from_raw_parts(params_ptr, params_len);
    let result_buf = core::slice::from_raw_parts_mut(result_ptr, result_cap);

    match action {
        "echo" => {
            // Return the params JSON unchanged.
            let n = params_bytes.len().min(result_cap);
            result_buf[..n].copy_from_slice(&params_bytes[..n]);
            n as i32
        }
        "ping" => {
            let response = b"{\"pong\":true}";
            let n = response.len().min(result_cap);
            result_buf[..n].copy_from_slice(&response[..n]);
            n as i32
        }
        other => {
            // Write error message and return negative length.
            let msg = alloc::format!("unknown action: '{}'", other);
            let bytes = msg.as_bytes();
            let n = bytes.len().min(result_cap);
            result_buf[..n].copy_from_slice(&bytes[..n]);
            if n == 0 { -1 } else { -(n as i32) }
        }
    }
}
