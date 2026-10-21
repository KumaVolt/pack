//! Pack desktop shell. The daemon (`pack daemon`) does all the work; this app
//! makes sure it's running, shows its dashboard and lives in the menu bar.
//! Closing the window keeps Pack (and your sites) running.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const PORT: u16 = 7171;

fn daemon_up() -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(200)).is_ok()
}

/// The `pack` binary sits next to this executable (cargo target dir, or the
/// app bundle's MacOS dir); fall back to Pack's bin dir, then PATH.
fn pack_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("pack"))),
        Some(PathBuf::from(home).join("Library/Application Support/Pack/bin/pack")),
    ];
    candidates.into_iter().flatten().find(|p| p.exists()).unwrap_or_else(|| PathBuf::from("pack"))
}

fn ensure_daemon() -> Result<(), String> {
    if daemon_up() {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(pack_path());
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start the Pack daemon: {e}"))?;
    for _ in 0..50 {
        if daemon_up() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("Pack daemon did not come up on port {PORT}"))
}

/// Tiny HTTP client for the local API (avoids pulling in an HTTP stack).
fn local(method: &str, path: &str) -> Option<String> {
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let body = if method == "POST" { "{}" } else { "" };
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.split_once("\r\n\r\n").map(|(_, b)| b.to_string())
}

/// (host, url) pairs from /api/sites, without a JSON dependency.
fn sites() -> Vec<(String, String)> {
    let Some(body) = local("GET", "/api/sites") else { return vec![] };
    let field = |chunk: &str, key: &str| -> Option<String> {
        let start = chunk.find(&format!("\"{key}\":\""))? + key.len() + 4;
        Some(chunk[start..].split('"').next()?.to_string())
    };
    body.split("{\"")
        .filter_map(|chunk| Some((field(chunk, "host")?, field(chunk, "url")?)))
        .collect()
}

fn show(app: &AppHandle, hash: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.eval(&format!("location.hash = '{hash}'"));
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            ensure_daemon()?;
            let url = format!("http://127.0.0.1:{PORT}/").parse().unwrap();
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Pack")
                .inner_size(1180.0, 800.0)
                .min_inner_size(760.0, 520.0)
                .build()?;
            let w = win.clone();
            win.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w.hide();
                }
            });

            let handle = app.handle();
            let site_items: Vec<MenuItem<_>> = sites()
                .into_iter()
                .map(|(host, url)| MenuItem::with_id(handle, format!("site:{url}"), host, true, None::<&str>))
                .collect::<Result<_, _>>()?;
            let site_refs: Vec<&dyn tauri::menu::IsMenuItem<_>> = site_items.iter().map(|i| i as _).collect();
            let sites_menu = Submenu::with_items(handle, "Sites", !site_refs.is_empty(), &site_refs)?;
            let menu = Menu::with_items(
                handle,
                &[
                    &MenuItem::with_id(handle, "open", "Open Pack", true, None::<&str>)?,
                    &sites_menu,
                    &PredefinedMenuItem::separator(handle)?,
                    &MenuItem::with_id(handle, "page:mail", "Mail", true, None::<&str>)?,
                    &MenuItem::with_id(handle, "page:dumps", "Dumps", true, None::<&str>)?,
                    &MenuItem::with_id(handle, "page:services", "Services", true, None::<&str>)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &MenuItem::with_id(handle, "restart", "Restart all", true, None::<&str>)?,
                    &MenuItem::with_id(handle, "quit", "Quit Pack (sites keep running)", true, None::<&str>)?,
                ],
            )?;
            TrayIconBuilder::with_id("pack")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("Pack")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    let id = event.id().as_ref();
                    match id {
                        "open" => show(app, "overview"),
                        "restart" => {
                            std::thread::spawn(|| local("POST", "/api/restart"));
                        }
                        "quit" => app.exit(0),
                        _ if id.starts_with("page:") => show(app, &id[5..]),
                        _ if id.starts_with("site:") => {
                            let _ = Command::new("open").arg(&id[5..]).spawn();
                        }
                        _ => {}
                    }
                })
                .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Pack");
}
