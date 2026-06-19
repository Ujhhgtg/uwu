use std::path::PathBuf;

#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winreg::enums::*;

#[cfg(windows)]
pub fn install_associations() -> Result<(), Box<dyn std::error::Error>> {
    let exe_path = std::env::current_exe()?;
    let exe_str = exe_path.to_str().ok_or("invalid exe path")?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    // Create/open HKCU\Software\Classes\.owo
    let (owo_key, _) = hkcu.create_subkey(r"Software\Classes\.owo")?;
    owo_key.set_value("", &"owo.AssocFile")?;

    // Create/open HKCU\Software\Classes\owo.AssocFile
    let (assoc_key, _) = hkcu.create_subkey(r"Software\Classes\owo.AssocFile")?;
    assoc_key.set_value("", &"Open Whiteboard Objects")?;

    // Create/open HKCU\Software\Classes\owo.AssocFile\DefaultIcon
    let (icon_key, _) = hkcu.create_subkey(r"Software\Classes\owo.AssocFile\DefaultIcon")?;
    icon_key.set_value("", &format!("\"{}\",0", exe_str))?;

    // Create/open HKCU\Software\Classes\owo.AssocFile\shell\open\command
    let (cmd_key, _) = hkcu.create_subkey(r"Software\Classes\owo.AssocFile\shell\open\command")?;
    cmd_key.set_value("", &format!("\"{}\" \"%1\"", exe_str))?;

    Ok(())
}

#[cfg(windows)]
pub fn uninstall_associations() -> Result<(), Box<dyn std::error::Error>> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // Delete .owo
    let _ = hkcu.delete_subkey_all(r"Software\Classes\.owo");
    // Delete owo.AssocFile
    let _ = hkcu.delete_subkey_all(r"Software\Classes\owo.AssocFile");
    Ok(())
}

#[cfg(target_os = "linux")]
fn get_linux_applications_dir() -> PathBuf {
    let base = if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg_data_home)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        panic!("failed to find applications dir")
    };
    base.join("applications")
}

#[cfg(target_os = "linux")]
fn get_linux_mime_packages_dir() -> PathBuf {
    let base = if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg_data_home)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        panic!("failed to find mime packages dir")
    };
    base.join("mime").join("packages")
}

#[cfg(target_os = "linux")]
fn get_linux_icons_dir() -> PathBuf {
    let base = if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(xdg_data_home)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        panic!("failed to find icons dir")
    };
    base.join("icons")
}

#[cfg(target_os = "linux")]
pub fn install_associations() -> Result<(), Box<dyn std::error::Error>> {
    let applications_dir = get_linux_applications_dir();
    let mime_packages_dir = get_linux_mime_packages_dir();
    let icons_dir = get_linux_icons_dir();

    std::fs::create_dir_all(&applications_dir)?;
    std::fs::create_dir_all(&mime_packages_dir)?;
    std::fs::create_dir_all(&icons_dir)?;

    // Save icon
    let icon_path = icons_dir.join("uwu.png");
    if let Ok(icon) = image::load_from_memory(crate::assets::ICON) {
        let _ = icon.save(&icon_path);
    }

    // MIME XML
    let mime_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-owo">
    <comment>Open Whiteboard Objects</comment>
    <glob pattern="*.owo"/>
  </mime-type>
</mime-info>
"#;
    std::fs::write(mime_packages_dir.join("uwu.xml"), mime_xml)?;

    // Desktop Entry
    let exe_path = std::env::current_exe()?;
    let exe_str = exe_path.to_str().ok_or("invalid exe path")?;
    let desktop_content = format!(
        r#"[Desktop Entry]
Type=Application
Name=uwu
Comment=ujhhgtg's whiteboard, unleashed
Exec={} %f
Icon={}
Terminal=false
MimeType=application/x-owo;
Categories=Utility;Graphics;
"#,
        exe_str,
        icon_path.display()
    );
    std::fs::write(applications_dir.join("uwu.desktop"), desktop_content)?;

    // Update caches
    if let Some(mime_dir) = mime_packages_dir.parent() {
        let _ = std::process::Command::new("update-mime-database")
            .arg(mime_dir)
            .status();
    }

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications_dir)
        .status();

    let _ = std::process::Command::new("xdg-mime")
        .args(&["default", "uwu.desktop", "application/x-owo"])
        .status();

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall_associations() -> Result<(), Box<dyn std::error::Error>> {
    let applications_dir = get_linux_applications_dir();
    let mime_packages_dir = get_linux_mime_packages_dir();
    let icons_dir = get_linux_icons_dir();

    let desktop_path = applications_dir.join("uwu.desktop");
    if desktop_path.exists() {
        std::fs::remove_file(desktop_path)?;
    }

    let xml_path = mime_packages_dir.join("uwu.xml");
    if xml_path.exists() {
        std::fs::remove_file(xml_path)?;
    }

    let icon_path = icons_dir.join("uwu.png");
    if icon_path.exists() {
        std::fs::remove_file(icon_path)?;
    }

    if let Some(mime_dir) = mime_packages_dir.parent() {
        let _ = std::process::Command::new("update-mime-database")
            .arg(mime_dir)
            .status();
    }

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications_dir)
        .status();

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn check_and_update_linux_desktop_file() {
    let applications_dir = get_linux_applications_dir();
    let desktop_path = applications_dir.join("uwu.desktop");
    if !desktop_path.exists() {
        return;
    }

    let Ok(exe_path) = std::env::current_exe() else {
        return;
    };
    let Some(exe_str) = exe_path.to_str() else {
        return;
    };

    let Ok(content) = std::fs::read_to_string(&desktop_path) else {
        return;
    };

    let mut needs_update = true;
    for line in content.lines() {
        if line.starts_with("Exec=") {
            let exec_val = line["Exec=".len()..].trim();
            if exec_val == format!("{} %f", exe_str) || exec_val == exe_str {
                needs_update = false;
                break;
            }
        }
    }

    if needs_update {
        println!("existing application file is out-of-date, updating...");
        let icons_dir = get_linux_icons_dir();
        let icon_path = icons_dir.join("uwu.png");
        let new_content = format!(
            r#"[Desktop Entry]
Type=Application
Name=uwu
Comment=ujhhgtg's whiteboard, unleashed
Exec={} %f
Icon={}
Terminal=false
MimeType=application/x-owo;
Categories=Utility;Graphics;
"#,
            exe_str,
            icon_path.display()
        );
        if let Err(e) = std::fs::write(&desktop_path, new_content) {
            eprintln!("failed to update desktop file: {}", e);
            return;
        }
        let _ = std::process::Command::new("update-desktop-database")
            .arg(&applications_dir)
            .status();
    }
}

// Fallback stubs for unsupported targets
#[cfg(not(any(windows, target_os = "linux")))]
pub fn install_associations() -> Result<(), Box<dyn std::error::Error>> {
    Err("Not supported on this platform".into())
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn uninstall_associations() -> Result<(), Box<dyn std::error::Error>> {
    Err("Not supported on this platform".into())
}

#[cfg(windows)]
pub fn is_associations_installed() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(owo_key) = hkcu.open_subkey(r"Software\Classes\.owo") {
        if let Ok(val) = owo_key.get_value::<String, _>("") {
            if val == "owo.AssocFile" {
                return hkcu
                    .open_subkey(r"Software\Classes\owo.AssocFile\shell\open\command")
                    .is_ok();
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
pub fn is_associations_installed() -> bool {
    let applications_dir = get_linux_applications_dir();
    applications_dir.join("uwu.desktop").exists()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn is_associations_installed() -> bool {
    false
}
