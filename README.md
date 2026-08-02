# uwu - ujhhgtg's whiteboard, unleashed

a high-performance whiteboard app written in rust

reinventing the wheel because ~~others suck~~ why not

## features

- lightning-fast startup speed

- good frame rates

- multi-touch support for every tool

- overlay mode with 100% feature parity with standard mode

- first-class linux support

- awesome name

## building

### prepare

```bash
rustup toolchain install nightly
rustup default nightly

# --- system deps ---
yay -S alsa-lib gtk3 libappindicator xdotool pkgconf
# or for debian-based
sudo apt install libasound2-dev libglib2.0-dev libgtk-3-dev libappindicator3-dev libxdo-dev pkg-config
# --- end ---
```

### compile

```bash
cargo build --release
# or with profiling
cargo build --release --no-default-features --features profiling
```

the `profiling` feature implies `embedded_font`, so the profiling build embeds
the bundled CJK font and does not depend on system fonts.

### cross-compiling for windows from linux

#### prepare

good luck figuring this out if you're not using arch (download & install manually from [llvm-mingw releases](https://github.com/mstorsjo/llvm-mingw/releases))

```bash
# first add chaotic-aur, then
yay -S llvm-mingw llvm lld

rustup target add x86_64-pc-windows-gnullvm aarch64-pc-windows-gnullvm
export PATH=/opt/llvm-mingw/bin/:$PATH
```

#### compile x86_64

```bash
cargo build --release --target x86_64-pc-windows-gnullvm
```

#### compile aarch64

```bash
cargo build --release --target aarch64-pc-windows-gnullvm
```

## tech stack

egui + wgpu + winit

## supported platforms

- windows

- macos (untested)

- linux (mouse passthrough support might vary from DE/WMs; tested: GNOME (mutter), KDE (KWin), Hyprland)

## credits

[noto cjk](https://github.com/notofonts/noto-cjk)

[maple mono](https://github.com/subframe7536/Maple-font)

## license

gpl 3
