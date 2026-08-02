use std::{env, fs, path::PathBuf};

fn main() {
    // --- startup animation ---

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let frames_dir = manifest_dir
        .join("assets")
        .join("startup_animation")
        .join("frames");

    println!("cargo:rerun-if-changed={}", frames_dir.display());

    let mut entries: Vec<_> = fs::read_dir(&frames_dir)
        .unwrap_or_else(|e| {
            panic!(
                "failed to read frames directory: {}\npath: {}",
                e,
                frames_dir.display()
            )
        })
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .collect();

    entries.sort();

    fs::create_dir_all(&out_dir).unwrap();

    let mut code = String::from("pub const STARTUP_FRAMES: &[&[u8]] = &[\n");
    for path in entries {
        code.push_str(&format!("    include_bytes!(r\"{}\"),\n", path.display()));
    }
    code.push_str("];\n");

    fs::write(out_dir.join("startup_frames.rs"), code).unwrap();

    // --- end ---
}
