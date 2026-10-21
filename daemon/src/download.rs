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

/// Unpack an archive with the system `tar` (handles .tar.gz, .tgz, .tar.xz, .zip).
pub async fn extract(archive: &Path, into: &Path, strip_components: u32) -> anyhow::Result<()> {
    std::fs::create_dir_all(into)?;
    let mut cmd = tokio::process::Command::new("/usr/bin/tar");
    cmd.arg("-xf").arg(archive).arg("-C").arg(into);
    if strip_components > 0 {
        cmd.arg(format!("--strip-components={strip_components}"));
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        bail!("tar failed for {}: {}", archive.display(), String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

pub fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

