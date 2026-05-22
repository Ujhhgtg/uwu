use std::ffi::CStr;

use plugin_api::Plugin;

/// NUL-terminated rustc version string for C FFI.
static RUSTC_VERSION_CSTR: &CStr = {
    // Safety: concat! produces a byte string with a trailing NUL at compile time.
    unsafe { CStr::from_bytes_with_nul_unchecked(concat!(env!("RUSTC_VERSION"), "\0").as_bytes()) }
};

struct ExamplePlugin;

impl Plugin for ExamplePlugin {
    fn name(&self) -> &'static str {
        "示例插件"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn id(&self) -> &'static str {
        "example_plugin"
    }

    fn init(&mut self) {
        println!("[example plugin] plugin loaded!")
    }

    fn uninit(&mut self) {
        println!("[example plugin] plugin unloaded!")
    }

    fn ui(&mut self, ctx: &egui::Context) {
        egui::Window::new("示例插件")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading("你好, uwu!");
                ui.label("这是一个示例插件.");
                ui.label("如果你能看见这个窗口, 说明插件系统工作正常!");
            });
    }
}

// ---- extern "C" exports (the only stable ABI boundary) ----

/// Returns a pointer to a NUL-terminated C string containing the plugin's rustc version.
///
/// # Safety
///
/// The caller must treat the returned pointer as a C string valid for the lifetime
/// of the loaded library. This is the ONLY function guaranteed safe to call across
/// potentially-mismatched rustc versions — it uses raw C ABI only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn plugin_rustc_version() -> *const std::ffi::c_char {
    RUSTC_VERSION_CSTR.as_ptr()
}

/// Constructs and returns a `Box<dyn Plugin>` as a raw fat-pointer.
///
/// # Safety
///
/// The caller assumes ownership of the returned pointer and must eventually
/// reconstruct the `Box<dyn Plugin>` and drop it. Only call this after a
/// successful rustc-version check.
#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn plugin_create() -> *mut dyn Plugin {
    let plugin: Box<dyn Plugin> = Box::new(ExamplePlugin);
    Box::into_raw(plugin)
}
