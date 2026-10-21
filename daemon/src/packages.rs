//! Every binary Pack runs (PHP, Caddy, MySQL, Postgres, Valkey, ...) comes
//! from the Pack repo: `dist/manifest.json` lists release assets with
//! SHA-256, built by `scripts/packages.py` in CI.
//!
//! Layout: `packages/<name>/<series>/<version>/` plus a `current` symlink.
//! Updating a series installs the new version next to the old one, flips the
//! symlink and restarts whatever uses it; the previous version is removed
//! once the switch succeeded.

use crate::{download, paths};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The GitHub repo that publishes Pack and its packages.
pub const REPO: &str = "KumaVolt/pack";
pub const DEFAULT_MANIFEST: &str = "https://raw.githubusercontent.com/KumaVolt/pack/main/dist/manifest.json";
const MAX_AGE: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub series: String,
    pub version: String,
    pub arch: String,
    pub url: String,
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub generated: String,
    #[serde(default)]
    pub packages: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub name: String,
    pub series: String,
    pub version: String,
    pub latest: Option<String>,
    pub update: bool,
}

pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" }
}

fn source() -> String {
    std::env::var("PACK_MANIFEST")
        .ok()
        .or_else(|| {
            let c = crate::config::Config::load();
            (!c.manifest_url.is_empty()).then_some(c.manifest_url)
        })
        .unwrap_or_else(|| DEFAULT_MANIFEST.to_string())
}

fn cache_file() -> PathBuf {
    paths::home().join("manifest.json")
}

/// The package manifest: cached copy if fresh, otherwise fetched. Falls back
/// to the cache when offline.
pub async fn manifest(refresh: bool) -> anyhow::Result<Manifest> {
    let cache = cache_file();
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < MAX_AGE);
    if fresh && !refresh {
        if let Ok(m) = read_cache() {
            return Ok(m);
        }
    }
    let src = source();
    let fetched: anyhow::Result<String> = if is_local(&src) {
        std::fs::read_to_string(local_path(&src)).with_context(|| format!("reading {src}"))
    } else {
        download::text(&src).await
    };
    match fetched.and_then(|t| Ok((serde_json::from_str::<Manifest>(&t)?, t))) {
        Ok((m, raw)) => {
            std::fs::write(&cache, raw)?;
            Ok(m)
        }
        Err(e) => read_cache().map_err(|_| e.context(format!("could not load the package manifest from {src}"))),
    }
}

fn read_cache() -> anyhow::Result<Manifest> {
    Ok(serde_json::from_slice(&std::fs::read(cache_file())?)?)
}

fn is_local(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("file://")
}
fn local_path(s: &str) -> &str {
    s.strip_prefix("file://").unwrap_or(s)
}

impl Manifest {
    fn for_host<'a, 'n>(&'a self, name: &'n str) -> impl Iterator<Item = &'a Entry> + use<'a, 'n> {
        self.packages.iter().filter(move |e| e.name == name && (e.arch == host_arch() || e.arch == "any"))
    }

    /// Series of a package available for this Mac, newest first.
    pub fn series(&self, name: &str) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for e in self.for_host(name) {
            if !out.contains(&e.series) {
                out.push(e.series.clone());
            }
        }
        out.sort_by(|a, b| key(b).cmp(&key(a)));
        out
    }

    pub fn latest(&self, name: &str, series: &str) -> Option<&Entry> {
        self.for_host(name).filter(|e| e.series == series).max_by_key(|e| key(&e.version))
    }

    pub fn exact(&self, name: &str, version: &str) -> Option<&Entry> {
        self.for_host(name).find(|e| e.version == version)
    }

    /// Every series of every package, for the UI.
    pub fn catalog(&self) -> std::collections::BTreeMap<String, Vec<(String, String)>> {
        let mut map = std::collections::BTreeMap::new();
        let names: std::collections::BTreeSet<&str> = self.packages.iter().map(|e| e.name.as_str()).collect();
        for name in names {
            let list: Vec<(String, String)> = self
                .series(name)
                .into_iter()
                .filter_map(|s| self.latest(name, &s).map(|e| (s.clone(), e.version.clone())))
                .collect();
            map.insert(name.to_string(), list);
        }
        map
    }
}

pub fn key(v: &str) -> Vec<u64> {
    v.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect()
}

pub fn series_dir(name: &str, series: &str) -> PathBuf {
    paths::home().join("packages").join(name).join(series)
}

/// Where the active version of a series lives (a symlink).
pub fn current(name: &str, series: &str) -> PathBuf {
    series_dir(name, series).join("current")
}

pub fn installed_version(name: &str, series: &str) -> Option<String> {
    let target = std::fs::read_link(current(name, series)).ok()?;
    Some(target.file_name()?.to_string_lossy().to_string())
}

pub fn is_installed(name: &str, series: &str) -> bool {
    installed_version(name, series).is_some() && current(name, series).exists()
}

/// Everything installed, with update info from the manifest.
pub fn installed(m: &Manifest) -> Vec<Installed> {
    let mut out = vec![];
    let root = paths::home().join("packages");
    for name in read_dirs(&root) {
        for series in read_dirs(&root.join(&name)) {
            let Some(version) = installed_version(&name, &series) else { continue };
            let latest = m.latest(&name, &series).map(|e| e.version.clone());
            let update = latest.as_ref().is_some_and(|l| key(l) > key(&version));
            out.push(Installed { name: name.clone(), series, version, latest, update });
        }
    }
    out
}

fn read_dirs(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            // Real directories only: skip the `current` symlink and temp dirs.
            !name.starts_with('.') && e.file_type().is_ok_and(|t| t.is_dir())
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

/// Install the newest build of `name`/`series` (or a specific version) and
/// make it current. Returns the version that is now active.
pub async fn install(name: &str, series: &str, version: Option<&str>) -> anyhow::Result<String> {
    let m = manifest(false).await?;
    let entry = match version {
        Some(v) => m.exact(name, v).cloned(),
        None => match m.latest(name, series) {
            Some(e) => Some(e.clone()),
            // A stale cache may not know a brand-new series yet.
            None => manifest(true).await?.latest(name, series).cloned(),
        },
    };
    let entry = entry.with_context(|| {
        let have = m.series(name);
        if have.is_empty() {
            format!("{name} is not available for this Mac ({}) in the package manifest", host_arch())
        } else {
            format!("{name} {series} is not available — choose one of: {}", have.join(", "))
        }
    })?;
    if installed_version(name, series).as_deref() == Some(entry.version.as_str()) {
        return Ok(entry.version);
    }
    let dir = series_dir(name, series);
    let target = dir.join(&entry.version);
    if !target.join(".pack-package").exists() {
        let archive = fetch(&entry).await?;
        let tmp = dir.join(format!(".{}.tmp", entry.version));
        let _ = std::fs::remove_dir_all(&tmp);
        download::extract(&archive, &tmp, 0).await?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&tmp, &target)?;
        let _ = std::fs::remove_file(&archive);
    }
    // Atomically point `current` at the new version.
    let link = current(name, series);
    let tmp_link = dir.join(".current.tmp");
    let _ = std::fs::remove_file(&tmp_link);
    std::os::unix::fs::symlink(&entry.version, &tmp_link)?;
    std::fs::rename(&tmp_link, &link)?;
    Ok(entry.version)
}

/// Remove versions other than the current one.
pub fn prune(name: &str, series: &str) {
    let Some(active) = installed_version(name, series) else { return };
    for v in read_dirs(&series_dir(name, series)) {
        if v != active {
            let _ = std::fs::remove_dir_all(series_dir(name, series).join(v));
        }
    }
}

pub fn uninstall(name: &str, series: &str) -> anyhow::Result<()> {
    let dir = series_dir(name, series);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

async fn fetch(entry: &Entry) -> anyhow::Result<PathBuf> {
    let label = format!("{} {}", entry.name, entry.version);
    let file = if is_local(&entry.url) {
        PathBuf::from(local_path(&entry.url))
    } else {
        download::fetch(&entry.url, &label).await?
    };
    let bytes = tokio::fs::read(&file).await?;
    let digest = tokio::task::spawn_blocking(move || {
        let mut h = Sha256::new();
        h.update(&bytes);
        format!("{:x}", h.finalize())
    })
    .await?;
    if digest != entry.sha256 {
        if !is_local(&entry.url) {
            let _ = std::fs::remove_file(&file);
        }
        bail!("checksum mismatch for {label}: expected {}, got {digest}", entry.sha256);
    }
    if is_local(&entry.url) {
        // Never delete the source of a local manifest; work on a copy.
        let copy = paths::downloads().join(format!("{}-{}.tar.gz", entry.name, entry.version));
        std::fs::copy(&file, &copy)?;
        return Ok(copy);
    }
    Ok(file)
}
