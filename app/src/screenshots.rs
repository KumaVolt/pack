//! `pack-app --screenshots <out-dir> [light|dark] [page...]` renders pages of
//! the app to `<out-dir>/<page>.png` and `<page>-dark.png`, offscreen, from
//! the daemon that is running. Each page waits for its data first (Services:
//! every service running). Built only with the `screenshots` feature.

use crate::ui::{self, PackApp, Page};
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn run(args: Vec<String>) {
    let Some(out) = args.first().map(PathBuf::from) else {
        eprintln!("usage: pack-app --screenshots <out-dir> [light|dark] [page...]");
        std::process::exit(2);
    };
    let mut modes: Vec<ThemeMode> = args[1..]
        .iter()
        .filter_map(|a| match a.as_str() {
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            _ => None,
        })
        .collect();
    if modes.is_empty() {
        modes = vec![ThemeMode::Light, ThemeMode::Dark];
    }
    let mut pages: Vec<Page> = args[1..].iter().filter_map(|a| Page::from_name(a)).collect();
    if pages.is_empty() {
        pages = Page::ALL.to_vec();
    }
    std::fs::create_dir_all(&out).expect("output directory");

    gpui_kit::application().with_assets(ui::Assets).run(move |cx| {
        crate::init(cx);
        cx.global_mut::<crate::Options>().fixed_theme = true;
        ui::apply_theme(modes[0], cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx))),
            titlebar: Some(TitlebarOptions { title: Some("Pack".into()), appears_transparent: true, traffic_light_position: None }),
            ..Default::default()
        };
        let first = pages[0];
        let (window, view) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut app = PackApp::new(first, window, cx);
                app.demo = true;
                app
            })
        })
        .expect("window");

        cx.spawn(async move |cx| {
            let mut failed = false;
            for mode in &modes {
                cx.update(|cx| ui::apply_theme(*mode, cx));
                for page in &pages {
                    view.update(cx, |app, cx| app.go(*page, cx));
                    let mut ready = false;
                    for _ in 0..600 {
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                        // A hidden window isn't drawn by the display link.
                        window.update(cx, |_, window, cx| window.draw(cx).clear(cx)).ok();
                        if view.read_with(cx, |app, _| app.ready()) {
                            ready = true;
                            break;
                        }
                    }
                    if !ready {
                        eprintln!("{}: not ready after 60 s", page.name());
                        failed = true;
                        continue;
                    }
                    // Let images load and animations finish.
                    for _ in 0..8 {
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                        window.update(cx, |_, window, cx| window.draw(cx).clear(cx)).ok();
                    }
                    let name = format!("{}{}.png", page.name(), if mode.is_dark() { "-dark" } else { "" });
                    let file = out.join(name);
                    match window.update(cx, |_, window, _| window.render_to_image()) {
                        Ok(Ok(image)) => {
                            save_png(&file, image.width(), image.height(), image.as_raw());
                            println!("{}", file.display());
                        }
                        Ok(Err(e)) | Err(e) => {
                            eprintln!("{}: {e:#}", file.display());
                            failed = true;
                        }
                    }
                }
            }
            cx.update(|cx| cx.quit());
            if failed {
                std::process::exit(1);
            }
        })
        .detach();
    });
}

fn save_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("screenshot file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut w| w.write_image_data(rgba)).expect("write screenshot");
}
