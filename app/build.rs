//! Tauri needs the `pack` sidecar at binaries/pack-<target-triple>. Release
//! bundles get it from scripts/build-app; for plain `cargo build` copy a
//! locally built one if there is one, else leave a placeholder (the app then
//! finds pack next to itself or on PATH).
fn main() {
    let target = std::env::var("TARGET").unwrap();
    let ext = if target.contains("windows") { ".exe" } else { "" };
    let sidecar = std::path::PathBuf::from(format!("binaries/pack-{target}{ext}"));
    if !sidecar.exists() {
        std::fs::create_dir_all("binaries").unwrap();
        let built = ["../target/release", "../target/debug"]
            .iter()
            .map(|d| std::path::Path::new(d).join(format!("pack{ext}")))
            .find(|p| p.exists());
        match built {
            Some(p) => {
                std::fs::copy(p, &sidecar).unwrap();
            }
            None => std::fs::write(&sidecar, b"").unwrap(),
        }
    }
    println!("cargo:rerun-if-changed=binaries");
    tauri_build::build()
}
