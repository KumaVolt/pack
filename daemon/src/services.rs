//! Services — databases, caches, search, mail and storage, built from free, open-source
//! binaries. Each kind knows how to download itself, initialise its data
//! directory and run in the foreground under the supervisor.

use crate::config::{Config, ServiceInstance};
use crate::packages;
use crate::paths;
use crate::supervisor::Spec;
use anyhow::{Context, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub port: u16,
    /// Series a fresh install gets (the manifest lists the others).
    pub default_series: &'static str,
    pub license: &'static str,
}

pub const KINDS: &[Kind] = &[
    Kind { id: "mysql", name: "MySQL", description: "MySQL Community Server", port: 3306, default_series: "8.4", license: "GPL-2.0" },
    Kind { id: "postgres", name: "PostgreSQL", description: "PostgreSQL database", port: 5432, default_series: "18", license: "PostgreSQL" },
    Kind { id: "redis", name: "Redis (Valkey)", description: "Valkey — the BSD-licensed Redis fork, wire-compatible with Redis", port: 6379, default_series: "9", license: "BSD-3-Clause" },
    Kind { id: "meilisearch", name: "Meilisearch", description: "Search engine for Laravel Scout", port: 7700, default_series: "1", license: "MIT" },
    Kind { id: "typesense", name: "Typesense", description: "Search engine for Laravel Scout", port: 8108, default_series: "30", license: "GPL-3.0" },
    Kind { id: "mail", name: "Mail (Mailpit)", description: "Catches every email your apps send, with a web inbox", port: 2525, default_series: "1", license: "MIT" },
    Kind { id: "s3", name: "S3 storage (RustFS)", description: "S3-compatible object storage, a MinIO replacement", port: 9000, default_series: "1", license: "Apache-2.0" },
    Kind { id: "mongodb", name: "MongoDB", description: "MongoDB Community Server", port: 27017, default_series: "8.0", license: "SSPL (free to use)" },
    Kind { id: "reverb", name: "Laravel Reverb", description: "WebSocket server — runs `php artisan reverb:start` in a project", port: 8080, default_series: "-", license: "MIT" },
];

/// Services installed on a fresh setup — "in the box".
pub const DEFAULT_STACK: &[&str] = &["mysql", "postgres", "redis", "mail"];

pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// Kinds that are a project process rather than a downloaded package.
pub fn is_project_process(kind: &str) -> bool {
    kind == "reverb"
}

/// `dir/rel` with the platform's executable suffix.
fn exe_in(dir: &Path, rel: &str) -> PathBuf {
    dir.join(crate::platform::exe(rel))
}

fn bin_dir(inst: &ServiceInstance) -> PathBuf {
    packages::current(&inst.kind, &inst.version)
}

pub fn is_installed(inst: &ServiceInstance) -> bool {
    is_project_process(&inst.kind) || packages::is_installed(&inst.kind, &inst.version)
}

/// Secondary port, for services with a web UI.
pub fn ui_port(inst: &ServiceInstance) -> Option<u16> {
    match inst.kind.as_str() {
        "mail" => Some(opt_port(inst, "ui_port", 8025)),
        "s3" => Some(opt_port(inst, "console_port", inst.port + 1)),
        _ => None,
    }
}

fn opt_port(inst: &ServiceInstance, key: &str, default: u16) -> u16 {
    inst.options.get(key).and_then(|p| p.parse().ok()).unwrap_or(default)
}

pub async fn install(inst: &ServiceInstance) -> anyhow::Result<()> {
    if is_project_process(&inst.kind) {
        return Ok(());
    }
    packages::install(&inst.kind, &inst.version, None).await?;
    Ok(())
}

pub fn installed_version(inst: &ServiceInstance) -> Option<String> {
    packages::installed_version(&inst.kind, &inst.version)
}

/// Whether a series of this kind can replace another in place (same data dir).
/// Databases only move forward within compatible versions.
pub fn can_switch(kind: &str, from: &str, to: &str) -> bool {
    let newer = packages::key(to) >= packages::key(from);
    match kind {
        // In-place upgrades: MySQL 8.4 -> 9.7, Valkey 8 -> 9, MongoDB one major step.
        "mysql" | "redis" | "mongodb" => newer,
        // Postgres needs pg_upgrade/dump-restore across majors: use a second instance.
        "postgres" => from == to,
        _ => true,
    }
}

/// One-time data directory initialisation (databases).
pub async fn init(inst: &ServiceInstance) -> anyhow::Result<()> {
    let data = paths::service_data(&inst.id);
    let bin = bin_dir(inst);
    match inst.kind.as_str() {
        "mysql" if !data.join("mysql").exists() => {
            std::fs::create_dir_all(&data)?;
            run_once(
                &exe_in(&bin, "bin/mysqld"),
                &["--no-defaults", "--initialize-insecure", &format!("--basedir={}", bin.display()), &format!("--datadir={}", data.display())],
                &inst.id,
            )
            .await?;
        }
        "postgres" if !data.join("PG_VERSION").exists() => {
            let _ = std::fs::remove_dir_all(&data);
            run_once(
                &exe_in(&bin, "bin/initdb"),
                &["-D", &data.display().to_string(), "-U", "root", "--auth=trust", "-E", "UTF8", "--locale=C"],
                &inst.id,
            )
            .await?;
        }
        _ => std::fs::create_dir_all(&data)?,
    }
    Ok(())
}

async fn run_once(program: &Path, args: &[&str], id: &str) -> anyhow::Result<()> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args);
    crate::platform::no_window_tokio(&mut cmd);
    let out = cmd.output().await.with_context(|| format!("running {}", program.display()))?;
    if !out.status.success() {
        bail!(
            "{id} init failed: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(())
}

pub fn spec(cfg: &Config, inst: &ServiceInstance) -> anyhow::Result<Spec> {
    let bin = bin_dir(inst);
    let data = paths::service_data(&inst.id);
    let d = data.display().to_string();
    let port = inst.port.to_string();
    let mut env = BTreeMap::new();
    let mut cwd = Some(data.clone());
    let (program, args): (PathBuf, Vec<String>) = match inst.kind.as_str() {
        "mysql" => (
            exe_in(&bin, "bin/mysqld"),
            vec![
                "--no-defaults".into(),
                format!("--basedir={}", bin.display()),
                format!("--datadir={d}"),
                format!("--port={port}"),
                "--bind-address=127.0.0.1".into(),
                "--mysqlx=OFF".into(),
                format!("--log-error={}", paths::logs().join(format!("{}.err.log", inst.id)).display()),
            ]
            .into_iter()
            // Unix socket for local clients; Windows uses TCP only.
            .chain(cfg!(unix).then(|| format!("--socket={}", paths::run().join(format!("{}.sock", inst.id)).display())))
            .collect(),
        ),
        "postgres" => {
            let postgres = exe_in(&bin, "bin/postgres");
            let mut a = vec!["-D".to_string(), d, "-p".into(), port, "-h".into(), "127.0.0.1".into()];
            if cfg!(unix) {
                a.extend(["-k".to_string(), paths::run().display().to_string()]);
            }
            // PostgreSQL won't run as a Windows administrator: start it through
            // `pack run-restricted`, which drops admin rights like pg_ctl does.
            if crate::platform::is_admin() {
                a.splice(0..0, ["run-restricted".to_string(), "--".into(), postgres.display().to_string()]);
                (std::env::current_exe().context("finding the pack binary")?, a)
            } else {
                (postgres, a)
            }
        }
        "redis" => (
            exe_in(&bin, "valkey-server"),
            // The Windows build is MSYS2-based: it wants forward slashes.
            vec!["--port".into(), port, "--bind".into(), "127.0.0.1".into(), "--dir".into(), crate::platform::slash(&data), "--daemonize".into(), "no".into()],
        ),
        "meilisearch" => (
            exe_in(&bin, "meilisearch"),
            vec!["--db-path".into(), format!("{d}/data.ms"), "--http-addr".into(), format!("127.0.0.1:{port}"), "--no-analytics".into(), "--env".into(), "development".into(), "--experimental-dumpless-upgrade".into()],
        ),
        "typesense" => (
            exe_in(&bin, "typesense-server"),
            vec![format!("--data-dir={d}"), "--api-key=pack".into(), "--api-address=127.0.0.1".into(), format!("--api-port={port}"), "--enable-cors".into()],
        ),
        "mail" => (
            exe_in(&bin, "mailpit"),
            vec![
                "--smtp".into(),
                format!("127.0.0.1:{port}"),
                "--listen".into(),
                format!("127.0.0.1:{}", ui_port(inst).unwrap_or(8025)),
                "--database".into(),
                format!("{d}/mailpit.db"),
                "--smtp-auth-accept-any".into(),
                "--smtp-auth-allow-insecure".into(),
            ],
        ),
        "s3" => (
            exe_in(&bin, "rustfs"),
            vec![
                "server".into(),
                "--address".into(),
                format!("127.0.0.1:{port}"),
                "--access-key".into(),
                "pack".into(),
                "--secret-key".into(),
                "packpack".into(),
                "--console-enable".into(),
                "--console-address".into(),
                format!("127.0.0.1:{}", ui_port(inst).unwrap_or(9001)),
                d,
            ],
        ),
        "mongodb" => (
            exe_in(&bin, "bin/mongod"),
            vec!["--dbpath".into(), d, "--port".into(), port, "--bind_ip".into(), "127.0.0.1".into()],
        ),
        "reverb" => {
            let project = inst.options.get("path").context("Reverb needs a project path")?;
            cwd = Some(PathBuf::from(project));
            let php = crate::php::version_for_dir(cfg, Path::new(project));
            env.insert("PHP_INI_SCAN_DIR".into(), crate::php::scan_dir(&php));
            (
                crate::php::cli(&php),
                vec!["artisan".into(), "reverb:start".into(), "--host=127.0.0.1".into(), format!("--port={port}")],
            )
        }
        other => bail!("unknown service kind {other}"),
    };
    Ok(Spec {
        name: format!("service:{}", inst.id),
        program,
        args,
        env,
        cwd,
        log: paths::logs().join(format!("{}.log", inst.id)),
    })
}

/// The `.env` lines a Laravel app needs to talk to this service.
pub fn env_snippet(inst: &ServiceInstance) -> String {
    let p = inst.port;
    match inst.kind.as_str() {
        "mysql" => format!("DB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_DATABASE=laravel\nDB_USERNAME=root\nDB_PASSWORD="),
        "postgres" => format!("DB_CONNECTION=pgsql\nDB_HOST=127.0.0.1\nDB_PORT={p}\nDB_DATABASE=laravel\nDB_USERNAME=root\nDB_PASSWORD="),
        "redis" => format!("REDIS_CLIENT=phpredis\nREDIS_HOST=127.0.0.1\nREDIS_PASSWORD=null\nREDIS_PORT={p}"),
        "meilisearch" => format!("SCOUT_DRIVER=meilisearch\nMEILISEARCH_HOST=http://127.0.0.1:{p}\nMEILISEARCH_KEY="),
        "typesense" => format!("SCOUT_DRIVER=typesense\nTYPESENSE_HOST=127.0.0.1\nTYPESENSE_PORT={p}\nTYPESENSE_PROTOCOL=http\nTYPESENSE_API_KEY=pack"),
        "mail" => format!("MAIL_MAILER=smtp\nMAIL_HOST=127.0.0.1\nMAIL_PORT={p}\nMAIL_USERNAME=${{APP_NAME}}\nMAIL_PASSWORD=null"),
        "s3" => format!(
            "FILESYSTEM_DISK=s3\nAWS_ACCESS_KEY_ID=pack\nAWS_SECRET_ACCESS_KEY=packpack\nAWS_DEFAULT_REGION=us-east-1\nAWS_BUCKET=laravel\nAWS_ENDPOINT=http://127.0.0.1:{p}\nAWS_URL=http://127.0.0.1:{p}/laravel\nAWS_USE_PATH_STYLE_ENDPOINT=true"
        ),
        "mongodb" => format!("MONGODB_URI=mongodb://127.0.0.1:{p}"),
        "reverb" => format!("BROADCAST_CONNECTION=reverb\nREVERB_HOST=localhost\nREVERB_PORT={p}\nREVERB_SCHEME=http"),
        _ => String::new(),
    }
}

/// Create a database in a MySQL or Postgres instance.
pub async fn create_database(inst: &ServiceInstance, name: &str) -> anyhow::Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        bail!("invalid database name");
    }
    let bin = bin_dir(inst);
    let port = inst.port.to_string();
    match inst.kind.as_str() {
        "mysql" => {
            let sql = format!("CREATE DATABASE IF NOT EXISTS `{name}`");
            run_once(&exe_in(&bin, "bin/mysql"), &["-uroot", "-h127.0.0.1", &format!("-P{port}"), "-e", &sql], &inst.id).await
        }
        "postgres" => {
            let sql = format!("CREATE DATABASE \"{name}\"");
            run_once(&exe_in(&bin, "bin/psql"), &["-h", "127.0.0.1", "-p", &port, "-U", "root", "-d", "postgres", "-c", &sql], &inst.id).await
        }
        other => bail!("{other} has no databases"),
    }
}

/// Client binaries worth putting on PATH (mysql, psql, valkey-cli → redis-cli, ...).
pub fn client_binaries(inst: &ServiceInstance) -> Vec<(String, PathBuf)> {
    let bin = bin_dir(inst);
    let pick = |names: &[(&str, &str)]| names.iter().map(|(link, rel)| (link.to_string(), exe_in(&bin, rel))).collect();
    match inst.kind.as_str() {
        "mysql" => pick(&[("mysql", "bin/mysql"), ("mysqldump", "bin/mysqldump"), ("mysqladmin", "bin/mysqladmin")]),
        "postgres" => pick(&[("psql", "bin/psql"), ("pg_dump", "bin/pg_dump"), ("pg_restore", "bin/pg_restore"), ("createdb", "bin/createdb")]),
        "redis" => pick(&[("redis-cli", "valkey-cli"), ("valkey-cli", "valkey-cli")]),
        "mongodb" => vec![],
        _ => vec![],
    }
}
