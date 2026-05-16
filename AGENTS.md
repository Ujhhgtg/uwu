# uwu (ujhhgtg's whiteboard, unleashed)

A high-performance digital whiteboard app, designed for touchscreen devices.

## Toolchain

- **Rust nightly** required (edition 2024).

## Build

```bash
cargo build            # debug
cargo build --release  # release
```

## Lint

```bash
cargo clippy --release
```

## Code Navigation

Use the LSP tool for code navigation whenever possible (you need to read at least one source file to activate LSP tool) — it's more precise than text search for finding definitions, references, symbol locations, and call hierarchies.

## Architecture

- GUI app using **egui + wgpu + winit**
- Entrypoint: `src/main.rs`
- States: `src/state/mod.rs`
- rkyv serialization states: `src/state/flat.rs`
- Rendering: `src/render.rs`
- App logic: `src/app.rs`
- Utilities: `src/utils/*.rs`
- UI content: `src/ui.rs`
