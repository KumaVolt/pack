//! Site discovery: parked directories, links and proxies, resolved into the
//! list of hosts Caddy serves.

use crate::config::Config;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Site {
    pub name: String,
    pub host: String,
    pub url: String,
    /// Project directory (empty for proxies).
    pub path: PathBuf,
    /// Directory Caddy serves from (public/, web/, or the project root).
    pub docroot: PathBuf,
    pub kind: SiteKind,
    pub php: String,
    pub isolated: bool,
    pub secured: bool,
    pub proxy: Option<String>,
    /// Detected framework, for display ("Laravel", "WordPress", "Static", ...).
    pub framework: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SiteKind {
    Parked,
    Linked,
    Proxy,
}

/// Turn a directory name into a host label: "My App" -> "my-app".
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

pub fn all(cfg: &Config) -> Vec<Site> {
    let mut sites: Vec<Site> = vec![];
    let push = |site: Site, sites: &mut Vec<Site>| {
        sites.retain(|s| s.name != site.name);
        sites.push(site);
    };
    for parked in &cfg.paths {
        let Ok(entries) = std::fs::read_dir(parked) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !entry.path().is_dir() {
                continue;
            }
            let slug = slug(&name);
            if slug.is_empty() {
                continue;
            }
            push(build(cfg, &slug, entry.path(), SiteKind::Parked), &mut sites);
        }
    }
    for (name, path) in &cfg.links {
        push(build(cfg, name, path.clone(), SiteKind::Linked), &mut sites);
    }
    for (name, upstream) in &cfg.proxies {
        let mut site = build(cfg, name, PathBuf::new(), SiteKind::Proxy);
        site.proxy = Some(upstream.clone());
        site.framework = "Proxy".into();
        push(site, &mut sites);
    }
    sites.sort_by(|a, b| a.name.cmp(&b.name));
    sites
}

pub fn find(cfg: &Config, name: &str) -> Option<Site> {
    let name = name.trim_end_matches(&format!(".{}", cfg.tld));
    all(cfg).into_iter().find(|s| s.name == name)
}

/// The site whose project directory contains `dir`.
pub fn for_dir(cfg: &Config, dir: &Path) -> Option<Site> {
    all(cfg)
        .into_iter()
        .filter(|s| !s.path.as_os_str().is_empty() && dir.starts_with(&s.path))
        .max_by_key(|s| s.path.components().count())
}

fn build(cfg: &Config, name: &str, path: PathBuf, kind: SiteKind) -> Site {
    let host = format!("{name}.{}", cfg.tld);
    let secured = cfg.is_secured(name);
    let isolated = cfg.isolated.get(name);
    let scheme = if secured { "https" } else { "http" };
    let port = if secured { cfg.https_port } else { cfg.http_port };
    let default_port = if secured { 443 } else { 80 };
    let url = if port == default_port { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") };
    let (docroot, framework) = detect(&path);
    Site {
        name: name.to_string(),
        host,
        url,
        docroot,
        path,
        kind,
        php: isolated.cloned().unwrap_or_else(|| cfg.php.clone()),
        isolated: isolated.is_some(),
        secured,
        proxy: None,
        framework,
    }
}

/// Pick the web root and guess the framework, like Valet's drivers do.
fn detect(path: &Path) -> (PathBuf, String) {
    if path.as_os_str().is_empty() {
        return (PathBuf::new(), String::new());
    }
    let has = |p: &str| path.join(p).exists();
    if has("artisan") && has("public/index.php") {
        let name = if has("vendor/statamic") { "Statamic" } else { "Laravel" };
        return (path.join("public"), name.into());
    }
    if has("craft") && has("web/index.php") {
        return (path.join("web"), "Craft CMS".into());
    }
    if has("wp-config.php") || has("wp-config-sample.php") {
        return (path.to_path_buf(), "WordPress".into());
    }
    if has("web/app/themes") && has("web/wp") {
        return (path.join("web"), "Bedrock".into());
    }
    if has("bin/console") && has("public/index.php") {
        return (path.join("public"), "Symfony".into());
    }
    for root in ["public", "web", "dist", "build", "_site"] {
        if path.join(root).is_dir() {
            let php = path.join(root).join("index.php").exists();
            return (path.join(root), if php { "PHP" } else { "Static" }.into());
        }
    }
    let php = has("index.php");
    (path.to_path_buf(), if php { "PHP" } else { "Static" }.into())
}

/// Point a Laravel app's APP_URL at the site's URL, if it still has the
/// default (`http://localhost`) or an old URL of this same site.
pub fn sync_app_url(site: &Site) {
    if site.framework != "Laravel" && site.framework != "Statamic" {
        return;
    }
    let env = site.path.join(".env");
    let Ok(text) = std::fs::read_to_string(&env) else { return };
    let mut changed = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let Some(value) = line.strip_prefix("APP_URL=") else { return line.to_string() };
            let current = value.trim().trim_matches('"');
            let host = current.split("://").nth(1).unwrap_or("").split(['/', ':']).next().unwrap_or("");
            if (current == "http://localhost" || host == site.host) && current != site.url {
                changed = true;
                format!("APP_URL={}", site.url)
            } else {
                line.to_string()
            }
        })
        .collect();
    if changed {
        let mut out = lines.join("\n");
        if text.ends_with('\n') {
            out.push('\n');
        }
        let _ = std::fs::write(env, out);
    }
}

impl Site {
    pub fn serves_php(&self) -> bool {
        self.proxy.is_none() && self.framework != "Static"
    }
}
