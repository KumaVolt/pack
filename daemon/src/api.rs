//! HTTP API on 127.0.0.1:7171, used by the CLI and the dashboard.

use crate::app::{Dump, Shared};
use crate::sites::{self, SiteKind};
use crate::supervisor::Status;
use crate::{caddy, download, logs, packages, paths, php, services, system};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

pub const PORT: u16 = 7171;

pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{:#}", self.0) }))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

type R = Result<Json<Value>, ApiError>;

fn ok() -> R {
    Ok(Json(json!({ "ok": true })))
}

pub fn router(app: Shared) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../ui/index.html")) }))
        .route("/api/status", get(status))
        .route("/api/sites", get(list_sites))
        .route("/api/sites/park", post(park))
        .route("/api/sites/unpark", post(unpark))
        .route("/api/sites/link", post(link))
        .route("/api/sites/unlink", post(unlink))
        .route("/api/sites/secure", post(secure))
        .route("/api/sites/isolate", post(isolate))
        .route("/api/sites/proxy", post(proxy))
        .route("/api/sites/share", post(share))
        .route("/api/sites/unshare", post(unshare))
        .route("/api/php", get(php_list))
        .route("/api/php/install", post(php_install))
        .route("/api/php/default", post(php_default))
        .route("/api/php/uninstall", post(php_uninstall))
        .route("/api/services", get(services_list).post(service_create))
        .route("/api/services/{id}/start", post(service_start))
        .route("/api/services/{id}/stop", post(service_stop))
        .route("/api/services/{id}", delete(service_delete))
        .route("/api/services/{id}/databases", post(service_database))
        .route("/api/services/{id}/version", post(service_version))
        .route("/api/packages", get(packages_list))
        .route("/api/packages/update", post(packages_update))
        .route("/api/dumps", get(dumps_list).post(dumps_add).delete(dumps_clear))
        .route("/api/logs", get(site_logs))
        .route("/api/logs/system", get(system_logs))
        .route("/api/settings", get(settings).post(settings_update))
        .route("/api/setup", post(setup))
        .route("/api/bootstrap", post(bootstrap))
        .route("/api/restart", post(restart))
        .route("/api/shutdown", post(shutdown))
        .route("/api/open", post(open_url))
        .layer(axum::middleware::from_fn(guard_origin))
        .with_state(app)
}

/// Origins allowed to use the API from a browser context: the desktop app
/// (tauri://localhost on macOS/Linux, http(s)://tauri.localhost on Windows) and
/// the dashboard itself. Anything else — e.g. a website trying to drive the
/// local API — is refused.
const APP_ORIGINS: &[&str] = &["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];

async fn guard_origin(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    use axum::http::{HeaderValue, Method, header};
    let origin = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()).map(str::to_string);
    let same = origin.as_deref().is_none_or(|o| o == format!("http://127.0.0.1:{PORT}") || o == format!("http://localhost:{PORT}"));
    let app = origin.as_deref().is_some_and(|o| APP_ORIGINS.contains(&o));
    if !same && !app {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    let preflight = req.method() == Method::OPTIONS;
    let mut res = if preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(req).await };
    if let (true, Some(o)) = (app, origin) {
        let h = res.headers_mut();
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_str(&o).unwrap());
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, DELETE, OPTIONS"));
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("content-type"));
    }
    res
}

#[derive(Deserialize)]
struct UrlBody {
    url: String,
}

/// Open a site or dashboard in the default browser.
async fn open_url(Json(b): Json<UrlBody>) -> R {
    if !(b.url.starts_with("http://") || b.url.starts_with("https://")) {
        return Err(anyhow::anyhow!("only http(s) URLs can be opened").into());
    }
    crate::platform::open_url(&b.url)?;
    ok()
}

fn status_json(s: Status) -> Value {
    serde_json::to_value(s).unwrap_or(Value::Null)
}

async fn status(State(app): State<Shared>) -> R {
    let cfg = app.config();
    let php: Vec<Value> = cfg
        .php_versions_in_use()
        .iter()
        .map(|v| json!({ "version": v, "installed": php::is_installed(v), "status": status_json(app.status_of(&format!("php-{v}"))) }))
        .collect();
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "home": paths::home(),
        "tld": cfg.tld,
        "caddy": { "installed": caddy::binary().exists(), "status": status_json(app.status_of("caddy")) },
        "os": crate::platform::target().0,
        "dns": { "port": crate::platform::dns_port(&cfg), "resolver": crate::platform::dns_ok(&cfg), "low_ports": crate::platform::low_ports_ok() },
        "ca_trusted": system::ca_trusted(),
        "php": php,
        "default_php": cfg.php,
        "composer": system::composer_phar().exists(),
        "on_path": crate::platform::on_path(&paths::bin()),
        "downloads": download::progress(),
        "jobs": app.jobs.lock().unwrap().values().cloned().collect::<Vec<_>>(),
    })))
}

async fn list_sites(State(app): State<Shared>) -> R {
    let cfg = app.config();
    let list: Vec<Value> = sites::all(&cfg)
        .into_iter()
        .map(|s| {
            let share = app.status_of(&format!("share:{}", s.name));
            let share_url = matches!(share, Status::Running).then(|| share_url(&s.name)).flatten();
            let mut v = serde_json::to_value(&s).unwrap();
            v["path_display"] = json!(paths::tilde(&s.path));
            v["shared"] = json!(share_url);
            v
        })
        .collect();
    Ok(Json(json!({ "sites": list, "paths": cfg.paths, "tld": cfg.tld })))
}

#[derive(Deserialize)]
struct PathBody {
    path: PathBuf,
    name: Option<String>,
}

#[derive(Deserialize)]
struct NameBody {
    name: String,
}

fn expand(path: &std::path::Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        return paths::user_home().join(rest);
    }
    path.to_path_buf()
}

async fn park(State(app): State<Shared>, Json(b): Json<PathBody>) -> R {
    let path = expand(&b.path);
    if !path.is_dir() {
        return Err(anyhow::anyhow!("{} is not a directory", path.display()).into());
    }
    app.update(|c| {
        if !c.paths.contains(&path) {
            c.paths.push(path.clone());
        }
    })?;
    app.reconcile().await?;
    ok()
}

async fn unpark(State(app): State<Shared>, Json(b): Json<PathBody>) -> R {
    let path = expand(&b.path);
    app.update(|c| c.paths.retain(|p| p != &path))?;
    app.reconcile().await?;
    ok()
}

async fn link(State(app): State<Shared>, Json(b): Json<PathBody>) -> R {
    let path = expand(&b.path);
    if !path.is_dir() {
        return Err(anyhow::anyhow!("{} is not a directory", path.display()).into());
    }
    let name = sites::slug(&b.name.unwrap_or_else(|| path.file_name().unwrap_or_default().to_string_lossy().into()));
    if name.is_empty() {
        return Err(anyhow::anyhow!("invalid site name").into());
    }
    app.update(|c| {
        c.links.insert(name.clone(), path.clone());
    })?;
    app.reconcile().await?;
    let site = sites::find(&app.config(), &name);
    if let Some(site) = &site {
        sites::sync_app_url(site);
    }
    Ok(Json(json!({ "ok": true, "site": site })))
}

async fn unlink(State(app): State<Shared>, Json(b): Json<NameBody>) -> R {
    let name = b.name.clone();
    app.update(|c| {
        c.links.remove(&name);
        c.proxies.remove(&name);
        c.secured.retain(|s| s != &name);
        c.insecure.retain(|s| s != &name);
        c.isolated.remove(&name);
    })?;
    app.reconcile().await?;
    ok()
}

#[derive(Deserialize)]
struct SecureBody {
    name: String,
    secure: bool,
}

async fn secure(State(app): State<Shared>, Json(b): Json<SecureBody>) -> R {
    let cfg = app.config();
    if sites::find(&cfg, &b.name).is_none() {
        return Err(anyhow::anyhow!("no site named {}", b.name).into());
    }
    app.update(|c| c.set_secured(&b.name, b.secure))?;
    app.reconcile().await?;
    if let Some(site) = sites::find(&app.config(), &b.name) {
        sites::sync_app_url(&site);
    }
    ok()
}

#[derive(Deserialize)]
struct IsolateBody {
    name: String,
    php: Option<String>,
}

async fn isolate(State(app): State<Shared>, Json(b): Json<IsolateBody>) -> R {
    if sites::find(&app.config(), &b.name).is_none() {
        return Err(anyhow::anyhow!("no site named {}", b.name).into());
    }
    if let Some(v) = &b.php {
        if !php::is_installed(v) {
            return Err(anyhow::anyhow!("PHP {v} is not installed — run `pack php install {v}`").into());
        }
    }
    app.update(|c| match &b.php {
        Some(v) => {
            c.isolated.insert(b.name.clone(), v.clone());
        }
        None => {
            c.isolated.remove(&b.name);
        }
    })?;
    app.reconcile().await?;
    ok()
}

#[derive(Deserialize)]
struct ProxyBody {
    name: String,
    upstream: String,
}

async fn proxy(State(app): State<Shared>, Json(b): Json<ProxyBody>) -> R {
    let name = sites::slug(&b.name);
    let upstream = if b.upstream.contains("://") { b.upstream.clone() } else { format!("http://{}", b.upstream) };
    app.update(|c| {
        c.proxies.insert(name.clone(), upstream.clone());
    })?;
    app.reconcile().await?;
    ok()
}

fn cloudflared() -> PathBuf {
    packages::current("cloudflared", "stable").join("cloudflared")
}

fn share_url(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(paths::logs().join(format!("share-{name}.log"))).ok()?;
    let start = text.rfind("https://")?;
    let url: String = text[start..].chars().take_while(|c| !c.is_whitespace() && *c != '|').collect();
    url.contains("trycloudflare.com").then_some(url)
}

async fn share(State(app): State<Shared>, Json(b): Json<NameBody>) -> R {
    let cfg = app.config();
    let site = sites::find(&cfg, &b.name).ok_or_else(|| anyhow::anyhow!("no site named {}", b.name))?;
    if !cloudflared().exists() {
        packages::install("cloudflared", "stable", None).await?;
    }
    let log = paths::logs().join(format!("share-{}.log", site.name));
    let _ = std::fs::remove_file(&log);
    let mut args = vec!["tunnel".to_string(), "--no-autoupdate".into()];
    if site.secured {
        args.extend([
            "--url".into(),
            format!("https://127.0.0.1:{}", cfg.https_port),
            "--no-tls-verify".into(),
            "--origin-server-name".into(),
            site.host.clone(),
        ]);
    } else {
        args.extend(["--url".into(), format!("http://127.0.0.1:{}", cfg.http_port)]);
    }
    args.extend(["--http-host-header".into(), site.host.clone()]);
    app.sup
        .ensure(crate::supervisor::Spec {
            name: format!("share:{}", site.name),
            program: cloudflared(),
            args,
            env: Default::default(),
            cwd: None,
            log,
        })
        .await;
    for _ in 0..150 {
        if let Some(url) = share_url(&site.name) {
            return Ok(Json(json!({ "ok": true, "url": url })));
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(anyhow::anyhow!("tunnel did not come up — see {}", paths::logs().join(format!("share-{}.log", site.name)).display()).into())
}

async fn unshare(State(app): State<Shared>, Json(b): Json<NameBody>) -> R {
    app.sup.stop(&format!("share:{}", b.name)).await;
    ok()
}

async fn php_list(State(app): State<Shared>) -> R {
    let cfg = app.config();
    let available = php::available().await.unwrap_or_default();
    let installed: Vec<Value> = php::installed()
        .into_iter()
        .map(|p| {
            let status = app.status_of(&format!("php-{}", p.version));
            let mut v = serde_json::to_value(&p).unwrap();
            v["status"] = status_json(status);
            v["dir"] = json!(paths::php_dir(&p.version));
            v
        })
        .collect();
    Ok(Json(json!({ "installed": installed, "available": available, "default": cfg.php, "extensions": php::EXTENSIONS })))
}

#[derive(Deserialize)]
struct VersionBody {
    version: String,
}

async fn php_install(State(app): State<Shared>, Json(b): Json<VersionBody>) -> R {
    let v = b.version.clone();
    app.spawn_job(format!("php-{v}"), format!("Installing PHP {v}"), async move { php::install(&v).await.map(|_| ()) });
    ok()
}

async fn php_default(State(app): State<Shared>, Json(b): Json<VersionBody>) -> R {
    if !php::is_installed(&b.version) {
        return Err(anyhow::anyhow!("PHP {} is not installed", b.version).into());
    }
    app.update(|c| c.php = b.version.clone())?;
    app.reconcile().await?;
    ok()
}

async fn php_uninstall(State(app): State<Shared>, Json(b): Json<VersionBody>) -> R {
    let cfg = app.config();
    if cfg.php_versions_in_use().contains(&b.version) {
        return Err(anyhow::anyhow!("PHP {} is in use (default or isolated site)", b.version).into());
    }
    app.stop_php(&b.version).await;
    packages::uninstall("php", &b.version)?;
    let _ = std::fs::remove_dir_all(paths::php_dir(&b.version));
    let _ = std::fs::remove_file(paths::bin().join(format!("php{}", b.version.replace('.', ""))));
    ok()
}

async fn services_list(State(app): State<Shared>) -> R {
    let cfg = app.config();
    let jobs = app.jobs.lock().unwrap().clone();
    let instances: Vec<Value> = cfg
        .services
        .iter()
        .map(|inst| {
            let job = jobs.get(&format!("service-{}", inst.id));
            json!({
                "id": inst.id,
                "kind": inst.kind,
                "name": services::kind(&inst.kind).map(|k| k.name),
                "version": inst.version,
                "installed_version": services::installed_version(inst),
                "series": inst.version,
                "port": inst.port,
                "ui_port": services::ui_port(inst),
                "autostart": inst.autostart,
                "options": inst.options,
                "installed": services::is_installed(inst),
                "status": status_json(app.status_of(&format!("service:{}", inst.id))),
                "job": job,
                "env": services::env_snippet(inst),
                "log": paths::logs().join(format!("{}.log", inst.id)),
            })
        })
        .collect();
    let catalog = packages::manifest(false).await.map(|m| m.catalog()).unwrap_or_default();
    Ok(Json(json!({ "kinds": services::KINDS, "instances": instances, "series": catalog })))
}

#[derive(Deserialize)]
struct CreateService {
    kind: String,
    version: Option<String>,
    port: Option<u16>,
    path: Option<String>,
}

async fn service_create(State(app): State<Shared>, Json(b): Json<CreateService>) -> R {
    let path = b.path.map(|p| expand(std::path::Path::new(&p)).display().to_string());
    let inst = app.add_service(&b.kind, b.version, b.port, path)?;
    let id = inst.id.clone();
    let name = services::kind(&inst.kind).map(|k| k.name).unwrap_or("service");
    app.spawn_job(format!("service-{id}"), format!("Installing {name} {}", inst.version), async move { services::install(&inst).await });
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// Move a service to another series (e.g. MySQL 8.4 → 9.7), keeping its data.
async fn service_version(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<VersionBody>) -> R {
    let inst = app.find_service(&id).ok_or_else(|| anyhow::anyhow!("no service {id}"))?;
    if !services::can_switch(&inst.kind, &inst.version, &b.version) {
        return Err(anyhow::anyhow!(
            "{} can't move from {} to {} in place — add a second {} service and migrate your data",
            inst.kind, inst.version, b.version, inst.kind
        )
        .into());
    }
    let mut next = inst.clone();
    next.version = b.version.clone();
    let job_app = app.clone();
    app.spawn_job(format!("service-{id}"), format!("Switching {id} to {}", b.version), async move {
        services::install(&next).await?;
        job_app.sup.stop(&format!("service:{}", next.id)).await;
        let cfg = job_app.update(|c| {
            if let Some(s) = c.services.iter_mut().find(|s| s.id == next.id) {
                s.version = next.version.clone();
            }
        })?;
        // Drop the old version line if no other instance still runs it.
        if !cfg.services.iter().any(|s| s.kind == inst.kind && s.version == inst.version) {
            packages::uninstall(&inst.kind, &inst.version)?;
        }
        Ok(())
    });
    ok()
}

#[derive(Deserialize)]
struct RefreshQuery {
    #[serde(default)]
    refresh: Option<String>,
}

async fn packages_list(State(app): State<Shared>, Query(q): Query<RefreshQuery>) -> R {
    let m = packages::manifest(q.refresh.is_some()).await?;
    let installed = packages::installed(&m);
    let _ = &app;
    Ok(Json(json!({
        "generated": m.generated,
        "arch": packages::host_arch(),
        "catalog": m.catalog(),
        "installed": installed,
        "updates": installed.iter().filter(|p| p.update).count(),
    })))
}

#[derive(Deserialize)]
struct UpdateBody {
    name: Option<String>,
    series: Option<String>,
}

async fn packages_update(State(app): State<Shared>, Json(b): Json<UpdateBody>) -> R {
    let only = b.name.zip(b.series);
    let label = match &only {
        Some((n, s)) => format!("Updating {n} {s}"),
        None => "Updating packages".to_string(),
    };
    let job_app = app.clone();
    app.spawn_job("update".into(), label, async move { job_app.update_packages(only).await.map(|_| ()) });
    ok()
}

async fn set_autostart(app: &Shared, id: &str, on: bool) -> R {
    if app.find_service(id).is_none() {
        return Err(anyhow::anyhow!("no service {id}").into());
    }
    app.update(|c| {
        if let Some(s) = c.services.iter_mut().find(|s| s.id == id) {
            s.autostart = on;
        }
    })?;
    app.reconcile().await?;
    ok()
}

async fn service_start(State(app): State<Shared>, Path(id): Path<String>) -> R {
    if let Some(inst) = app.find_service(&id) {
        if !services::is_installed(&inst) {
            let i = inst.clone();
            app.spawn_job(format!("service-{id}"), format!("Installing {id}"), async move { services::install(&i).await });
        }
    }
    set_autostart(&app, &id, true).await
}

async fn service_stop(State(app): State<Shared>, Path(id): Path<String>) -> R {
    set_autostart(&app, &id, false).await
}

#[derive(Deserialize)]
struct DeleteQuery {
    #[serde(default)]
    data: bool,
}

async fn service_delete(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<DeleteQuery>) -> R {
    app.update(|c| c.services.retain(|s| s.id != id))?;
    app.reconcile().await?;
    if q.data {
        let _ = std::fs::remove_dir_all(paths::service_data(&id));
    }
    ok()
}

async fn service_database(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<NameBody>) -> R {
    let inst = app.find_service(&id).ok_or_else(|| anyhow::anyhow!("no service {id}"))?;
    services::create_database(&inst, &b.name).await?;
    ok()
}

#[derive(Deserialize)]
struct SinceQuery {
    #[serde(default)]
    since: u64,
}

async fn dumps_list(State(app): State<Shared>, Query(q): Query<SinceQuery>) -> R {
    let dumps: Vec<Dump> = app.dumps.lock().unwrap().iter().filter(|d| d.id > q.since).cloned().collect();
    Ok(Json(json!({ "dumps": dumps })))
}

async fn dumps_add(State(app): State<Shared>, Json(d): Json<Dump>) -> R {
    app.add_dump(d);
    ok()
}

async fn dumps_clear(State(app): State<Shared>) -> R {
    app.dumps.lock().unwrap().clear();
    ok()
}

#[derive(Deserialize)]
struct LogQuery {
    site: String,
    file: Option<String>,
}

async fn site_logs(State(app): State<Shared>, Query(q): Query<LogQuery>) -> R {
    let site = sites::find(&app.config(), &q.site).ok_or_else(|| anyhow::anyhow!("no site {}", q.site))?;
    if site.kind == SiteKind::Proxy {
        return Ok(Json(json!({ "files": [], "entries": [] })));
    }
    let files = logs::files(&site.path);
    let chosen = q
        .file
        .and_then(|f| files.iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == f)).cloned())
        .or_else(|| files.first().cloned());
    let entries = chosen.as_ref().map(|f| logs::parse(&logs::tail(f, 2_000_000), 300)).unwrap_or_default();
    let names: Vec<String> = files.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into())).collect();
    Ok(Json(json!({ "files": names, "file": chosen.and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())), "entries": entries })))
}

#[derive(Deserialize)]
struct SystemLogQuery {
    name: Option<String>,
}

async fn system_logs(Query(q): Query<SystemLogQuery>) -> R {
    let mut names: Vec<String> = std::fs::read_dir(paths::logs())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| n.ends_with(".log"))
        .collect();
    names.sort();
    let text = q
        .name
        .filter(|n| names.contains(n))
        .map(|n| logs::tail(&paths::logs().join(n), 200_000))
        .unwrap_or_default();
    Ok(Json(json!({ "files": names, "text": text })))
}

async fn settings(State(app): State<Shared>) -> R {
    Ok(Json(serde_json::to_value(app.config())?))
}

#[derive(Deserialize)]
struct SettingsBody {
    dumps: Option<bool>,
    lan_access: Option<bool>,
    https: Option<bool>,
    auto_update: Option<bool>,
    manifest_url: Option<String>,
    php_ini: Option<crate::config::PhpIni>,
}

async fn settings_update(State(app): State<Shared>, Json(b): Json<SettingsBody>) -> R {
    let restart_php = b.php_ini.is_some() || b.dumps.is_some();
    let https_changed = b.https.is_some();
    app.update(|c| {
        if let Some(v) = b.dumps {
            c.dumps = v;
        }
        if let Some(v) = b.lan_access {
            c.lan_access = v;
        }
        if let Some(v) = b.https {
            c.https = v;
            c.insecure.clear();
            c.secured.clear();
        }
        if let Some(v) = b.auto_update {
            c.auto_update = v;
        }
        if let Some(v) = b.manifest_url {
            c.manifest_url = v;
        }
        if let Some(v) = b.php_ini {
            c.php_ini = v;
        }
    })?;
    app.reconcile().await?;
    if https_changed {
        for site in sites::all(&app.config()) {
            sites::sync_app_url(&site);
        }
    }
    if restart_php {
        restart_php_pools(&app).await;
    }
    ok()
}

async fn restart_php_pools(app: &Shared) {
    for v in app.config().php_versions_in_use() {
        app.stop_php(&v).await;
    }
    let _ = app.reconcile().await;
}

async fn restart(State(app): State<Shared>) -> R {
    app.sup.stop_all().await;
    app.reconcile().await?;
    ok()
}

#[derive(Deserialize, Default)]
struct SetupBody {
    #[serde(default)]
    minimal: bool,
}

/// First-run setup from the app: install everything from the manifest (web
/// server, PHP, Composer, the default services), then ask for the admin
/// password once to route .test and trust the HTTPS CA.
async fn setup(State(app): State<Shared>, body: Option<Json<SetupBody>>) -> R {
    let minimal = body.map(|b| b.0.minimal).unwrap_or_default();
    let notes = app.bootstrap(minimal).await?;
    let cfg = app.config();
    system::configure_system(&cfg, true).await?;
    system::add_to_path()?;
    Ok(Json(json!({ "ok": true, "notes": notes })))
}

/// Install the box (no privileged steps) as a background job; the CLI shows
/// progress and does the sudo part itself.
async fn bootstrap(State(app): State<Shared>, body: Option<Json<SetupBody>>) -> R {
    let minimal = body.map(|b| b.0.minimal).unwrap_or_default();
    let job_app = app.clone();
    app.spawn_job("bootstrap".into(), "Installing Pack".into(), async move {
        for note in job_app.bootstrap(minimal).await? {
            tracing_note(&note);
        }
        Ok(())
    });
    ok()
}

fn tracing_note(note: &str) {
    crate::app::tracing_log(note);
}

/// Stop everything and exit (used by `pack uninstall` and self-update).
async fn shutdown(State(app): State<Shared>) -> R {
    tokio::spawn(async move {
        app.sup.stop_all().await;
        std::process::exit(0);
    });
    ok()
}
