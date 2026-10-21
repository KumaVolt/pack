//! Pack desktop app. The `pack` daemon (shipped next to this executable)
//! does the work; the app makes sure it runs, shows the dashboard in a native
//! window and lives in the menu bar / system tray. Closing the window keeps
//! Pack — and your sites — running.

#![cfg_attr(windows, windows_subsystem = "windows")]

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

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// The `pack` binary: bundled next to this executable, else a dev build or PATH.
fn pack_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe_path| exe_path.parent().map(|d| d.join(exe("pack"))))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
        .unwrap_or_else(|| PathBuf::from(exe("pack")))
}

fn start_daemon() {
    if daemon_up() {
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
    body.split("{\"").filter_map(|chunk| Some((field(chunk, "host")?, field(chunk, "url")?))).collect()
}

fn open_url(url: &str) {
    let _ = local_post_json("/api/open", &format!("{{\"url\":\"{url}\"}}"));
}

fn local_post_json(path: &str, json: &str) -> Option<()> {
    let mut s = TcpStream::connect_timeout(&([127, 0, 0, 1], PORT).into(), Duration::from_millis(500)).ok()?;
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
        json.len()
    )
    .ok()?;
    let mut sink = String::new();
    let _ = s.read_to_string(&mut sink);
    Some(())
}

fn show(app: &AppHandle, page: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.eval(&format!("location.hash = '{page}'"));
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let site_items: Vec<MenuItem<_>> = sites()
        .into_iter()
        .map(|(host, url)| MenuItem::with_id(app, format!("site:{url}"), host, true, None::<&str>))
        .collect::<Result<_, _>>()?;
    let site_refs: Vec<&dyn tauri::menu::IsMenuItem<_>> = site_items.iter().map(|i| i as _).collect();
    let sites_menu = Submenu::with_items(app, "Sites", !site_refs.is_empty(), &site_refs)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open Pack", true, None::<&str>)?,
            &sites_menu,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "page:sites", "Sites", true, None::<&str>)?,
            &MenuItem::with_id(app, "page:services", "Services", true, None::<&str>)?,
            &MenuItem::with_id(app, "page:mail", "Mail", true, None::<&str>)?,
            &MenuItem::with_id(app, "page:dumps", "Dumps", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "restart", "Restart services", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit (sites keep running)", true, None::<&str>)?,
        ],
    )?;
    // macOS: monochrome template icon that follows the menu bar's colour.
    // Elsewhere the coloured app icon reads better on light and dark taskbars.
    let (icon, template) = if cfg!(target_os = "macos") {
        (tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?, true)
    } else {
        (app.default_window_icon().cloned().expect("app icon"), false)
    };
    TrayIconBuilder::with_id("pack")
        .icon(icon)
        .icon_as_template(template)
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
                    let url = id[5..].to_string();
                    std::thread::spawn(move || open_url(&url));
                }
                _ => {}
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    // `--background`: start in the tray only (used when launched at login).
    let background = std::env::args().any(|a| a == "--background");
    tauri::Builder::default()
        .setup(move |app| {
            std::thread::spawn(start_daemon);
            let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Pack")
                .inner_size(1180.0, 780.0)
                .min_inner_size(820.0, 540.0)
                .visible(!background);
            #[cfg(target_os = "macos")]
            {
                use tauri::window::{Effect, EffectsBuilder};
                builder = builder
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .hidden_title(true)
                    .transparent(true)
                    .effects(EffectsBuilder::new().effect(Effect::Sidebar).build());
            }
            #[cfg(windows)]
            {
                use tauri::window::{Effect, EffectsBuilder};
                builder = builder.transparent(true).effects(EffectsBuilder::new().effect(Effect::Mica).build());
            }
            let win = builder.build()?;
            let w = win.clone();
            win.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w.hide();
                }
            });
            // The site list in the tray needs the daemon; build it once it answers.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    if daemon_up() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let h = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Err(e) = build_tray(&h) {
                        eprintln!("tray: {e}");
                    }
                });
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Pack")
        .run(|app, event| {
            // Clicking the Dock icon brings the window back.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show(app, "overview");
            }
            let _ = (app, event);
        });
}
