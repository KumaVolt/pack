//! Daemon state and the reconcile loop that turns the config into running
//! processes: Caddy, one PHP-FPM pool per PHP version in use, services.

use crate::config::{Config, ServiceInstance};
use crate::supervisor::{Status, Supervisor};
use crate::{caddy, packages, paths, php, services, system};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Dump {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub time: u64,
    pub site: Option<String>,
    pub request: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub html: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub label: String,
    pub state: &'static str,
    pub error: Option<String>,
    /// Unix ms when the job was started.
    pub started: u64,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub struct App {
    pub cfg: Mutex<Config>,
    pub sup: Supervisor,
    pub dumps: Mutex<VecDeque<Dump>>,
    pub jobs: Mutex<BTreeMap<String, Job>>,
    /// Serialises reconciles so concurrent API calls don't race.
    reconcile_lock: tokio::sync::Mutex<()>,
}

pub type Shared = Arc<App>;

impl App {
    pub fn new() -> Shared {
        Arc::new(App {
            cfg: Mutex::new(Config::load()),
            sup: Supervisor::default(),
            dumps: Mutex::new(VecDeque::new()),
            jobs: Mutex::new(BTreeMap::new()),
            reconcile_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn config(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    /// Mutate and persist the config.
    pub fn update(&self, f: impl FnOnce(&mut Config)) -> anyhow::Result<Config> {
        let mut cfg = self.cfg.lock().unwrap();
        f(&mut cfg);
        cfg.save()?;
        Ok(cfg.clone())
    }

    pub fn add_dump(&self, mut dump: Dump) {
        let mut dumps = self.dumps.lock().unwrap();
        dump.id = dumps.back().map(|d| d.id + 1).unwrap_or(1);
        dump.time = now_ms();
        dumps.push_back(dump);
        while dumps.len() > 500 {
            dumps.pop_front();
        }
    }

    pub fn status_of(&self, name: &str) -> Status {
        self.sup.status(name)
    }

    /// Run a slow task in the background, visible in the UI's job list.
    pub fn spawn_job<F>(self: &Shared, id: String, label: String, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        {
            let mut jobs = self.jobs.lock().unwrap();
            if jobs.get(&id).is_some_and(|j| j.state == "running") {
                return;
            }
            jobs.insert(id.clone(), Job { id: id.clone(), label, state: "running", error: None, started: now_ms() });
        }
        let app = self.clone();
        tokio::spawn(async move {
            let result = fut.await;
            if result.is_ok() {
                let _ = app.reconcile().await;
            }
            let mut jobs = app.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(&id) {
                match result {
                    Ok(()) => job.state = "done",
                    Err(e) => {
                        job.state = "failed";
                        job.error = Some(format!("{e:#}"));
                    }
                }
            }
        });
    }

    /// Bring running processes in line with the config.
    pub async fn reconcile(self: &Shared) -> anyhow::Result<()> {
        let _guard = self.reconcile_lock.lock().await;
        let cfg = self.config();
        std::fs::write(paths::tools().join("dumps.php"), include_str!("dumps.php"))?;

        // PHP-FPM pools for every installed version that a site (or the default) uses.
        let wanted = cfg.php_versions_in_use();
        for p in php::installed() {
            if wanted.contains(&p.version) {
                php::write_config(&p.version, &cfg)?;
                for spec in php::pool_specs(&p.version, &cfg) {
                    self.sup.ensure(spec).await;
                }
                // Don't hand Caddy a pool that isn't listening yet (avoids a 502 right after isolate).
                for _ in 0..30 {
                    if php::pool_ready(&p.version) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            } else {
                self.stop_php(&p.version).await;
            }
        }

        if caddy::binary().exists() {
            self.sup.ensure(caddy::spec()).await;
            // Give a freshly started Caddy a moment before pushing the config.
            for _ in 0..30 {
                if tokio::net::TcpStream::connect(caddy::ADMIN).await.is_ok() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            caddy::apply(&cfg).await?;
        }

        for inst in &cfg.services {
            let name = format!("service:{}", inst.id);
            if inst.autostart && services::is_installed(inst) {
                if !self.sup.is_running(&name) {
                    if let Err(e) = services::init(inst).await {
                        tracing_log(&format!("init {}: {e:#}", inst.id));
                        continue;
                    }
                }
                match services::spec(&cfg, inst) {
                    Ok(spec) => self.sup.ensure(spec).await,
                    Err(e) => tracing_log(&format!("{}: {e:#}", inst.id)),
                }
            } else if self.sup.is_running(&name) {
                self.sup.stop(&name).await;
            }
        }
        // Stop processes for services that were removed.
        for spec in self.sup.running_specs() {
            if let Some(id) = spec.name.strip_prefix("service:") {
                if !cfg.services.iter().any(|s| s.id == id) {
                    self.sup.stop(&spec.name).await;
                }
            }
        }
        let _ = system::write_shims(&cfg);
        Ok(())
    }

    pub fn find_service(&self, id: &str) -> Option<ServiceInstance> {
        self.config().services.into_iter().find(|s| s.id == id)
    }

    /// Add a service instance (not yet installed). Picks the next free port
    /// when the default one is taken.
    pub fn add_service(&self, kind_id: &str, series: Option<String>, port: Option<u16>, path: Option<String>) -> anyhow::Result<ServiceInstance> {
        let kind = services::kind(kind_id).ok_or_else(|| anyhow::anyhow!("unknown service {kind_id}"))?;
        let cfg = self.config();
        let port = match port {
            Some(p) => {
                if let Some(other) = cfg.services.iter().find(|s| s.port == p) {
                    anyhow::bail!("port {p} is already used by {}", other.id);
                }
                p
            }
            None => free_port(kind.port, &cfg),
        };
        let mut id = kind.id.to_string();
        let mut n = 2;
        while cfg.services.iter().any(|s| s.id == id) {
            id = format!("{}-{n}", kind.id);
            n += 1;
        }
        let mut options = std::collections::BTreeMap::new();
        if services::is_project_process(kind.id) {
            let path = path.ok_or_else(|| anyhow::anyhow!("{} needs a project path", kind.name))?;
            options.insert("path".into(), path);
        }
        let inst = ServiceInstance {
            id,
            kind: kind.id.into(),
            version: series.unwrap_or_else(|| packages::series_or_newest(kind.id, kind.default_series)),
            port,
            autostart: true,
            options,
        };
        self.update(|c| c.services.push(inst.clone()))?;
        Ok(inst)
    }

    /// First run: web server, Composer, the default PHP and — unless
    /// `minimal` — the default service stack, all from the package manifest.
    pub async fn bootstrap(self: &Shared, minimal: bool) -> anyhow::Result<Vec<String>> {
        let mut notes = vec![];
        let cfg = self.config();
        caddy::install().await?;
        system::ensure_composer().await?;
        if !php::is_installed(&cfg.php) {
            php::install(&cfg.php).await?;
        }
        for dir in &cfg.paths {
            let _ = std::fs::create_dir_all(dir);
        }
        if !minimal {
            for kind in services::DEFAULT_STACK {
                if self.config().services.iter().any(|s| s.kind == *kind) {
                    continue;
                }
                let inst = self.add_service(kind, None, None, None)?;
                let default = services::kind(kind).map(|k| k.port).unwrap_or(0);
                if inst.port != default {
                    notes.push(format!("{} uses port {} ({} is taken)", inst.id, inst.port, default));
                }
                services::install(&inst).await?;
            }
        }
        self.reconcile().await?;
        Ok(notes)
    }

    /// Bring installed packages up to the newest build of their series.
    /// `only` limits it to one package/series.
    pub async fn update_packages(self: &Shared, only: Option<(String, String)>) -> anyhow::Result<Vec<String>> {
        let m = packages::manifest(true).await?;
        let mut done = vec![];
        for p in packages::installed(&m) {
            if !p.update || only.as_ref().is_some_and(|(n, s)| *n != p.name || *s != p.series) {
                continue;
            }
            let version = packages::install(&p.name, &p.series, None).await?;
            self.restart_users_of(&p.name, &p.series).await;
            packages::prune(&p.name, &p.series);
            done.push(format!("{} {} → {version}", p.name, p.version));
        }
        Ok(done)
    }

    /// Stop every FastCGI process of a PHP version.
    pub async fn stop_php(&self, version: &str) {
        for name in self.sup.names_with_prefix(&format!("php-{version}")) {
            self.sup.stop(&name).await;
        }
    }

    /// Restart the processes running a package's binaries.
    pub async fn restart_users_of(self: &Shared, name: &str, series: &str) {
        let cfg = self.config();
        match name {
            "php" => self.stop_php(series).await,
            "caddy" => self.sup.stop("caddy").await,
            _ => {
                for inst in cfg.services.iter().filter(|s| s.kind == name && s.version == series) {
                    self.sup.stop(&format!("service:{}", inst.id)).await;
                }
            }
        }
        let _ = self.reconcile().await;
    }
}

/// The first port from `start` that nothing listens on and no service claims.
pub fn free_port(start: u16, cfg: &crate::config::Config) -> u16 {
    (start..start.saturating_add(100))
        .find(|p| {
            !cfg.services.iter().any(|s| s.port == *p || services::ui_port(s) == Some(*p))
                && std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok()
                && std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], *p).into(), std::time::Duration::from_millis(50)).is_err()
        })
        .unwrap_or(start)
}

pub fn tracing_log(msg: &str) {
    eprintln!("[pack] {msg}");
}
