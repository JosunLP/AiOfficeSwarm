//! WASM plugin loader: instantiate precompiled `.wasm` binaries as [`Plugin`]s.
//!
//! ## Overview
//!
//! [`WasmPlugin`] wraps a precompiled WebAssembly module and drives it
//! through the standard [`Plugin`] lifecycle using the
//! [ABI defined in `wasm_manifest`](crate::wasm_manifest).
//!
//! [`WasmPluginLoader`] provides high-level helpers that combine manifest
//! parsing and module loading into a single call.
//!
//! ## Usage
//!
//! ```no_run
//! use std::path::Path;
//! use swarm_plugin::wasm_loader::WasmPluginLoader;
//! use swarm_plugin::PluginHost;
//!
//! # async fn example() -> swarm_core::error::SwarmResult<()> {
//! // Load a plugin from a manifest + .wasm file pair
//! let plugin = WasmPluginLoader::from_manifest_file(
//!     Path::new("plugins/my-plugin/plugin.toml"),
//! )?;
//!
//! let host = PluginHost::new();
//! let id = host.load(Box::new(plugin)).await?;
//!
//! let result = host
//!     .invoke(&id, "my_action", serde_json::json!({"key": "value"}))
//!     .await?;
//! println!("Result: {result}");
//! # Ok(())
//! # }
//! ```
//!
//! ## WASM ABI requirements
//!
//! The `.wasm` module **must** export the following symbols. All strings are
//! passed as UTF-8 bytes through WASM linear memory; the host uses
//! `swarm_alloc` / `swarm_dealloc` to manage those allocations.
//!
//! | Export | Type | Semantics |
//! |--------|------|-----------|
//! | `memory` | memory | The module's linear memory |
//! | `swarm_alloc` | `(i32) -> i32` | Allocate bytes; returns pointer |
//! | `swarm_dealloc` | `(i32, i32)` | Free bytes at pointer |
//! | `swarm_on_load` | `() -> i32` | `0` = success |
//! | `swarm_on_unload` | `() -> i32` | `0` = success |
//! | `swarm_health_check` | `() -> i32` | `0` = healthy |
//! | `swarm_invoke` | `(i32, i32, i32, i32, i32, i32) -> i32` | see below |
//!
//! ### `swarm_invoke` return convention
//!
//! Parameters: `(action_ptr, action_len, params_ptr, params_len, result_ptr, result_cap)`
//!
//! - `n >= 0` → success; `n` bytes of JSON result were written to `result_ptr`.
//! - `n < 0`  → error; `(-n)` bytes of a UTF-8 error message were written to
//!   `result_ptr`. If `n == -1` the error message is empty.

use std::path::Path;
use std::ops::Range;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use wasmtime::{Engine, Instance, Memory, Module, Store, TypedFunc};

use swarm_core::error::{SwarmError, SwarmResult};

use crate::{
    manifest::PluginManifest,
    wasm_manifest::WasmManifestFile,
    Plugin,
};

// ─── Internal runtime state ───────────────────────────────────────────────────

/// Result buffer capacity used by the host when reading WASM output.
///
/// 256 KiB should be sufficient for almost all plugin responses. Plugins
/// that need to return larger payloads should chunk their output.
const RESULT_BUFFER_CAPACITY: i32 = 256 * 1024; // 256 KiB

/// All wasmtime objects that must be accessed together (they share a lifetime
/// via the `Store`).
struct WasmRuntime {
    store: Store<()>,
    /// Linear memory of the WASM module.
    memory: Memory,
    // Typed function handles – extracted once during `on_load` to avoid
    // repeated lookups.
    fn_alloc: TypedFunc<i32, i32>,
    fn_dealloc: TypedFunc<(i32, i32), ()>,
    fn_on_load: TypedFunc<(), i32>,
    fn_on_unload: TypedFunc<(), i32>,
    fn_health_check: TypedFunc<(), i32>,
    fn_invoke: TypedFunc<(i32, i32, i32, i32, i32, i32), i32>,
}
// wasmtime guarantees Store<T>: Send + Sync when T: Send + Sync.
// All other field types (Memory, TypedFunc) are also Send + Sync in wasmtime.
// Therefore WasmRuntime: Send + Sync by the compiler's automatic derivation.

#[derive(Debug, Clone, Copy)]
struct GuestAllocation {
    ptr: i32,
    len: i32,
}

#[derive(Debug, Default)]
struct GuestAllocations(Vec<GuestAllocation>);

impl GuestAllocations {
    fn push(&mut self, ptr: i32, len: i32) {
        self.0.push(GuestAllocation { ptr, len });
    }

    fn deallocate_all(self, rt: &mut WasmRuntime) -> SwarmResult<()> {
        let mut first_err = None;

        for allocation in self.0.into_iter().rev() {
            if let Err(err) = WasmPlugin::dealloc(rt, allocation.ptr, allocation.len) {
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        }

        first_err.map_or(Ok(()), Err)
    }
}

// ─── WasmPlugin ───────────────────────────────────────────────────────────────

/// A plugin backed by a precompiled WebAssembly module.
///
/// Create instances via [`WasmPluginLoader`].
pub struct WasmPlugin {
    manifest: PluginManifest,
    /// The compiled module, kept around so we can re-instantiate if needed.
    module: Arc<Module>,
    /// Wasmtime engine shared with the module.
    engine: Engine,
    /// Live runtime – `None` before `on_load`, `Some` after.
    runtime: Arc<Mutex<Option<WasmRuntime>>>,
}

impl WasmPlugin {
    fn new(manifest: PluginManifest, engine: Engine, module: Module) -> Self {
        Self {
            manifest,
            module: Arc::new(module),
            engine,
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    // ── Memory helpers ────────────────────────────────────────────────────────

    /// Write `data` into the WASM module's linear memory using
    /// `swarm_alloc`. Returns `(ptr, len)` to be passed to the WASM function.
    ///
    /// The caller is responsible for calling `swarm_dealloc(ptr, len)` when done.
    fn alloc_and_write(
        rt: &mut WasmRuntime,
        plugin_name: &str,
        data: &[u8],
    ) -> SwarmResult<(i32, i32)> {
        let len = i32::try_from(data.len()).map_err(|_| SwarmError::PluginOperationFailed {
            name: plugin_name.into(),
            reason: format!("input too large for WASM ABI: {} bytes", data.len()),
        })?;
        let ptr = rt
            .fn_alloc
            .call(&mut rt.store, len)
            .map_err(|e| SwarmError::PluginOperationFailed {
                name: plugin_name.into(),
                reason: format!("swarm_alloc failed: {e}"),
            })?;

        let range = Self::checked_memory_range(
            plugin_name,
            rt.memory.data_size(&rt.store),
            ptr,
            data.len(),
            "swarm_alloc returned an out-of-bounds pointer for input data",
        )?;
        let mem = rt.memory.data_mut(&mut rt.store);
        mem[range].copy_from_slice(data);
        Ok((ptr, len))
    }

    /// Free previously allocated WASM memory.
    fn dealloc(rt: &mut WasmRuntime, ptr: i32, len: i32) -> SwarmResult<()> {
        rt.fn_dealloc
            .call(&mut rt.store, (ptr, len))
            .map_err(|e| SwarmError::PluginOperationFailed {
                name: "wasm".into(),
                reason: format!("swarm_dealloc failed: {e}"),
            })
    }

    /// Allocate a result buffer in WASM memory of size `RESULT_BUFFER_CAPACITY`.
    fn alloc_result_buffer(rt: &mut WasmRuntime, plugin_name: &str) -> SwarmResult<i32> {
        let ptr = rt
            .fn_alloc
            .call(&mut rt.store, RESULT_BUFFER_CAPACITY)
            .map_err(|e| SwarmError::PluginOperationFailed {
                name: plugin_name.into(),
                reason: format!("swarm_alloc (result buffer) failed: {e}"),
            })?;
        Self::checked_memory_range(
            plugin_name,
            rt.memory.data_size(&rt.store),
            ptr,
            0,
            "swarm_alloc returned an invalid result buffer pointer",
        )?;
        Ok(ptr)
    }

    /// Read `len` bytes from the WASM memory at `ptr` into a `Vec<u8>`.
    fn read_bytes(
        rt: &WasmRuntime,
        plugin_name: &str,
        ptr: i32,
        len: usize,
    ) -> SwarmResult<Vec<u8>> {
        let range = Self::checked_memory_range(
            plugin_name,
            rt.memory.data_size(&rt.store),
            ptr,
            len,
            "plugin attempted to read outside linear memory",
        )?;
        Ok(rt.memory.data(&rt.store)[range].to_vec())
    }

    fn checked_memory_range(
        plugin_name: &str,
        memory_len: usize,
        ptr: i32,
        len: usize,
        context: &str,
    ) -> SwarmResult<Range<usize>> {
        let start = usize::try_from(ptr).map_err(|_| SwarmError::PluginOperationFailed {
            name: plugin_name.into(),
            reason: format!("{context}: negative pointer {ptr}"),
        })?;
        let end = start.checked_add(len).ok_or_else(|| SwarmError::PluginOperationFailed {
            name: plugin_name.into(),
            reason: format!("{context}: pointer {ptr} with length {len} overflows"),
        })?;

        if end > memory_len {
            return Err(SwarmError::PluginOperationFailed {
                name: plugin_name.into(),
                reason: format!(
                    "{context}: range {start}..{end} exceeds linear memory size {memory_len}"
                ),
            });
        }

        Ok(start..end)
    }
}

#[async_trait]
impl Plugin for WasmPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn on_load(&mut self) -> SwarmResult<()> {
        let mut store = Store::new(&self.engine, ());

        // Instantiate the module with no imports (pure WASM).
        let instance =
            Instance::new(&mut store, &self.module, &[]).map_err(|e| {
                SwarmError::PluginInitFailed {
                    name: self.manifest.name.clone(),
                    reason: format!("WASM instantiation failed: {e}"),
                }
            })?;

        // Resolve required exports.
        let memory = resolve_memory(&instance, &mut store, &self.manifest.name)?;
        let fn_alloc = resolve_fn::<i32, i32>(&instance, &mut store, "swarm_alloc", &self.manifest.name)?;
        let fn_dealloc = resolve_fn::<(i32, i32), ()>(&instance, &mut store, "swarm_dealloc", &self.manifest.name)?;
        let fn_on_load = resolve_fn::<(), i32>(&instance, &mut store, "swarm_on_load", &self.manifest.name)?;
        let fn_on_unload = resolve_fn::<(), i32>(&instance, &mut store, "swarm_on_unload", &self.manifest.name)?;
        let fn_health_check = resolve_fn::<(), i32>(&instance, &mut store, "swarm_health_check", &self.manifest.name)?;
        let fn_invoke = resolve_fn::<(i32, i32, i32, i32, i32, i32), i32>(
            &instance, &mut store, "swarm_invoke", &self.manifest.name,
        )?;

        let mut rt = WasmRuntime {
            store,
            memory,
            fn_alloc,
            fn_dealloc,
            fn_on_load,
            fn_on_unload,
            fn_health_check,
            fn_invoke,
        };

        // Call `swarm_on_load` inside the WASM module.
        let rc = rt
            .fn_on_load
            .call(&mut rt.store, ())
            .map_err(|e| SwarmError::PluginInitFailed {
                name: self.manifest.name.clone(),
                reason: format!("swarm_on_load trap: {e}"),
            })?;

        if rc != 0 {
            return Err(SwarmError::PluginInitFailed {
                name: self.manifest.name.clone(),
                reason: format!("swarm_on_load returned error code {rc}"),
            });
        }

        *self.runtime.lock().await = Some(rt);
        Ok(())
    }

    async fn on_unload(&mut self) -> SwarmResult<()> {
        let mut guard = self.runtime.lock().await;
        let rt = match guard.as_mut() {
            Some(rt) => rt,
            None => return Ok(()), // nothing to do if never loaded
        };

        let rc = rt
            .fn_on_unload
            .call(&mut rt.store, ())
            .map_err(|e| SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: format!("swarm_on_unload trap: {e}"),
            })?;

        if rc != 0 {
            tracing::warn!(
                plugin = %self.manifest.name,
                code = rc,
                "swarm_on_unload returned non-zero (continuing unload)"
            );
        }

        *guard = None;
        Ok(())
    }

    async fn invoke(
        &self,
        action: &str,
        params: serde_json::Value,
    ) -> SwarmResult<serde_json::Value> {
        let mut guard = self.runtime.lock().await;
        let rt = guard.as_mut().ok_or_else(|| SwarmError::PluginOperationFailed {
            name: self.manifest.name.clone(),
            reason: "WASM plugin is not loaded".into(),
        })?;

        let params_json = params.to_string();
        let plugin_name = self.manifest.name.clone();
        let mut allocations = GuestAllocations::default();

        let result = (|| -> SwarmResult<serde_json::Value> {
            // Write action name and params into WASM memory.
            let (action_ptr, action_len) = Self::alloc_and_write(rt, &plugin_name, action.as_bytes())?;
            allocations.push(action_ptr, action_len);

            let (params_ptr, params_len) =
                Self::alloc_and_write(rt, &plugin_name, params_json.as_bytes())?;
            allocations.push(params_ptr, params_len);

            // Allocate the result buffer.
            let result_ptr = Self::alloc_result_buffer(rt, &plugin_name)?;
            allocations.push(result_ptr, RESULT_BUFFER_CAPACITY);

            // Call `swarm_invoke`.
            let ret = rt
                .fn_invoke
                .call(
                    &mut rt.store,
                    (
                        action_ptr,
                        action_len,
                        params_ptr,
                        params_len,
                        result_ptr,
                        RESULT_BUFFER_CAPACITY,
                    ),
                )
                .map_err(|e| SwarmError::PluginOperationFailed {
                    name: plugin_name.clone(),
                    reason: format!("swarm_invoke trap: {e}"),
                })?;

            // Interpret the return value.
            if ret >= 0 {
                let len = ret as usize;
                if len > RESULT_BUFFER_CAPACITY as usize {
                    return Err(SwarmError::PluginOperationFailed {
                        name: plugin_name.clone(),
                        reason: format!(
                            "WASM plugin returned success length {len} exceeding result buffer capacity {}",
                            RESULT_BUFFER_CAPACITY
                        ),
                    });
                }

                let bytes = Self::read_bytes(rt, &plugin_name, result_ptr, len)?;
                let json: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(SwarmError::Serialization)?;
                Ok(json)
            } else {
                let err_msg = if ret == -1 {
                    "WASM plugin returned an unspecified error".into()
                } else {
                    let err_len = (-ret) as usize;
                    if err_len > RESULT_BUFFER_CAPACITY as usize {
                        return Err(SwarmError::PluginOperationFailed {
                            name: plugin_name.clone(),
                            reason: format!(
                                "WASM plugin returned error length {err_len} exceeding result buffer capacity {}",
                                RESULT_BUFFER_CAPACITY
                            ),
                        });
                    }

                    let bytes = Self::read_bytes(rt, &plugin_name, result_ptr, err_len)?;
                    String::from_utf8_lossy(&bytes).into_owned()
                };

                Err(SwarmError::PluginOperationFailed {
                    name: plugin_name.clone(),
                    reason: err_msg,
                })
            }
        })();

        let cleanup_result = allocations.deallocate_all(rt);
        match (result, cleanup_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
            (Err(primary_err), Ok(())) => Err(primary_err),
            (Err(primary_err), Err(cleanup_err)) => {
                tracing::warn!(
                    plugin = %plugin_name,
                    cleanup_error = %cleanup_err,
                    "failed to deallocate guest buffers after invoke error"
                );
                Err(primary_err)
            }
        }
    }

    async fn health_check(&self) -> SwarmResult<()> {
        let mut guard = self.runtime.lock().await;
        let rt = guard.as_mut().ok_or_else(|| SwarmError::PluginOperationFailed {
            name: self.manifest.name.clone(),
            reason: "WASM plugin is not loaded".into(),
        })?;

        let rc = rt
            .fn_health_check
            .call(&mut rt.store, ())
            .map_err(|e| SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: format!("swarm_health_check trap: {e}"),
            })?;

        if rc == 0 {
            Ok(())
        } else {
            Err(SwarmError::PluginOperationFailed {
                name: self.manifest.name.clone(),
                reason: format!("swarm_health_check returned {rc}"),
            })
        }
    }
}

// ─── Resolution helpers ───────────────────────────────────────────────────────

fn resolve_memory(
    instance: &Instance,
    store: &mut Store<()>,
    plugin_name: &str,
) -> SwarmResult<Memory> {
    instance
        .get_memory(store, "memory")
        .ok_or_else(|| SwarmError::PluginInitFailed {
            name: plugin_name.to_string(),
            reason: "WASM module does not export 'memory'".into(),
        })
}

fn resolve_fn<P, R>(
    instance: &Instance,
    store: &mut Store<()>,
    name: &str,
    plugin_name: &str,
) -> SwarmResult<TypedFunc<P, R>>
where
    P: wasmtime::WasmParams,
    R: wasmtime::WasmResults,
{
    instance
        .get_typed_func::<P, R>(store, name)
        .map_err(|e| SwarmError::PluginInitFailed {
            name: plugin_name.to_string(),
            reason: format!("WASM export '{name}' has unexpected type or is missing: {e}"),
        })
}

// ─── WasmPluginLoader ─────────────────────────────────────────────────────────

/// High-level helper for loading a WASM plugin from files on disk.
///
/// Combines manifest parsing and module compilation into a single call.
///
/// ## Example
///
/// ```no_run
/// use std::path::Path;
/// use swarm_plugin::wasm_loader::WasmPluginLoader;
///
/// # fn example() -> swarm_core::error::SwarmResult<()> {
/// let plugin = WasmPluginLoader::from_manifest_file(
///     Path::new("plugins/my-plugin/plugin.toml"),
/// )?;
/// // `plugin` is a `Box<dyn Plugin>` ready to be loaded into `PluginHost`.
/// # let _ = plugin;
/// # Ok(())
/// # }
/// ```
pub struct WasmPluginLoader;

impl WasmPluginLoader {
    /// Load a WASM plugin from a manifest TOML file.
    ///
    /// The `.wasm` binary path is resolved relative to the manifest file's
    /// directory as declared in the `wasm_file` field.
    ///
    /// # Errors
    /// - [`SwarmError::Io`] – manifest or WASM file not readable.
    /// - [`SwarmError::Internal`] – manifest is invalid TOML.
    /// - [`SwarmError::PluginInitFailed`] – WASM binary fails to compile.
    pub fn from_manifest_file(manifest_path: &Path) -> SwarmResult<WasmPlugin> {
        let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
        let manifest_file = WasmManifestFile::load(manifest_path)?;
        let (manifest, wasm_path) =
            manifest_file.into_plugin_manifest_and_wasm_path(manifest_dir)?;
        Self::from_files_and_manifest(&wasm_path, manifest)
    }

    /// Load a WASM plugin from a pre-parsed [`PluginManifest`] and a path to
    /// the `.wasm` binary.
    ///
    /// Useful when the caller has already parsed the manifest or wants to
    /// supply a manifest constructed programmatically.
    pub fn from_files_and_manifest(
        wasm_path: &Path,
        manifest: PluginManifest,
    ) -> SwarmResult<WasmPlugin> {
        let bytes = std::fs::read(wasm_path).map_err(SwarmError::Io)?;
        Self::from_bytes_and_manifest(&bytes, manifest)
    }

    /// Compile WASM bytes and wrap them with a [`PluginManifest`].
    ///
    /// This variant is useful in tests where the bytes are already available
    /// in memory (e.g., built via `include_bytes!`).
    pub fn from_bytes_and_manifest(
        wasm_bytes: &[u8],
        manifest: PluginManifest,
    ) -> SwarmResult<WasmPlugin> {
        let engine = Engine::default();
        let module =
            Module::from_binary(&engine, wasm_bytes).map_err(|e| SwarmError::PluginInitFailed {
                name: manifest.name.clone(),
                reason: format!("WASM compilation failed: {e}"),
            })?;
        Ok(WasmPlugin::new(manifest, engine, module))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    /// Build a minimal but complete echo WASM module from WAT source.
    ///
    /// The module implements the full swarm ABI:
    /// - `swarm_alloc` / `swarm_dealloc`: bump allocator starting at page 1
    /// - `swarm_on_load` / `swarm_on_unload`: return 0 (no-op)
    /// - `swarm_health_check`: returns 0 (always healthy)
    /// - `swarm_invoke`: echoes the params JSON back to the result buffer
    fn echo_wasm_bytes() -> Vec<u8> {
        // Use byte-by-byte copy loop (WASM 1.0, no bulk-memory proposal needed)
        let wat = r#"
(module
  ;; Two pages of linear memory (128 KiB).
  ;; Page 0 (0x00000 – 0x0FFFF): reserved for static data & result buffers
  ;; Page 1+ (0x10000+): bump allocator heap
  (memory (export "memory") 2)

  ;; Bump allocator state: starts at the beginning of page 1.
  (global $heap_ptr (mut i32) (i32.const 65536))

  ;; Allocate `$size` bytes from the bump heap; return the old heap pointer.
  (func (export "swarm_alloc") (param $size i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $heap_ptr))
    (global.set $heap_ptr (i32.add (global.get $heap_ptr) (local.get $size)))
    (local.get $ptr)
  )

  ;; No-op free (bump allocator).
  (func (export "swarm_dealloc") (param $ptr i32) (param $len i32))

  ;; Lifecycle hooks – all return 0 (success).
  (func (export "swarm_on_load")     (result i32) (i32.const 0))
  (func (export "swarm_on_unload")   (result i32) (i32.const 0))
  (func (export "swarm_health_check")(result i32) (i32.const 0))

  ;; Echo: copy params bytes into result buffer; return byte count.
  (func (export "swarm_invoke")
    (param $ap i32) (param $al i32)   ;; action ptr + len (unused by echo)
    (param $pp i32) (param $pl i32)   ;; params ptr + len
    (param $rp i32) (param $rc i32)   ;; result ptr + capacity
    (result i32)
    (local $i i32)
    (local $n i32)

    ;; n = min(params_len, result_cap)
    (if (i32.lt_s (local.get $pl) (local.get $rc))
      (then (local.set $n (local.get $pl)))
      (else (local.set $n (local.get $rc)))
    )

    ;; Copy params[0..n] → result[0..n] byte by byte.
    (local.set $i (i32.const 0))
    (block $break
      (loop $loop
        (br_if $break (i32.ge_s (local.get $i) (local.get $n)))
        (i32.store8
          (i32.add (local.get $rp) (local.get $i))
          (i32.load8_u (i32.add (local.get $pp) (local.get $i)))
        )
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $loop)
      )
    )

    (local.get $n)
  )
)
"#;
        wat::parse_str(wat).expect("WAT should compile to valid WASM")
    }

    fn invoke_error_no_message_wasm_bytes() -> Vec<u8> {
        let wat = r#"
(module
  (memory (export "memory") 2)
  (global $heap_ptr (mut i32) (i32.const 65536))
  (func (export "swarm_alloc") (param $size i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $heap_ptr))
    (global.set $heap_ptr (i32.add (global.get $heap_ptr) (local.get $size)))
    (local.get $ptr)
  )
  (func (export "swarm_dealloc") (param $ptr i32) (param $len i32))
  (func (export "swarm_on_load") (result i32) (i32.const 0))
  (func (export "swarm_on_unload") (result i32) (i32.const 0))
  (func (export "swarm_health_check") (result i32) (i32.const 0))
  (func (export "swarm_invoke")
    (param $ap i32) (param $al i32)
    (param $pp i32) (param $pl i32)
    (param $rp i32) (param $rc i32)
    (result i32)
    (i32.const -1)
  )
)
"#;
        wat::parse_str(wat).expect("WAT should compile to valid WASM")
    }

    fn invoke_error_length_too_large_wasm_bytes() -> Vec<u8> {
        let wat = format!(
            r#"
(module
  (memory (export "memory") 2)
  (global $heap_ptr (mut i32) (i32.const 65536))
  (func (export "swarm_alloc") (param $size i32) (result i32)
    (local $ptr i32)
    (local.set $ptr (global.get $heap_ptr))
    (global.set $heap_ptr (i32.add (global.get $heap_ptr) (local.get $size)))
    (local.get $ptr)
  )
  (func (export "swarm_dealloc") (param $ptr i32) (param $len i32))
  (func (export "swarm_on_load") (result i32) (i32.const 0))
  (func (export "swarm_on_unload") (result i32) (i32.const 0))
  (func (export "swarm_health_check") (result i32) (i32.const 0))
  (func (export "swarm_invoke")
    (param $ap i32) (param $al i32)
    (param $pp i32) (param $pl i32)
    (param $rp i32) (param $rc i32)
    (result i32)
    (i32.const -{})
  )
)
"#,
            RESULT_BUFFER_CAPACITY + 1
        );
        wat::parse_str(&wat).expect("WAT should compile to valid WASM")
    }

    fn invalid_alloc_pointer_wasm_bytes() -> Vec<u8> {
        let wat = r#"
(module
  (memory (export "memory") 1)
  (func (export "swarm_alloc") (param $size i32) (result i32)
    (i32.const 70000)
  )
  (func (export "swarm_dealloc") (param $ptr i32) (param $len i32))
  (func (export "swarm_on_load") (result i32) (i32.const 0))
  (func (export "swarm_on_unload") (result i32) (i32.const 0))
  (func (export "swarm_health_check") (result i32) (i32.const 0))
  (func (export "swarm_invoke")
    (param $ap i32) (param $al i32)
    (param $pp i32) (param $pl i32)
    (param $rp i32) (param $rc i32)
    (result i32)
    (i32.const 0)
  )
)
"#;
        wat::parse_str(wat).expect("WAT should compile to valid WASM")
    }

    fn test_manifest() -> PluginManifest {
        PluginManifest::new("test-wasm", "0.1.0", "tests", "WASM integration test plugin")
    }

    // ── Loader ──────────────────────────────────────────────────────────────

    #[test]
    fn loader_compiles_valid_wasm() {
        let bytes = echo_wasm_bytes();
        let plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest());
        assert!(plugin.is_ok(), "valid WASM should compile: {:?}", plugin.err());
    }

    #[test]
    fn loader_rejects_invalid_bytes() {
        let result = WasmPluginLoader::from_bytes_and_manifest(b"not wasm", test_manifest());
        assert!(result.is_err(), "invalid bytes should be rejected");
    }

    // ── Plugin lifecycle ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn on_load_succeeds() {
        let bytes = echo_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load should succeed");
    }

    #[tokio::test]
    async fn health_check_passes_after_load() {
        let bytes = echo_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");
        plugin.health_check().await.expect("health_check should pass");
    }

    #[tokio::test]
    async fn health_check_fails_before_load() {
        let bytes = echo_wasm_bytes();
        let plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        // Plugin not loaded yet → should return an error.
        assert!(plugin.health_check().await.is_err());
    }

    #[tokio::test]
    async fn invoke_echo_returns_params_json() {
        let bytes = echo_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");

        let params = serde_json::json!({"message": "hello from host", "value": 42});
        let result = plugin
            .invoke("echo", params.clone())
            .await
            .expect("invoke should succeed");

        assert_eq!(result, params, "echo should return params unchanged");
    }

    #[tokio::test]
    async fn invoke_fails_before_load() {
        let bytes = echo_wasm_bytes();
        let plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        let result = plugin.invoke("echo", serde_json::json!({})).await;
        assert!(result.is_err(), "invoke before on_load should fail");
    }

    #[tokio::test]
    async fn invoke_error_without_message_uses_unspecified_error_text() {
        let bytes = invoke_error_no_message_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");

        let err = plugin
            .invoke("echo", serde_json::json!({"message": "hello"}))
            .await
            .expect_err("invoke should fail");
        assert!(
            err.to_string().contains("WASM plugin returned an unspecified error"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_error_length_exceeding_result_buffer_capacity() {
        let bytes = invoke_error_length_too_large_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");

        let err = plugin
            .invoke("echo", serde_json::json!({"message": "hello"}))
            .await
            .expect_err("invoke should fail");
        assert!(
            err.to_string().contains("exceeding result buffer capacity"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn invoke_rejects_out_of_bounds_allocator_pointer() {
        let bytes = invalid_alloc_pointer_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");

        let err = plugin
            .invoke("echo", serde_json::json!({"message": "hello"}))
            .await
            .expect_err("invoke should fail");
        assert!(
            err.to_string().contains("out-of-bounds pointer"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn on_unload_after_load_succeeds() {
        let bytes = echo_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_load().await.expect("on_load");
        plugin.on_unload().await.expect("on_unload should succeed");
    }

    #[tokio::test]
    async fn on_unload_before_load_is_noop() {
        // Calling on_unload before on_load should not panic.
        let bytes = echo_wasm_bytes();
        let mut plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        plugin.on_unload().await.expect("on_unload before on_load is a no-op");
    }

    #[tokio::test]
    async fn manifest_is_accessible() {
        let bytes = echo_wasm_bytes();
        let plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");
        assert_eq!(plugin.manifest().name, "test-wasm");
        assert_eq!(plugin.manifest().version, "0.1.0");
    }

    // ── Full lifecycle via PluginHost ────────────────────────────────────────

    #[tokio::test]
    async fn plugin_host_full_lifecycle() {
        use crate::PluginHost;

        let bytes = echo_wasm_bytes();
        let plugin = WasmPluginLoader::from_bytes_and_manifest(&bytes, test_manifest())
            .expect("compile");

        let host = PluginHost::new();
        let id = host.load(Box::new(plugin)).await.expect("host.load");

        // Invoke echo action via the host.
        let params = serde_json::json!({"x": 1, "y": 2});
        let result = host
            .invoke(&id, "echo", params.clone())
            .await
            .expect("host.invoke");
        assert_eq!(result, params);

        // Health check via host.
        let checks = host.health_check_all().await;
        assert_eq!(checks.len(), 1);
        assert!(checks[0].1.is_ok(), "WASM plugin should be healthy");

        // Unload.
        host.unload(&id).await.expect("host.unload");
        let record = host.registry().get(&id).expect("record should still exist");
        assert_eq!(record.state.label(), "unloaded");
    }
}
