/// Returns `Some(true)` if dark mode is active, `Some(false)` if light,
/// or `None` if it cannot be determined.
pub fn is_dark_mode() -> Option<bool> {
    platform::detect()
}

// ─── Windows ────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    pub fn detect() -> Option<bool> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            .ok()?;

        // 0 = dark, 1 = light
        let light: u32 = key.get_value("AppsUseLightTheme").ok()?;
        Some(light == 0)
    }
}

// ─── macOS ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod platform {
    use std::process::Command;

    pub fn detect() -> Option<bool> {
        // `defaults` exits non-zero in light mode (key absent) and prints
        // "Dark\n" in dark mode.
        let out = Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .output()
            .ok()?;

        Some(out.status.success() && out.stdout.starts_with(b"Dark"))
    }
}

// ─── Linux ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    pub fn detect() -> Option<bool> {
        let de = std::env::var("XDG_CURRENT_DESKTOP")
            .or_else(|_| std::env::var("DESKTOP_SESSION"))
            .unwrap_or_default()
            .to_lowercase();

        if is_gnome_like(&de) {
            gnome::detect()
        } else if is_kde_like(&de) {
            kde::detect()
        } else {
            // Unknown DE: try both, prefer gsettings
            gnome::detect().or_else(kde::detect)
        }
    }

    fn is_gnome_like(de: &str) -> bool {
        [
            "gnome", "unity", "budgie", "cinnamon", "mate", "lxde", "xfce",
        ]
        .iter()
        .any(|s| de.contains(s))
    }

    fn is_kde_like(de: &str) -> bool {
        ["kde", "plasma", "lxqt"].iter().any(|s| de.contains(s))
    }

    // ── GNOME / GTK ─────────────────────────────────────────────────────────

    mod gnome {
        use std::process::Command;

        pub fn detect() -> Option<bool> {
            // Prefer the dedicated color-scheme key (GNOME 42+)
            if let Some(result) = read_color_scheme() {
                return Some(result);
            }
            // Fall back to inspecting the GTK theme name
            read_gtk_theme()
        }

        fn gsettings(schema: &str, key: &str) -> Option<String> {
            let out = Command::new("gsettings")
                .args(["get", schema, key])
                .output()
                .ok()?;
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
            } else {
                None
            }
        }

        fn read_color_scheme() -> Option<bool> {
            let val = gsettings("org.gnome.desktop.interface", "color-scheme")?;
            // Possible values: 'default', 'prefer-dark', 'prefer-light'
            if val.contains("dark") {
                Some(true)
            } else if val.contains("light") || val.contains("default") {
                Some(false)
            } else {
                None
            }
        }

        fn read_gtk_theme() -> Option<bool> {
            let val = gsettings("org.gnome.desktop.interface", "gtk-theme")?;
            // Theme names like "Adwaita-dark", "Yaru-dark", etc.
            Some(val.contains("dark"))
        }
    }

    // ── KDE / Qt ─────────────────────────────────────────────────────────────

    mod kde {
        use std::{fs, path::PathBuf};

        pub fn detect() -> Option<bool> {
            let home = std::env::var("HOME").ok()?;
            let config = PathBuf::from(&home).join(".config");

            // 1. kdeglobals → [General] → ColorScheme (e.g. "BreezeDark")
            if let Some(r) = kdeglobals(&config) {
                return Some(r);
            }

            // 2. qt6ct, then qt5ct → [Appearance] → color_scheme_path
            for qt_ct in ["qt6ct/qt6ct.conf", "qt5ct/qt5ct.conf"] {
                if let Some(r) = qt_ct_config(&config.join(qt_ct)) {
                    return Some(r);
                }
            }

            None
        }

        fn kdeglobals(config: &std::path::Path) -> Option<bool> {
            let content = fs::read_to_string(config.join("kdeglobals")).ok()?;
            let mut in_general = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_general = trimmed.eq_ignore_ascii_case("[General]");
                    continue;
                }
                if in_general
                    && let Some(val) = trimmed
                        .strip_prefix("ColorScheme=")
                        .or_else(|| trimmed.strip_prefix("colorScheme="))
                {
                    return Some(val.to_lowercase().contains("dark"));
                }
            }
            None
        }

        fn qt_ct_config(path: &std::path::Path) -> Option<bool> {
            let content = fs::read_to_string(path).ok()?;
            let mut in_appearance = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_appearance = trimmed.eq_ignore_ascii_case("[Appearance]");
                    continue;
                }
                if in_appearance && let Some(val) = trimmed.strip_prefix("color_scheme_path=") {
                    // Path ends with e.g. "/darker.conf", "/DarkBreeze.conf"
                    return Some(val.to_lowercase().contains("dark"));
                }
            }
            None
        }
    }
}

// ─── Unsupported platform ────────────────────────────────────────────────────

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn detect() -> Option<bool> {
        None
    }
}
