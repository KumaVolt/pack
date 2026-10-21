//! Downloading and unpacking binaries, with progress the UI can poll.

use crate::paths;
use anyhow::{Context, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub label: String,
    pub done: u64,
    pub total: Option<u64>,
}

static PROGRESS: Mutex<BTreeMap<String, Progress>> = Mutex::new(BTreeMap::new());

pub fn progress() -> Vec<Progress> {
    PROGRESS.lock().unwrap().values().cloned().collect()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("pack/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub async fn text(url: &str) -> anyhow::Result<String> {
    let res = client().get(url).send().await?.error_for_status()?;
    Ok(res.text().await?)
}

/// Latest release tag of a GitHub repo, read from the /releases/latest
/// redirect (the REST API allows only 60 anonymous calls an hour).
pub async fn latest_github_tag(repo: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
    let res = client.head(format!("https://github.com/{repo}/releases/latest")).send().await?;
    let location = res
        .headers()
        .get("location")
        .and_then(|l| l.to_str().ok())
        .with_context(|| format!("no latest release for {repo}"))?;
    let tag = location.rsplit("/tag/").next().unwrap_or_default();
    if tag.is_empty() || tag == location {
        bail!("no latest release for {repo}");
    }
    Ok(tag.to_string())
}

/// Download `url` into the downloads cache and return the file path.
pub async fn fetch(url: &str, label: &str) -> anyhow::Result<PathBuf> {
    // Cache key from the whole URL: some projects reuse file names across versions.
    let name: String = url
        .split("://")
        .last()
        .unwrap_or(url)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let dest = paths::downloads().join(&name);
    if dest.exists() {
        return Ok(dest);
    }
    std::fs::create_dir_all(paths::downloads())?;
    let mut res = client()
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !res.status().is_success() {
        bail!("download failed: {url} returned {}", res.status());
    }
    let key = url.to_string();
    let total = res.content_length();
    PROGRESS.lock().unwrap().insert(key.clone(), Progress { label: label.into(), done: 0, total });
    let part = dest.with_extension("part");
    let result: anyhow::Result<()> = async {
        let mut file = tokio::fs::File::create(&part).await?;
        let mut done = 0u64;
        while let Some(chunk) = res.chunk().await? {
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            if let Some(p) = PROGRESS.lock().unwrap().get_mut(&key) {
                p.done = done;
            }
        }
        file.flush().await?;
        Ok(())
    }
    .await;
    PROGRESS.lock().unwrap().remove(&key);
    result?;
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// Unpack a .tar.gz (every Pack package is one), optionally dropping leading
/// path components. Pure Rust, so it behaves the same on every OS.
pub async fn extract(archive: &Path, into: &Path, strip_components: usize) -> anyhow::Result<()> {
    let archive = archive.to_path_buf();
    let into = into.to_path_buf();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        std::fs::create_dir_all(&into)?;
        let file = std::fs::File::open(&archive).with_context(|| format!("opening {}", archive.display()))?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)));
        tar.set_preserve_permissions(true);
        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            let rel: std::path::PathBuf = path.components().skip(strip_components).collect();
            if rel.as_os_str().is_empty() {
                continue;
            }
            if rel.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_))) {
                bail!("unsafe path in {}: {}", archive.display(), path.display());
            }
            let dest = into.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            entry.unpack(&dest).with_context(|| format!("unpacking {}", rel.display()))?;
        }
        Ok(())
    })
    .await?
}

pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = path;
    Ok(())
}

