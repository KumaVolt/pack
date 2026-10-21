//! Pack — a free, open-source local PHP development environment for macOS.
//! `pack daemon` runs the background service (web server, DNS, PHP, services,
//! dashboard API); every other subcommand is a thin client for it.

mod api;
mod app;
mod caddy;
mod config;
mod dns;
mod download;
mod logs;
mod paths;
mod packages;
mod php;
mod platform;
mod services;
mod sites;
mod supervisor;
mod system;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pack", version, about = "Free local PHP development environment — sites, PHP versions and services")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the Pack daemon in the foreground
    Daemon,
    /// Download the web server, PHP, Composer and services, set up .test domains + HTTPS and start at login
    Install {
        /// Don't start Pack at login (run `pack daemon` yourself)
        #[arg(long)]
        no_agent: bool,
        /// Only web server + PHP + Composer, no MySQL/Postgres/Redis/Mail
        #[arg(long)]
        minimal: bool,
        /// Leave DNS, certificates and PATH alone (no admin prompt; for CI)
        #[arg(long)]
        no_system: bool,
    },
    /// Update PHP, services and tools to the newest build of their version line
    Update {
        /// Only show what would be updated
        #[arg(long)]
        check: bool,
        /// Limit to one package (php, mysql, postgres, redis, caddy, ...)
        name: Option<String>,
        /// Limit to one version line of it (e.g. 8.4)
        series: Option<String>,
    },
    /// Update the pack CLI/daemon itself from the latest Pack release
    SelfUpdate,
    /// Point the PATH shims (and autostart) at this pack binary
    #[command(hide = true)]
    Refresh,
    /// Everything that can be installed, per version line
    Available,
    /// Stop Pack and remove autostart and the .test DNS setup
    Uninstall,
    /// Show what is running
    Status,
    /// Serve every folder inside a directory as <folder>.test
    Park { path: Option<PathBuf> },
    /// Stop serving a parked directory
    Unpark { path: Option<PathBuf> },
    /// List parked directories
    Paths,
    /// Serve the current directory (or PATH) as <name>.test
    Link {
        name: Option<String>,
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Remove a link or proxy
    Unlink { name: Option<String> },
    /// List all sites
    #[command(alias = "links")]
    Sites,
    /// Serve a site over HTTPS
    Secure { name: Option<String> },
    /// Serve a site over plain HTTP again
    Unsecure { name: Option<String> },
    /// Pin a site to a PHP version
    Isolate {
        version: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Use the default PHP version again
    Unisolate {
        #[arg(long)]
        site: Option<String>,
    },
    /// Proxy <name>.test to a local URL (e.g. 127.0.0.1:3000)
    Proxy { name: String, upstream: String },
    /// Remove a proxy
    Unproxy { name: String },
    /// Open a site in the browser
    Open { name: Option<String> },
    /// Share a site publicly with a free Cloudflare quick tunnel
    Share { name: Option<String> },
    /// Stop sharing a site
    Unshare { name: Option<String> },
    /// Set the default PHP version
    Use { version: String },
    /// Manage PHP versions
    Php {
        #[command(subcommand)]
        cmd: PhpCmd,
    },
    /// Run the right PHP for the current directory (what the `php` shim calls)
    #[command(hide = true)]
    ExecPhp {
        /// Force a version (php84 shim) instead of resolving it from the directory
        #[arg(long)]
        version: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Print the PHP binary for the current directory (used by the php shim)
    WhichPhp {
        /// Print the version directory instead of the binary
        #[arg(long)]
        dir: bool,
    },
    /// Manage services (MySQL, Postgres, Redis, Meilisearch, Mail, S3, ...)
    #[command(alias = "services")]
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// Create a database: `pack db laravel`
    Db {
        name: String,
        /// Service id (defaults to the first MySQL/Postgres service)
        #[arg(long)]
        service: Option<String>,
    },
    /// Stream dump() output to the terminal
    Dumps,
    /// Show the latest Laravel log entries of a site
    Logs { name: Option<String> },
    /// Restart all processes
    Restart,
}

#[derive(Subcommand)]
enum PhpCmd {
    /// Installed and available versions
    List,
    /// Install a prebuilt version (e.g. 8.3)
    Install { version: String },
    /// Use PHP binaries you built yourself (a dir containing php and php-fpm)
    Import { version: String, dir: PathBuf },
    /// Remove a version
    Uninstall { version: String },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// List services and what can be added
    List,
    /// Add (download + start) a service: mysql, postgres, redis, meilisearch, typesense, mail, s3, mongodb, reverb
    Add {
        kind: String,
        #[arg(long)]
        version: Option<String>,
        #[arg(long)]
        port: Option<u16>,
        /// Project path (Reverb)
        #[arg(long)]
        path: Option<PathBuf>,
    },
    Start { id: String },
    Stop { id: String },
    /// Remove a service (keeps its data unless --data)
    Remove {
        id: String,
        #[arg(long)]
        data: bool,
    },
    /// Print the .env lines for a service
    Env { id: String },
    /// Move a service to another version line, keeping its data (e.g. mysql 9.7)
    Version { id: String, series: String },
}

fn main() {
    let cli = Cli::parse();
    // The php shim calls this on every invocation: keep it synchronous and fast.
    if let Cmd::WhichPhp { dir } = &cli.cmd {
        let cfg = config::Config::load();
        let cwd = std::env::current_dir().unwrap_or_default();
        let version = php::version_for_dir(&cfg, &cwd);
        let path = if *dir { paths::php_dir(&version) } else { php::cli(&version) };
        if !php::is_installed(&version) {
            std::process::exit(1);
        }
        println!("{}", path.display());
        return;
    }
    if let Cmd::ExecPhp { version, args } = &cli.cmd {
        let cfg = config::Config::load();
        let version = version.clone().unwrap_or_else(|| php::version_for_dir(&cfg, &std::env::current_dir().unwrap_or_default()));
        if !php::is_installed(&version) {
            eprintln!("PHP {version} is not installed — run `pack php install {version}`");
            std::process::exit(1);
        }
        let dir = paths::php_dir(&version);
        let mut cmd = std::process::Command::new(php::cli(&version));
        cmd.arg("-c").arg(dir.join("php.ini")).args(args).env("PHP_INI_SCAN_DIR", php::scan_dir(&version));
        platform::exec(cmd);
    }
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    if let Err(e) = rt.block_on(run(cli.cmd)) {
        eprintln!("\x1b[31m{e:#}\x1b[0m");
        std::process::exit(1);
    }
}

async fn daemon() -> anyhow::Result<()> {
    paths::ensure_layout()?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", api::PORT))
        .await
        .map_err(|e| anyhow::anyhow!("cannot bind 127.0.0.1:{} ({e}) — is Pack already running?", api::PORT))?;
    let app = app::App::new();
    let cfg = app.config();
    let tld = cfg.tld.clone();
    let port = platform::dns_port(&cfg);
    tokio::spawn(async move {
        if let Err(e) = dns::serve(port, tld).await {
            app::tracing_log(&format!("DNS server failed: {e:#}"));
        }
    });
    if let Err(e) = app.reconcile().await {
        app::tracing_log(&format!("startup: {e:#}"));
    }
    // Check the package manifest every 6 hours; apply updates if enabled.
    let updater = app.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
            if packages::manifest(true).await.is_ok() && updater.config().auto_update {
                match updater.update_packages(None).await {
                    Ok(done) => done.iter().for_each(|d| app::tracing_log(&format!("updated {d}"))),
                    Err(e) => app::tracing_log(&format!("auto-update: {e:#}")),
                }
            }
        }
    });
    // Pick up folders added to (or removed from) parked directories.
    let watcher = app.clone();
    tokio::spawn(async move {
        let fingerprint = |a: &app::Shared| {
            sites::all(&a.config()).iter().map(|s| format!("{}={}|{}", s.name, s.docroot.display(), s.framework)).collect::<Vec<_>>()
        };
        let mut last = fingerprint(&watcher);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let now = fingerprint(&watcher);
            if now != last {
                // New projects (e.g. `laravel new`) get APP_URL pointed at their .test URL.
                for site in sites::all(&watcher.config()) {
                    let key = format!("{}={}|{}", site.name, site.docroot.display(), site.framework);
                    if !last.contains(&key) {
                        sites::sync_app_url(&site);
                    }
                }
                last = now;
                if let Err(e) = watcher.reconcile().await {
                    app::tracing_log(&format!("reconcile: {e:#}"));
                }
            }
        }
    });
    println!("Pack daemon on http://127.0.0.1:{} (DNS on 127.0.0.1:{port})", api::PORT);
    let shutdown_app = app.clone();
    axum::serve(listener, api::router(app))
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            {
                let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            #[cfg(windows)]
            let _ = tokio::signal::ctrl_c().await;
            shutdown_app.sup.stop_all().await;
        })
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- client

fn base() -> String {
    format!("http://127.0.0.1:{}", api::PORT)
}

async fn daemon_up() -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", api::PORT)).await.is_ok()
}

/// Start the daemon in the background if it isn't running.
async fn ensure_daemon() -> anyhow::Result<()> {
    if daemon_up().await {
        return Ok(());
    }
    if platform::autostart_installed() && !cfg!(windows) {
        platform::kick_autostart(false);
    } else {
        paths::ensure_layout()?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(paths::logs().join("daemon.log"))?;
        let mut cmd = std::process::Command::new(std::env::current_exe()?);
        // Detached, so closing the terminal doesn't take the daemon down.
        platform::detach(&mut cmd);
        cmd.arg("daemon").stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log).spawn()?;
    }
    for _ in 0..50 {
        if daemon_up().await {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    anyhow::bail!("Pack daemon did not start — see {}", paths::logs().join("daemon.log").display())
}

async fn call(method: &str, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
    ensure_daemon().await?;
    let client = reqwest::Client::new();
    let url = format!("{}{path}", base());
    let req = match method {
        "POST" => client.post(&url).json(&body.unwrap_or(json!({}))),
        "DELETE" => client.delete(&url),
        _ => client.get(&url),
    };
    let res = req.send().await?;
    let ok = res.status().is_success();
    let value: Value = res.json().await.unwrap_or(Value::Null);
    if !ok {
        anyhow::bail!("{}", value["error"].as_str().unwrap_or("request failed"));
    }
    Ok(value)
}

async fn get(path: &str) -> anyhow::Result<Value> {
    call("GET", path, None).await
}
async fn post(path: &str, body: Value) -> anyhow::Result<Value> {
    call("POST", path, Some(body)).await
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// The site named, or the one for the current directory.
fn site_name(name: Option<String>) -> anyhow::Result<String> {
    if let Some(n) = name {
        return Ok(n);
    }
    let cfg = config::Config::load();
    sites::for_dir(&cfg, &cwd())
        .map(|s| s.name)
        .ok_or_else(|| anyhow::anyhow!("this directory is not a Pack site — pass a site name or run `pack link`"))
}

fn state_label(status: &Value) -> String {
    match status["state"].as_str().unwrap_or("stopped") {
        "running" => "\x1b[32m● running\x1b[0m".into(),
        "starting" => "\x1b[33m● starting\x1b[0m".into(),
        "failed" => "\x1b[31m● failed\x1b[0m".into(),
        other => format!("\x1b[90m○ {other}\x1b[0m"),
    }
}

/// Wait for background jobs started by this command; false if any failed.
async fn wait_for_jobs() -> anyhow::Result<bool> {
    // Only report jobs started by this command (give the clock a little slack).
    let since = app::now_ms().saturating_sub(5_000);
    let mut last = String::new();
    loop {
        let status = get("/api/status").await?;
        for d in status["downloads"].as_array().into_iter().flatten() {
            let done = d["done"].as_u64().unwrap_or(0) as f64 / 1e6;
            let line = match d["total"].as_u64() {
                Some(t) => format!("  {} {:.0}/{:.0} MB", d["label"].as_str().unwrap_or(""), done, t as f64 / 1e6),
                None => format!("  {} {:.0} MB", d["label"].as_str().unwrap_or(""), done),
            };
            if line != last {
                eprint!("\r\x1b[2K{line}");
                last = line;
            }
        }
        let jobs: Vec<Value> = status["jobs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|j| j["started"].as_u64().unwrap_or(0) >= since)
            .cloned()
            .collect();
        if jobs.iter().all(|j| j["state"] != "running") {
            eprint!("\r\x1b[2K");
            let mut ok = true;
            for j in jobs.iter().filter(|j| j["state"] == "failed") {
                eprintln!("\x1b[31m{} failed: {}\x1b[0m", j["label"].as_str().unwrap_or(""), j["error"].as_str().unwrap_or(""));
                ok = false;
            }
            return Ok(ok);
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
}

async fn run(cmd: Cmd) -> anyhow::Result<()> {
    match cmd {
        Cmd::Daemon => daemon().await?,
        Cmd::WhichPhp { .. } | Cmd::ExecPhp { .. } => unreachable!(),
        Cmd::Install { no_agent, minimal, no_system } => install(no_agent, minimal, no_system).await?,
        Cmd::Update { check, name, series } => update(check, name, series).await?,
        Cmd::SelfUpdate => self_update().await?,
        Cmd::Refresh => {
            system::write_shims(&config::Config::load())?;
            if platform::autostart_installed() {
                platform::install_autostart(&std::env::current_exe()?, &paths::logs().join("daemon.log"))?;
            }
        }
        Cmd::Available => {
            let res = get("/api/packages").await?;
            for (name, lines) in res["catalog"].as_object().into_iter().flatten() {
                let lines: Vec<String> = lines
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| format!("{} ({})", l[0].as_str().unwrap_or(""), l[1].as_str().unwrap_or("")))
                    .collect();
                println!("{name:<12} {}", lines.join(", "));
            }
        }
        Cmd::Uninstall => {
            platform::remove_autostart();
            if daemon_up().await {
                let _ = post("/api/shutdown", json!({})).await;
            }
            let cfg = config::Config::load();
            if let Some(script) = platform::uninstall_script(&cfg) {
                platform::run_privileged_tty(&script)?;
            }
            println!("Pack stopped. Your data is still in {} — delete it to remove everything.", paths::home().display());
        }
        Cmd::Status => {
            let s = get("/api/status").await?;
            println!("Pack {}  ({})", s["version"].as_str().unwrap_or(""), s["home"].as_str().unwrap_or(""));
            println!("  web server   {}", state_label(&s["caddy"]["status"]));
            let dns = if s["dns"]["resolver"].as_bool() == Some(true) { "\x1b[32m● ok\x1b[0m" } else { "\x1b[31m● not set up — run `pack install`\x1b[0m" };
            println!("  .{} DNS    {dns}", s["tld"].as_str().unwrap_or("test"));
            let ca = if s["ca_trusted"].as_bool() == Some(true) { "\x1b[32m● trusted\x1b[0m" } else { "\x1b[33m○ not trusted\x1b[0m" };
            println!("  HTTPS CA     {ca}");
            for p in s["php"].as_array().into_iter().flatten() {
                println!("  PHP {:<8} {}", p["version"].as_str().unwrap_or(""), state_label(&p["status"]));
            }
            let svc = get("/api/services").await?;
            for i in svc["instances"].as_array().into_iter().flatten() {
                println!("  {:<12} {}  :{}", i["id"].as_str().unwrap_or(""), state_label(&i["status"]), i["port"]);
            }
        }
        Cmd::Park { path } => {
            let path = path.unwrap_or_else(cwd);
            post("/api/sites/park", json!({ "path": path })).await?;
            println!("Folders in {} are now served as <folder>.test", path.display());
        }
        Cmd::Unpark { path } => {
            post("/api/sites/unpark", json!({ "path": path.unwrap_or_else(cwd) })).await?;
        }
        Cmd::Paths => {
            for p in config::Config::load().paths {
                println!("{}", p.display());
            }
        }
        Cmd::Link { name, path } => {
            let res = post("/api/sites/link", json!({ "path": path.unwrap_or_else(cwd), "name": name })).await?;
            println!("Linked → {}", res["site"]["url"].as_str().unwrap_or(""));
        }
        Cmd::Unlink { name } => {
            post("/api/sites/unlink", json!({ "name": site_name(name)? })).await?;
        }
        Cmd::Sites => {
            let res = get("/api/sites").await?;
            for s in res["sites"].as_array().into_iter().flatten() {
                let lock = if s["secured"].as_bool() == Some(true) { "🔒" } else { "  " };
                let target = s["proxy"].as_str().map(|p| format!("→ {p}")).unwrap_or_else(|| s["path_display"].as_str().unwrap_or("").to_string());
                println!(
                    "{lock} {:<36} PHP {:<4}{} {:<10} {target}",
                    s["url"].as_str().unwrap_or(""),
                    s["php"].as_str().unwrap_or(""),
                    if s["isolated"].as_bool() == Some(true) { "*" } else { " " },
                    s["framework"].as_str().unwrap_or(""),
                );
            }
        }
        Cmd::Secure { name } => {
            let name = site_name(name)?;
            post("/api/sites/secure", json!({ "name": name, "secure": true })).await?;
            println!("{name} is now served over HTTPS");
        }
        Cmd::Unsecure { name } => {
            post("/api/sites/secure", json!({ "name": site_name(name)?, "secure": false })).await?;
        }
        Cmd::Isolate { version, site } => {
            let name = site_name(site)?;
            post("/api/sites/isolate", json!({ "name": name, "php": version })).await?;
            println!("{name} now uses PHP {version}");
        }
        Cmd::Unisolate { site } => {
            post("/api/sites/isolate", json!({ "name": site_name(site)?, "php": null })).await?;
        }
        Cmd::Proxy { name, upstream } => {
            post("/api/sites/proxy", json!({ "name": name, "upstream": upstream })).await?;
            println!("{name}.test → {upstream}");
        }
        Cmd::Unproxy { name } => {
            post("/api/sites/unlink", json!({ "name": name })).await?;
        }
        Cmd::Open { name } => {
            let name = site_name(name)?;
            let res = get("/api/sites").await?;
            let url = res["sites"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|s| s["name"] == name.as_str())
                .and_then(|s| s["url"].as_str().map(str::to_string))
                .ok_or_else(|| anyhow::anyhow!("no site {name}"))?;
            platform::open_url(&url)?;
        }
        Cmd::Share { name } => {
            let name = site_name(name)?;
            eprintln!("Starting a Cloudflare quick tunnel for {name}…");
            let res = post("/api/sites/share", json!({ "name": name })).await?;
            println!("{}", res["url"].as_str().unwrap_or(""));
            println!("Stop sharing with `pack unshare {name}`.");
        }
        Cmd::Unshare { name } => {
            post("/api/sites/unshare", json!({ "name": site_name(name)? })).await?;
        }
        Cmd::Use { version } => {
            if !php::is_installed(&version) {
                post("/api/php/install", json!({ "version": version })).await?;
                wait_for_jobs().await?;
            }
            post("/api/php/default", json!({ "version": version })).await?;
            println!("Default PHP is now {version}");
        }
        Cmd::Php { cmd } => match cmd {
            PhpCmd::List => {
                let res = get("/api/php").await?;
                let default = res["default"].as_str().unwrap_or("");
                println!("Installed:");
                for p in res["installed"].as_array().into_iter().flatten() {
                    let v = p["version"].as_str().unwrap_or("");
                    let latest = res["available"][v].as_str().unwrap_or("");
                    let patch = p["patch"].as_str().unwrap_or("");
                    let update = if !latest.is_empty() && packages::key(latest) > packages::key(patch) && !patch.ends_with("-local") {
                        format!("  (update: {latest})")
                    } else {
                        String::new()
                    };
                    println!(
                        "  {} {v:<5} {patch:<12} {}{update}",
                        if v == default { "*" } else { " " },
                        state_label(&p["status"])
                    );
                }
                let available: Vec<String> = res["available"].as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
                println!("Available: {}", available.join(", "));
            }
            PhpCmd::Install { version } => {
                post("/api/php/install", json!({ "version": version })).await?;
                wait_for_jobs().await?;
                if !php::is_installed(&version) {
                    anyhow::bail!("PHP {version} was not installed");
                }
                println!("PHP {version} installed (php{} on your PATH)", version.replace('.', ""));
            }
            PhpCmd::Import { version, dir } => {
                php::import(&version, &dir)?;
                println!("Imported PHP {version}");
                if daemon_up().await {
                    post("/api/restart", json!({})).await?;
                }
            }
            PhpCmd::Uninstall { version } => {
                post("/api/php/uninstall", json!({ "version": version })).await?;
            }
        },
        Cmd::Service { cmd } => service(cmd).await?,
        Cmd::Db { name, service } => {
            let id = match service {
                Some(id) => id,
                None => config::Config::load()
                    .services
                    .into_iter()
                    .find(|s| s.kind == "mysql" || s.kind == "postgres")
                    .map(|s| s.id)
                    .ok_or_else(|| anyhow::anyhow!("no MySQL or Postgres service — `pack service add mysql`"))?,
            };
            post(&format!("/api/services/{id}/databases"), json!({ "name": name })).await?;
            println!("Created database {name} in {id}");
        }
        Cmd::Dumps => {
            eprintln!("Waiting for dump() calls… (Ctrl-C to stop)");
            let mut since = get("/api/dumps").await?["dumps"].as_array().and_then(|d| d.last()).and_then(|d| d["id"].as_u64()).unwrap_or(0);
            loop {
                let res = get(&format!("/api/dumps?since={since}")).await?;
                for d in res["dumps"].as_array().into_iter().flatten() {
                    since = d["id"].as_u64().unwrap_or(since);
                    println!(
                        "\x1b[90m── {} {}:{}\x1b[0m\n{}",
                        d["site"].as_str().unwrap_or("-"),
                        d["file"].as_str().unwrap_or(""),
                        d["line"],
                        d["text"].as_str().unwrap_or("")
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        Cmd::Logs { name } => {
            let name = site_name(name)?;
            let res = get(&format!("/api/logs?site={name}")).await?;
            let entries = res["entries"].as_array().cloned().unwrap_or_default();
            for e in entries.iter().take(30).rev() {
                let level = e["level"].as_str().unwrap_or("");
                let color = match level {
                    "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY" => "31",
                    "WARNING" => "33",
                    _ => "36",
                };
                println!("\x1b[90m{}\x1b[0m \x1b[{color}m{level}\x1b[0m {}", e["time"].as_str().unwrap_or(""), e["message"].as_str().unwrap_or(""));
            }
        }
        Cmd::Restart => {
            post("/api/restart", json!({})).await?;
        }
    }
    Ok(())
}

async fn service(cmd: ServiceCmd) -> anyhow::Result<()> {
    match cmd {
        ServiceCmd::List => {
            let res = get("/api/services").await?;
            for i in res["instances"].as_array().into_iter().flatten() {
                println!(
                    "{:<14} {:<18} {:<10} :{:<6} {}",
                    i["id"].as_str().unwrap_or(""),
                    i["name"].as_str().unwrap_or(""),
                    i["installed_version"].as_str().unwrap_or(i["version"].as_str().unwrap_or("")),
                    i["port"],
                    state_label(&i["status"])
                );
            }
            println!("\nAvailable:");
            for k in res["kinds"].as_array().into_iter().flatten() {
                println!(
                    "  {:<12} {:<20} versions: {}",
                    k["id"].as_str().unwrap_or(""),
                    k["name"].as_str().unwrap_or(""),
                    k["versions"].as_array().map(|v| v.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
                );
            }
        }
        ServiceCmd::Add { kind, version, port, path } => {
            let res = post("/api/services", json!({ "kind": kind, "version": version, "port": port, "path": path })).await?;
            let id = res["id"].as_str().unwrap_or("").to_string();
            eprintln!("Installing {id}…");
            if !wait_for_jobs().await? {
                anyhow::bail!("{id} was added but could not be installed — fix the problem and run `pack service start {id}`");
            }
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            let svc = get("/api/services").await?;
            if let Some(i) = svc["instances"].as_array().into_iter().flatten().find(|i| i["id"] == id.as_str()) {
                println!("{} {}  :{}", id, state_label(&i["status"]), i["port"]);
                if let Some(ui) = i["ui_port"].as_u64() {
                    println!("Dashboard: http://127.0.0.1:{ui}");
                }
                println!("\n{}", i["env"].as_str().unwrap_or(""));
            }
        }
        ServiceCmd::Start { id } => {
            post(&format!("/api/services/{id}/start"), json!({})).await?;
            wait_for_jobs().await?;
        }
        ServiceCmd::Stop { id } => {
            post(&format!("/api/services/{id}/stop"), json!({})).await?;
        }
        ServiceCmd::Remove { id, data } => {
            call("DELETE", &format!("/api/services/{id}?data={data}"), None).await?;
        }
        ServiceCmd::Version { id, series } => {
            post(&format!("/api/services/{id}/version"), json!({ "version": series })).await?;
            wait_for_jobs().await?;
            println!("{id} now runs {series}");
        }
        ServiceCmd::Env { id } => {
            let svc = get("/api/services").await?;
            let i = svc["instances"].as_array().into_iter().flatten().find(|i| i["id"] == id.as_str()).ok_or_else(|| anyhow::anyhow!("no service {id}"))?;
            println!("{}", i["env"].as_str().unwrap_or(""));
        }
    }
    Ok(())
}

async fn install(no_agent: bool, minimal: bool, no_system: bool) -> anyhow::Result<()> {
    paths::ensure_layout()?;
    let cfg = config::Config::load();
    if !paths::config_file().exists() {
        cfg.save()?;
    }
    if no_agent {
        system::write_shims(&cfg)?;
        ensure_daemon().await?;
    } else if daemon_up().await {
        eprintln!("→ Pack is already running");
    } else {
        system::write_shims(&cfg)?;
        eprintln!("→ Starting Pack at login");
        platform::install_autostart(&std::env::current_exe()?, &paths::logs().join("daemon.log"))?;
        ensure_daemon().await?;
    }
    eprintln!("→ Installing Caddy, PHP {}, Composer{}", cfg.php, if minimal { "" } else { ", MySQL, PostgreSQL, Redis and Mail" });
    post("/api/bootstrap", json!({ "minimal": minimal })).await?;
    if !wait_for_jobs().await? {
        anyhow::bail!("installation failed");
    }
    if !no_system {
        eprintln!("→ Setting up .{} domains and trusted HTTPS (asks for your password once)", cfg.tld);
        system::configure_system(&cfg, false).await?;
        if system::add_to_path()? {
            eprintln!("→ Added {} to your PATH (open a new terminal)", paths::bin().display());
        }
    }
    if !minimal {
        let svc = get("/api/services").await?;
        println!();
        for i in svc["instances"].as_array().into_iter().flatten() {
            println!("  {:<10} {}  127.0.0.1:{}", i["id"].as_str().unwrap_or(""), state_label(&i["status"]), i["port"]);
        }
    }
    let scheme = if cfg.https { "https" } else { "http" };
    match cfg.paths.first() {
        Some(p) => println!("\nPack is ready. Put projects in {} and open {scheme}://<folder>.{}", paths::tilde(p), cfg.tld),
        None => println!("\nPack is ready. Run `pack link` in a project to serve it at {scheme}://<folder>.{}", cfg.tld),
    }
    Ok(())
}

async fn update(check: bool, name: Option<String>, series: Option<String>) -> anyhow::Result<()> {
    let res = get("/api/packages?refresh=1").await?;
    let pending: Vec<Value> = res["installed"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["update"] == true)
        .filter(|p| name.as_ref().is_none_or(|n| p["name"] == n.as_str()))
        .filter(|p| series.as_ref().is_none_or(|s| p["series"] == s.as_str()))
        .cloned()
        .collect();
    if pending.is_empty() {
        println!("Everything is up to date.");
        return Ok(());
    }
    for p in &pending {
        println!(
            "  {:<12} {:<6} {} → {}",
            p["name"].as_str().unwrap_or(""),
            p["series"].as_str().unwrap_or(""),
            p["version"].as_str().unwrap_or(""),
            p["latest"].as_str().unwrap_or("")
        );
    }
    if check {
        return Ok(());
    }
    for p in &pending {
        post("/api/packages/update", json!({ "name": p["name"], "series": p["series"] })).await?;
        wait_for_jobs().await?;
    }
    println!("Updated.");
    Ok(())
}

/// Replace the pack binary with the newest GitHub release of the Pack repo.
async fn self_update() -> anyhow::Result<()> {
    let tag = download::latest_github_tag(packages::REPO).await?;
    let latest = tag.trim_start_matches('v');
    if packages::key(latest) <= packages::key(env!("CARGO_PKG_VERSION")) {
        println!("pack {} is the latest version.", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let asset = format!("pack-{latest}-{}.tar.gz", packages::host_target());
    let base = format!("https://github.com/{}/releases/download/{tag}", packages::REPO);
    eprintln!("Downloading pack {latest}…");
    let file = download::fetch(&format!("{base}/{asset}"), &format!("pack {latest}")).await?;
    let expected = download::text(&format!("{base}/{asset}.sha256")).await?;
    let bytes = std::fs::read(&file)?;
    use sha2::Digest;
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    if !expected.starts_with(&digest) {
        anyhow::bail!("checksum mismatch for {asset}");
    }
    let dir = packages::series_dir("pack", "stable").join(latest);
    download::extract(&file, &dir, 0).await?;
    // Let the new binary point the shims and autostart at itself, then restart.
    let new = dir.join(platform::exe("pack"));
    let st = std::process::Command::new(&new).arg("refresh").status()?;
    if !st.success() {
        anyhow::bail!("the new pack binary failed to start");
    }
    if daemon_up().await {
        let _ = post("/api/shutdown", json!({})).await;
    }
    if platform::autostart_installed() && !cfg!(windows) {
        platform::kick_autostart(true);
    }
    println!("pack updated to {latest}.");
    Ok(())
}
