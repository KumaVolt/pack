//! Starts the `pack` daemon if it isn't running. The binary ships next to this
//! executable; a dev build or `pack` on PATH works too.

use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

pub fn up() -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], crate::api::PORT).into(), Duration::from_millis(200)).is_ok()
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// The `pack` binary: bundled next to this executable, else a dev build or PATH.
fn pack_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe_path| exe_path.parent().map(|d| d.join(exe("pack"))))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0))
        .unwrap_or_else(|| PathBuf::from(exe("pack")))
}

pub fn start() {
    if up() {
        return;
    }
    let mut cmd = Command::new(pack_path());
    cmd.arg("daemon").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    if let Err(e) = cmd.spawn() {
        eprintln!("could not start the Pack daemon ({}): {e}", pack_path().display());
    }
}
