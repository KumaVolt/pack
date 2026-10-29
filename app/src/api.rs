//! Client for the daemon's JSON API on 127.0.0.1:7171 (and Mailpit's, for the
//! Mail page). Calls block, so the UI runs them on the background executor.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

pub const PORT: u16 = 7171;

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_millis(500)))
        // Set up asks for the admin password and downloads everything.
        .timeout_global(Some(Duration::from_secs(900)))
        .build()
        .into()
});

fn url(path: &str) -> String {
    if path.starts_with("http") { path.to_string() } else { format!("http://127.0.0.1:{PORT}{path}") }
}

fn read<T: DeserializeOwned>(res: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> Result<T, String> {
    let mut res = res.map_err(|e| e.to_string())?;
    let ok = res.status().is_success();
    let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !ok {
        let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["error"].as_str().map(str::to_string));
        return Err(msg.unwrap_or_else(|| format!("{} {}", res.status(), text.trim())));
    }
    serde_json::from_str(if text.is_empty() { "null" } else { &text }).map_err(|e| e.to_string())
}

pub fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    read(AGENT.get(url(path)).call())
}

pub fn post(path: &str, body: Value) -> Result<Value, String> {
    read(AGENT.post(url(path)).send_json(body))
}

pub fn delete(path: &str) -> Result<Value, String> {
    read(AGENT.delete(url(path)).call())
}

/// Percent-encode a query value.
pub fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A supervised process: `{"state": "running"}` or `{"state": "failed", "detail": "..."}`.
#[derive(Deserialize, Clone, Default, PartialEq)]
#[serde(default)]
pub struct Proc {
    pub state: String,
    pub detail: Option<String>,
}

pub fn state(p: &Option<Proc>) -> &str {
    p.as_ref().map(|p| p.state.as_str()).filter(|s| !s.is_empty()).unwrap_or("stopped")
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Status {
    pub version: String,
    pub home: String,
    pub tld: String,
    pub caddy: Caddy,
    pub os: String,
    pub dns: Dns,
    pub ca_trusted: bool,
    pub php: Vec<PhpStatus>,
    pub default_php: String,
    pub downloads: Vec<Download>,
    pub jobs: Vec<Job>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Caddy {
    pub installed: bool,
    pub status: Option<Proc>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Dns {
    pub port: u16,
    pub resolver: bool,
    pub low_ports: bool,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct PhpStatus {
    pub version: String,
    pub installed: bool,
    pub status: Option<Proc>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Download {
    pub label: String,
    pub done: u64,
    pub total: Option<u64>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Job {
    pub id: String,
    pub label: String,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Sites {
    pub sites: Vec<Site>,
    pub paths: Vec<String>,
    pub paths_display: Vec<String>,
    pub tld: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Site {
    pub name: String,
    pub host: String,
    pub url: String,
    pub kind: String,
    pub php: String,
    pub isolated: bool,
    pub secured: bool,
    pub proxy: Option<String>,
    pub framework: String,
    pub path_display: String,
    pub shared: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Php {
    pub installed: Vec<PhpInstalled>,
    /// Version line ("8.4") → newest patch release in the manifest.
    pub available: BTreeMap<String, String>,
    pub default: String,
    pub extensions: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct PhpInstalled {
    pub version: String,
    pub patch: String,
    pub status: Option<Proc>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Services {
    pub kinds: Vec<Kind>,
    pub instances: Vec<Instance>,
    /// Kind → [(version line, newest release)].
    pub series: BTreeMap<String, Vec<(String, String)>>,
}

impl Services {
    pub fn lines(&self, kind: &str) -> Vec<String> {
        self.series.get(kind).map(|l| l.iter().map(|(s, _)| s.clone()).collect()).unwrap_or_default()
    }

    pub fn latest(&self, kind: &str, series: &str) -> Option<String> {
        self.series.get(kind)?.iter().find(|(s, _)| s == series).map(|(_, v)| v.clone())
    }
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Kind {
    pub id: String,
    pub name: String,
    pub description: String,
    pub port: u16,
    pub license: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Instance {
    pub id: String,
    pub kind: String,
    pub name: Option<String>,
    pub series: String,
    pub installed_version: Option<String>,
    pub port: u16,
    pub ui_port: Option<u16>,
    pub status: Option<Proc>,
    pub env: String,
}

impl Instance {
    pub fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.id.clone())
    }

    /// Where its web dashboard lives, if it has one.
    pub fn dashboard(&self) -> Option<String> {
        let port = self.ui_port?;
        let path = if self.kind == "s3" { "/rustfs/console/" } else { "" };
        Some(format!("http://127.0.0.1:{port}{path}"))
    }
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Packages {
    pub installed: Vec<Package>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub latest: Option<String>,
    pub update: bool,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Dumps {
    pub dumps: Vec<Dump>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Dump {
    pub id: u64,
    /// Unix ms.
    pub time: u64,
    pub site: Option<String>,
    pub request: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// VarDumper's CliDumper output.
    pub text: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct SiteLogs {
    pub files: Vec<String>,
    pub file: Option<String>,
    pub entries: Vec<LogEntry>,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct LogEntry {
    pub time: String,
    pub level: String,
    pub message: String,
    pub context: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct SystemLogs {
    pub files: Vec<String>,
    pub text: String,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Settings {
    pub https: bool,
    pub dumps: bool,
    pub lan_access: bool,
    pub auto_update: bool,
    pub php_ini: PhpIni,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct PhpIni {
    pub memory_limit: String,
    pub upload_max_filesize: String,
    pub post_max_size: String,
    pub max_execution_time: u32,
}

/// Mailpit's message list (`/api/v1/messages`).
#[derive(Deserialize, Clone, Default)]
#[serde(default)]
pub struct Inbox {
    pub messages: Vec<MailSummary>,
    pub total: u64,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct MailSummary {
    #[serde(rename = "ID")]
    pub id: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub subject: String,
    /// RFC 3339.
    pub created: String,
    pub snippet: String,
    pub read: bool,
}

#[derive(Deserialize, Clone, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct Address {
    pub name: String,
    pub address: String,
}

impl Address {
    pub fn display(&self) -> String {
        if self.name.is_empty() { self.address.clone() } else { format!("{} <{}>", self.name, self.address) }
    }
}

/// One message (`/api/v1/message/{id}`).
#[derive(Deserialize, Clone, Default)]
#[serde(default, rename_all = "PascalCase")]
pub struct Mail {
    #[serde(rename = "ID")]
    pub id: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub subject: String,
    pub date: String,
    pub text: String,
    #[serde(rename = "HTML")]
    pub html: String,
}

/// Compare version strings numerically ("8.4.10" > "8.4.9").
pub fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let nums = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect()
    };
    let (x, y) = (nums(a), nums(b));
    for i in 0..x.len().max(y.len()) {
        let o = x.get(i).unwrap_or(&0).cmp(y.get(i).unwrap_or(&0));
        if o.is_ne() {
            return o;
        }
    }
    std::cmp::Ordering::Equal
}
