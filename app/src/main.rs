//! Pack desktop app. The `pack` daemon (shipped next to this executable)
//! does the work; the app makes sure it runs, shows it in a native GPUI window
//! and lives in the menu bar / system tray. Closing the window keeps Pack —
//! and your sites — running.
//!
//! `--background` starts in the tray only (used when launched at login).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod api;
mod daemon;
#[cfg(feature = "screenshots")]
mod screenshots;
mod tray;
mod ui;

use gpui_kit::*;
use std::time::Duration;
use ui::Page;

actions!(pack, [Quit, CloseWindow]);

/// App-wide switches.
#[derive(Default)]
pub struct Options {
    /// Keep the theme the app started with instead of following the system.
    pub fixed_theme: bool,
}
impl Global for Options {}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    #[cfg(feature = "screenshots")]
    if let Some(i) = args.iter().position(|a| a == "--screenshots") {
        return screenshots::run(args[i + 1..].to_vec());
    }
    let background = args.iter().any(|a| a == "--background");
    std::thread::spawn(daemon::start);

    let app = gpui_kit::application().with_assets(ui::Assets).with_quit_mode(QuitMode::Explicit);
    // Clicking the Dock icon brings the window back.
    app.on_reopen(|cx| ui::show(Page::Overview, cx));
    app.run(move |cx| {
        init(cx);
        if !background {
            ui::show(Page::Overview, cx);
        }
        start_tray(cx);
    });
}

pub fn init(cx: &mut App) {
    gpui_kit::init(cx);
    cx.set_global(Options::default());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = cx.active_window() {
            window.update(cx, |_, window, _| window.remove_window()).ok();
        }
    });
    let (quit, close) = if cfg!(target_os = "macos") { ("cmd-q", "cmd-w") } else { ("ctrl-q", "ctrl-w") };
    cx.bind_keys([KeyBinding::new(quit, Quit, None), KeyBinding::new(close, CloseWindow, None)]);
    cx.set_menus(vec![Menu {
        name: "Pack".into(),
        items: vec![MenuItem::action("Close Window", CloseWindow), MenuItem::separator(), MenuItem::action("Quit Pack", Quit)],
        disabled: false,
    }]);
}

/// Waits for the daemon (the tray lists its sites), then shows the tray icon
/// and handles its menu.
fn start_tray(cx: &mut App) {
    let sites = cx.background_spawn(async {
        for _ in 0..100 {
            if daemon::up() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        api::get::<api::Sites>("/api/sites").map(|d| d.sites.into_iter().map(|s| (s.host, s.url)).collect::<Vec<_>>()).unwrap_or_default()
    });
    cx.spawn(async move |cx| {
        let sites = sites.await;
        // GTK runs the tray on Linux, on a thread of its own.
        #[cfg(target_os = "linux")]
        std::thread::spawn(move || {
            if let Err(e) = gtk::init() {
                eprintln!("tray: {e}");
                return;
            }
            match tray::build(&sites) {
                Ok(icon) => {
                    let _icon = icon;
                    gtk::main();
                }
                Err(e) => eprintln!("tray: {e}"),
            }
        });
        #[cfg(not(target_os = "linux"))]
        let _icon = match tray::build(&sites) {
            Ok(icon) => Some(icon),
            Err(e) => {
                eprintln!("tray: {e}");
                None
            }
        };
        loop {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            while let Some(command) = tray::next_command() {
                cx.update(|cx| match command {
                    tray::Command::Show(page) => ui::show(Page::from_name(page).unwrap_or(Page::Overview), cx),
                    tray::Command::OpenUrl(url) => cx.open_url(&url),
                    tray::Command::Restart => cx.background_spawn(async { api::post("/api/restart", serde_json::json!({})) }).detach(),
                    tray::Command::Quit => cx.quit(),
                });
            }
        }
    })
    .detach();
}
