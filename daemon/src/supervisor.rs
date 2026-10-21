//! Keeps child processes (Caddy, PHP-FPM pools, services) alive, restarts
//! them when they crash and stops them gracefully with SIGTERM.

use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct Spec {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub log: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase", tag = "state", content = "detail")]
pub enum Status {
    Starting,
    Running,
    Stopped,
    Failed(String),
}

struct Entry {
    status: Status,
    pid: Option<u32>,
    stop: Option<oneshot::Sender<()>>,
    spec: Spec,
}

#[derive(Clone, Default)]
pub struct Supervisor {
    procs: Arc<Mutex<HashMap<String, Entry>>>,
}

impl Supervisor {
    pub fn status(&self, name: &str) -> Status {
        self.procs
            .lock()
            .unwrap()
            .get(name)
            .map(|e| e.status.clone())
            .unwrap_or(Status::Stopped)
    }

    pub fn is_running(&self, name: &str) -> bool {
        matches!(self.status(name), Status::Running | Status::Starting)
    }

    pub fn running_specs(&self) -> Vec<Spec> {
        self.procs
            .lock()
            .unwrap()
            .values()
            .filter(|e| matches!(e.status, Status::Running | Status::Starting))
            .map(|e| e.spec.clone())
            .collect()
    }

    /// Start (or keep running) a process. A spec change restarts it.
    pub async fn ensure(&self, spec: Spec) {
        let same = {
            let procs = self.procs.lock().unwrap();
            procs.get(&spec.name).is_some_and(|e| {
                matches!(e.status, Status::Running | Status::Starting)
                    && e.spec.program == spec.program
                    && e.spec.args == spec.args
                    && e.spec.env == spec.env
            })
        };
        if same {
            return;
        }
        self.stop(&spec.name).await;
        self.start(spec);
    }

    fn start(&self, spec: Spec) {
        let (stop_tx, stop_rx) = oneshot::channel();
        let name = spec.name.clone();
        self.procs.lock().unwrap().insert(
            name.clone(),
            Entry { status: Status::Starting, pid: None, stop: Some(stop_tx), spec: spec.clone() },
        );
        let procs = self.procs.clone();
        tokio::spawn(run(procs, spec, stop_rx));
    }

    pub async fn stop(&self, name: &str) {
        let stop = {
            let mut procs = self.procs.lock().unwrap();
            procs.get_mut(name).and_then(|e| e.stop.take())
        };
        if let Some(tx) = stop {
            let _ = tx.send(());
            for _ in 0..150 {
                if !self.is_running(name) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        self.procs.lock().unwrap().remove(name);
    }

    /// Names of all processes starting with `prefix`.
    pub fn names_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.procs.lock().unwrap().keys().filter(|k| k.starts_with(prefix)).cloned().collect()
    }

    pub async fn stop_all(&self) {
        let names: Vec<String> = self.procs.lock().unwrap().keys().cloned().collect();
        let tasks = names.into_iter().map(|n| {
            let s = self.clone();
            tokio::spawn(async move { s.stop(&n).await })
        });
        for t in tasks {
            let _ = t.await;
        }
    }
}

fn set(procs: &Arc<Mutex<HashMap<String, Entry>>>, name: &str, status: Status, pid: Option<u32>) {
    if let Some(e) = procs.lock().unwrap().get_mut(name) {
        e.status = status;
        e.pid = pid;
    }
}

async fn run(procs: Arc<Mutex<HashMap<String, Entry>>>, spec: Spec, mut stop_rx: oneshot::Receiver<()>) {
    let mut quick_failures = 0;
    loop {
        if let Some(dir) = spec.log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&spec.log);
        let (out, err) = match log.and_then(|f| Ok((f.try_clone()?, f))) {
            Ok((a, b)) => (Stdio::from(a), Stdio::from(b)),
            Err(_) => (Stdio::null(), Stdio::null()),
        };
        let mut cmd = tokio::process::Command::new(&spec.program);
        cmd.args(&spec.args).envs(&spec.env).stdin(Stdio::null()).stdout(out).stderr(err);
        crate::platform::no_window_tokio(&mut cmd);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        let started = Instant::now();
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                set(&procs, &spec.name, Status::Failed(format!("could not start {}: {e}", spec.program.display())), None);
                return;
            }
        };
        let pid = child.id();
        set(&procs, &spec.name, Status::Running, pid);

        tokio::select! {
            _ = &mut stop_rx => {
                let graceful = pid.is_some_and(crate::platform::terminate);
                let grace = if graceful { Duration::from_secs(12) } else { Duration::ZERO };
                if tokio::time::timeout(grace, child.wait()).await.is_err() || !graceful {
                    let _ = child.kill().await;
                }
                set(&procs, &spec.name, Status::Stopped, None);
                return;
            }
            exit = child.wait() => {
                let code = exit.map(|s| s.to_string()).unwrap_or_else(|e| e.to_string());
                if started.elapsed() < Duration::from_secs(3) {
                    quick_failures += 1;
                } else {
                    quick_failures = 0;
                }
                if quick_failures >= 4 {
                    let tail = tail_file(&spec.log, 8);
                    set(&procs, &spec.name, Status::Failed(format!("{code}\n{tail}")), None);
                    return;
                }
                set(&procs, &spec.name, Status::Starting, None);
                tokio::select! {
                    _ = &mut stop_rx => { set(&procs, &spec.name, Status::Stopped, None); return; }
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    }
}

pub fn tail_file(path: &std::path::Path, lines: usize) -> String {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}
