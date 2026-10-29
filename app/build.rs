//! Embeds the app icon in the Windows executable (Explorer, taskbar, Alt-Tab).

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=icons/icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/icon.ico");
        res.compile().expect("embed the Windows icon");
    }
}
