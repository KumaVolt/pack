//! Persistent user configuration (`config.json`). The daemon is the only
//! writer; the CLI goes through the HTTP API.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub tld: String,
    pub http_port: u16,
    pub https_port: u16,
    pub dns_port: u16,
    /// Default PHP version ("8.4") for sites without isolation and the CLI shim.
    pub php: String,
    /// Package manifest (empty: the Pack repo's dist/manifest.json).
    pub manifest_url: String,
    /// Apply package updates (same series, e.g. 8.4.25 → 8.4.26) automatically.
    pub auto_update: bool,
    /// Parked directories: every sub-folder becomes `<folder>.<tld>`.
    pub paths: Vec<PathBuf>,
    /// Explicitly linked sites: name -> project directory.
    pub links: BTreeMap<String, PathBuf>,
    /// Serve every site over HTTPS (http redirects to https), except `insecure`.
    pub https: bool,
    /// Sites kept on plain HTTP while `https` is on.
    pub insecure: Vec<String>,
    /// Sites served over HTTPS while `https` is off.
    pub secured: Vec<String>,
    /// Per-site PHP version.
    pub isolated: BTreeMap<String, String>,
    /// Proxy sites: name -> upstream URL (e.g. http://127.0.0.1:3000).
    pub proxies: BTreeMap<String, String>,
    pub services: Vec<ServiceInstance>,
    /// Send `dump()` output to Pack instead of the page.
    pub dumps: bool,
    /// Serve sites to other devices on the network (off: localhost only).
    pub lan_access: bool,
    pub php_ini: PhpIni,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PhpIni {
    pub memory_limit: String,
    pub upload_max_filesize: String,
    pub post_max_size: String,
    pub max_execution_time: u32,
}

impl Default for PhpIni {
    fn default() -> Self {
        Self {
            memory_limit: "512M".into(),
            upload_max_filesize: "2G".into(),
            post_max_size: "2G".into(),
            max_execution_time: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInstance {
    /// Unique id, e.g. "mysql", "postgres-17" — also the data directory name.
    pub id: String,
    pub kind: String,
    pub version: String,
    pub port: u16,
    #[serde(default = "yes")]
    pub autostart: bool,
    /// Kind-specific settings (e.g. Reverb's project path).
    #[serde(default)]
    pub options: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        let sites = paths::user_home().join("Pack");
        Self {
            tld: "test".into(),
            http_port: 80,
            https_port: 443,
            dns_port: 53535,
            php: "8.4".into(),
            manifest_url: String::new(),
            auto_update: false,
            paths: vec![sites],
            links: BTreeMap::new(),
            https: true,
            insecure: vec![],
            secured: vec![],
            isolated: BTreeMap::new(),
            proxies: BTreeMap::new(),
            services: vec![],
            dumps: true,
            lan_access: false,
            php_ini: PhpIni::default(),
        }
    }
}

impl Config {
    pub fn load() -> Config {
        std::fs::read(paths::config_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let file = paths::config_file();
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, file)?;
        Ok(())
    }

    pub fn is_secured(&self, site: &str) -> bool {
        if self.https {
            !self.insecure.iter().any(|s| s == site)
        } else {
            self.secured.iter().any(|s| s == site)
        }
    }

    pub fn set_secured(&mut self, site: &str, on: bool) {
        self.insecure.retain(|s| s != site);
        self.secured.retain(|s| s != site);
        match (self.https, on) {
            (true, false) => self.insecure.push(site.to_string()),
            (false, true) => self.secured.push(site.to_string()),
            _ => {}
        }
    }

    /// PHP versions referenced anywhere in the config (default + isolations).
    pub fn php_versions_in_use(&self) -> Vec<String> {
        let mut versions = vec![self.php.clone()];
        for v in self.isolated.values() {
            if !versions.contains(v) {
                versions.push(v.clone());
            }
        }
        versions
    }
}
