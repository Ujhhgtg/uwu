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
