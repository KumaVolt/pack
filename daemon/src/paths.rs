//! Filesystem layout. Everything Pack owns lives under one directory
//! (see `platform::default_home`; override with `PACK_HOME`).

use std::path::PathBuf;

pub fn home() -> PathBuf {
    if let Ok(dir) = std::env::var("PACK_HOME") {
        return PathBuf::from(dir);
    }
    crate::platform::default_home()
}

pub fn user_home() -> PathBuf {
    crate::platform::user_home()
}

pub fn config_file() -> PathBuf {
    home().join("config.json")
}
/// Shims and helper binaries that go on the user's PATH.
pub fn bin() -> PathBuf {
    home().join("bin")
}
pub fn php_root() -> PathBuf {
    home().join("php")
}
pub fn php_dir(version: &str) -> PathBuf {
    php_root().join(version)
}
pub fn run() -> PathBuf {
    home().join("run")
}
pub fn logs() -> PathBuf {
    home().join("logs")
}
pub fn caddy_dir() -> PathBuf {
    home().join("caddy")
}
/// Persistent data for a service instance.
pub fn service_data(id: &str) -> PathBuf {
    home().join("data").join(id)
}
pub fn tools() -> PathBuf {
    home().join("tools")
}
pub fn downloads() -> PathBuf {
    home().join("downloads")
}

pub fn php_socket(version: &str) -> PathBuf {
    run().join(format!("php{}.sock", version.replace('.', "")))
}

pub fn ensure_layout() -> std::io::Result<()> {
    for dir in [home(), bin(), php_root(), run(), logs(), caddy_dir(), tools(), downloads()] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// `~/Projects/foo` instead of `/Users/me/Projects/foo` for display.
pub fn tilde(path: &std::path::Path) -> String {
    let home = user_home();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
