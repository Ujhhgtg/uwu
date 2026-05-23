use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use std::ffi::CStr;

use libloading::Library;
use plugin_api::Plugin;

/// A loaded plugin together with its backing library handle.
///
/// # Safety
///
/// The `Library` must outlive the `Box<dyn Plugin>` because the plugin's
/// vtables point into the library's memory.
///
/// The `plugin` field must be declared BEFORE `_library` so that on drop,
/// the plugin (whose vtable lives in the .so) is destroyed first,
/// and only then is `dlclose()` called to unload the library.
/// Reversing this order causes SIGSEGV.
pub struct LoadedPlugin {
    /// Plugin identifier.
    pub id: String,
    /// Name from the plugin itself (cached for display).
    pub name: String,
    /// Version from the plugin itself (cached for display).
    pub version: String,
    /// Path this plugin was loaded from.
    pub path: PathBuf,
    /// ⚠️ MUST be before `_library` — dropped first, so the vtable is
    /// still in memory when the plugin's destructor runs.
    pub plugin: Box<dyn Plugin>,
    /// ⚠️ MUST be after `plugin` — dropped last, after no code references
    /// the library. dlclose() unloads the .so.
    pub _library: Library,
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        // uninit() runs BEFORE fields are dropped. The field drop order
        // (plugin → _library) guarantees the library is still loaded.
        self.plugin.uninit();
    }
}

/// Error returned when attempting to load a plugin whose id is already loaded.
#[derive(Debug)]
pub struct PluginAlreadyLoaded {
    /// The duplicate plugin id.
    pub id: String,
}

impl fmt::Display for PluginAlreadyLoaded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin '{}' already loaded", self.id)
    }
}

impl Error for PluginAlreadyLoaded {}

type FnPluginRustcVersion = unsafe fn() -> *const i8;

type FnPluginCreate = unsafe fn() -> *mut (dyn Plugin + 'static);

/// Load a plugin from a dynamic library path.
///
/// # Safety
///
/// This function calls `extern "C"` functions on the loaded library. It is safe
/// because the rustc version is checked (via raw C ABI) before any Rust ABI code runs.
pub fn load_plugin_from_path(
    path: PathBuf,
    existing_ids: &[&str],
) -> Result<LoadedPlugin, Box<dyn std::error::Error>> {
    // SAFETY: libloading::Library::new() opens the .so without executing any code.
    // The first function we call is the C ABI version check, which is safe regardless
    // of rustc mismatch.
    let lib = unsafe { Library::new(path.as_path())? };

    // Load the rustc version function (C ABI — always safe).
    let rustc_version_fn: libloading::Symbol<FnPluginRustcVersion> =
        unsafe { lib.get(b"plugin_rustc_version")? };

    let plugin_rustc_version_ptr = unsafe { rustc_version_fn() };
    let plugin_rustc_version = unsafe { CStr::from_ptr(plugin_rustc_version_ptr) }
        .to_str()
        .map_err(|_| "plugin rustc version is not valid UTF-8")?;

    if plugin_rustc_version != plugin_api::RUSTC_VERSION {
        return Err(format!(
            "rustc version mismatch: plugin uses '{}' but host is '{}'",
            plugin_rustc_version,
            plugin_api::RUSTC_VERSION,
        )
        .into());
    }

    // Version matches — safe to load the Rust ABI function.
    let create_fn: libloading::Symbol<FnPluginCreate> = unsafe { lib.get(b"plugin_create")? };

    let plugin_ptr = unsafe { create_fn() };
    // SAFETY: plugin_ptr was created by Box::into_raw from a Box<dyn Plugin>.
    let mut plugin = unsafe { Box::from_raw(plugin_ptr) };

    let id = plugin.id().to_string();

    // Validate identifier format: only letters, digits, underscores.
    if !id.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!(
            "plugin id '{}' contains invalid characters — only letters, digits, and underscores allowed",
            id,
        )
        .into());
    }

    let name = plugin.name().to_string();
    let version = plugin.version().to_string();

    // Check for duplicate BEFORE calling init(), so init/uninit are never
    // called on a plugin that will be discarded.
    if existing_ids.iter().any(|eid| *eid == id) {
        // Drop the plugin (vtable still valid — lib is still loaded) then the
        // library handle itself. init() was never called so uninit() is skipped.
        drop(plugin);
        drop(lib);
        return Err(Box::new(PluginAlreadyLoaded { id }));
    }

    plugin.init();

    Ok(LoadedPlugin {
        id,
        name,
        version,
        path,
        _library: lib,
        plugin,
    })
}
