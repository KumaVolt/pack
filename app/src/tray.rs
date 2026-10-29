//! Menu bar / system tray icon. Its menu events are read by `main` on the
//! GPUI thread; see `Command`.

use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub enum Command {
    Show(&'static str),
    OpenUrl(String),
    Restart,
    Quit,
}

/// The next menu click, if any.
pub fn next_command() -> Option<Command> {
    let event = MenuEvent::receiver().try_recv().ok()?;
    let id = event.id.as_ref();
    Some(match id {
        "open" => Command::Show("overview"),
        "restart" => Command::Restart,
        "quit" => Command::Quit,
        _ if id.starts_with("site:") => Command::OpenUrl(id[5..].to_string()),
        _ => Command::Show(crate::ui::Page::from_name(id.strip_prefix("page:")?)?.name()),
    })
}

fn rgba(png_bytes: &[u8]) -> Icon {
    let mut reader = png::Decoder::new(std::io::Cursor::new(png_bytes)).read_info().expect("tray icon");
    let mut buf = vec![0; reader.output_buffer_size().expect("tray icon size")];
    let info = reader.next_frame(&mut buf).expect("tray icon pixels");
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).expect("tray icon")
}

/// Builds the tray icon with a submenu of `(host, url)` sites. Keep the
/// returned icon alive for as long as it should show.
pub fn build(sites: &[(String, String)]) -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let site_items: Vec<MenuItem> =
        sites.iter().map(|(host, url)| MenuItem::with_id(format!("site:{url}"), host, true, None)).collect();
    let site_refs: Vec<&dyn IsMenuItem> = site_items.iter().map(|i| i as &dyn IsMenuItem).collect();
    let sites_menu = Submenu::with_items("Sites", !site_refs.is_empty(), &site_refs)?;
    let menu = Menu::with_items(&[
        &MenuItem::with_id("open", "Open Pack", true, None),
        &sites_menu,
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("page:sites", "Sites", true, None),
        &MenuItem::with_id("page:services", "Services", true, None),
        &MenuItem::with_id("page:mail", "Mail", true, None),
        &MenuItem::with_id("page:dumps", "Dumps", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("restart", "Restart services", true, None),
        &MenuItem::with_id("quit", "Quit (sites keep running)", true, None),
    ])?;
    let builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Pack");
    // macOS: monochrome template icon that follows the menu bar's colour.
    // Elsewhere the coloured app icon reads better on light and dark taskbars.
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_templated(rgba(include_bytes!("../icons/tray.png")));
    #[cfg(not(target_os = "macos"))]
    let builder = builder.with_icon(rgba(include_bytes!("../icons/64x64.png")));
    Ok(builder.build()?)
}
