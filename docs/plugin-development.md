# Plugin Development Guide

## Overview

uwu supports loading Rust dynamic libraries (`.so` / `.dylib` / `.dll`) at runtime to extend functionality. Plugins are pure Rust — no C FFI beyond the two bootstrap symbols. The system relies on the user's guarantee that the plugin was compiled with the **same rustc version** as the host.

## Requirements

- Same `rustc` version as the host (checked at load time)
- `crate-type = ["dylib"]` (Rust dynamic library, **not** `cdylib`)
- Dependency on `plugin_api` crate from this workspace
- Same version of `egui` as the host

## Quick Start

### 1. Create a new crate

```toml
# my_plugin/Cargo.toml
[package]
name = "my_plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["dylib"]

[dependencies]
plugin_api = { path = "../path/to/plugin_api" }
egui = { version = "0.34", default-features = false }
```

### 2. Add a build.rs

Each plugin crate needs its own `build.rs` to embed the rustc version string:

```rust
// my_plugin/build.rs
use std::process::Command;

fn main() {
    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let version_trimmed = rustc_version.trim();
    println!("cargo:rustc-env=RUSTC_VERSION={}", version_trimmed);
    println!("cargo:rerun-if-changed=build.rs");
}
```

### 3. Implement the Plugin trait

```rust
// my_plugin/src/lib.rs
use std::ffi::CStr;
use plugin_api::Plugin;

static RUSTC_VERSION_CSTR: &CStr = {
    unsafe { CStr::from_bytes_with_nul_unchecked(concat!(env!("RUSTC_VERSION"), "\0").as_bytes()) }
};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &'static str {
        "My Plugin"
    }

    fn version(&self) -> &'static str {
        "0.1.0"
    }

    fn id(&self) -> &'static str {
        "my_plugin"
    }

    fn ui(&mut self, ctx: &egui::Context) {
        egui::Window::new("My Plugin").show(ctx, |ui| {
            ui.label("Hello from my plugin!");
        });
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn plugin_rustc_version() -> *const std::ffi::c_char {
    RUSTC_VERSION_CSTR.as_ptr()
}

#[unsafe(no_mangle)]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn plugin_create() -> *mut dyn Plugin {
    let plugin: Box<dyn Plugin> = Box::new(MyPlugin);
    Box::into_raw(plugin)
}
```

## The `Plugin` Trait

```rust
pub trait Plugin {
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn id(&self) -> &'static str;
    fn init(&mut self) {}
    fn before_ui(&mut self) {}
    fn uninit(&mut self) {}
    fn ui(&mut self, ctx: &egui::Context) {}
}
```

| Method     | Required | Description |
|------------|----------|-------------|
| `name()`   | Yes      | Display name shown in the plugin list |
| `version()`| Yes      | Semantic version string |
| `id()`     | Yes      | Unique identifier — only letters, digits, underscores |
| `init()`   | Optional | Called once after construction, before any frame callbacks |
| `before_ui()` | Optional | Called once per frame before `ui()` |
| `uninit()` | Optional | Called once when the plugin is unloaded, before the library is freed |

### `id()` requirements

- Must consist only of **letters, digits, and underscores** (`[a-zA-Z0-9_]+`)
- Used for deduplication — loading a plugin whose `id` matches an already-loaded plugin is rejected

All methods are called from the main render thread (winit event loop).

## Loading Protocol

The host loads plugins through this sequence:

1. `libloading::Library::new(path)` — open the dynamic library
2. `plugin_rustc_version()` (C ABI) — read the plugin's rustc version
3. Compare against `plugin_api::RUSTC_VERSION` — **fail on mismatch**
4. `plugin_create()` (extern "C" thunk, returns `*mut dyn Plugin`) — construct the plugin
5. Reconstruct `Box<dyn Plugin>` from the raw pointer
6. Call `plugin.id()` and validate the identifier format
7. Check that no loaded plugin has the same `id` — **fail if duplicate**
8. Call `plugin.init()`

The version check uses raw C ABI (no Rust ABI involved) so it works even across rustc versions. All subsequent calls use Rust ABI and assume layout compatibility.

## Unloading

Plugins are unloaded automatically when:

- The user clicks "卸载所有插件"
- The `LoadedPlugin` is dropped (e.g. `plugins.clear()`)

Immediately before the plugin is destroyed:

1. `plugin.uninit()` is called — the library is still loaded, so all vtables are valid
2. The `Box<dyn Plugin>` is dropped (destructor runs, vtable accessible)
3. `dlclose()` unloads the library

This guarantees that cleanup code in `uninit()` or `Drop` can safely call any plugin function.

## Loading Plugins at Runtime

1. Open Settings → Plugins section
2. Click "加载插件" (Load Plugin)
3. Select `.so` / `.dylib` / `.dll` file
4. The plugin appears in the list with name, version, and id
5. The plugin's UI renders in its own egui window(s)

## Safety Considerations

- The **user** is responsible for ensuring the plugin was compiled with the same rustc version.
- A version mismatch produces a toast error and the plugin is not loaded.
- A duplicate `id` produces a toast error and the plugin is not loaded.
- The `Library` handle must outlive the `Box<dyn Plugin>` because the plugin's vtables point into the library's memory.
- The `plugin` field is declared before `_library` in `LoadedPlugin` so Rust drops it first — reversing this causes SIGSEGV.
- Plugins run in the same process and have full access to the host's memory. Only load plugins from trusted sources.

## Example

See `crates/example_plugin/` for a complete working example.
