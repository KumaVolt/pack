//! PHP versions: static builds (static-php-cli with the full Laravel
//! extension set) published by the Pack repo, one FPM pool per version in
//! use, and the CLI shims. Binaries live in `packages/php/<series>/current`,
//! configuration in `php/<series>/`.

use crate::config::Config;
use crate::download;
use crate::packages;
use crate::paths;
use crate::platform;
use crate::supervisor::Spec;
use anyhow::{Context, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Extensions compiled into Pack's PHP builds (see scripts/packages.py).
pub const EXTENSIONS: &str = "bcmath,bz2,calendar,ctype,curl,dba,dom,exif,fileinfo,filter,ftp,gd,gmp,iconv,igbinary,intl,mbstring,mysqli,mysqlnd,opcache,openssl,pcntl,pdo,pdo_mysql,pdo_pgsql,pdo_sqlite,pgsql,phar,posix,readline,redis,session,shmop,simplexml,soap,sockets,sodium,sqlite3,sysvmsg,sysvsem,sysvshm,tokenizer,xml,xmlreader,xmlwriter,xsl,zip,zlib,zstd";

#[derive(Debug, Clone, Serialize)]
pub struct InstalledPhp {
    pub version: String,
    pub patch: String,
}

pub fn version_key(v: &str) -> Vec<u64> {
    packages::key(v)
}

pub fn cli(version: &str) -> PathBuf {
    packages::current("php", version).join(platform::exe("php"))
}

/// The FastCGI server: php-fpm on macOS/Linux, php-cgi on Windows.
pub fn fpm(version: &str) -> PathBuf {
    let name = if cfg!(windows) { "php-cgi" } else { "php-fpm" };
    packages::current("php", version).join(platform::exe(name))
}

/// php-cgi can't fork workers on Windows; run a few and let Caddy balance.
const WINDOWS_WORKERS: u16 = 4;

fn worker_port(version: &str, i: u16) -> u16 {
    let k = packages::key(version);
    20000 + (*k.first().unwrap_or(&8) as u16) * 1000 + (*k.get(1).unwrap_or(&0) as u16) * 10 + i
}

/// FastCGI addresses Caddy talks to for a version.
pub fn upstreams(version: &str) -> Vec<String> {
    if cfg!(windows) {
        (0..WINDOWS_WORKERS).map(|i| format!("127.0.0.1:{}", worker_port(version, i))).collect()
    } else {
        vec![format!("unix/{}", paths::php_socket(version).display())]
    }
}

/// PHP_INI_SCAN_DIR: the package's own conf.d (Windows extension list) and
/// the user's conf.d for this version.
pub fn scan_dir(version: &str) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let pkg = packages::current("php", version).join("conf.d");
    let user = paths::php_dir(version).join("conf.d");
    if pkg.exists() {
        format!("{}{sep}{}", pkg.display(), user.display())
    } else {
        user.display().to_string()
    }
}

pub fn installed() -> Vec<InstalledPhp> {
    let mut out: Vec<InstalledPhp> = std::fs::read_dir(paths::home().join("packages/php"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let version = e.file_name().to_string_lossy().to_string();
            let patch = packages::installed_version("php", &version)?;
            is_installed(&version).then_some(InstalledPhp { version, patch })
        })
        .collect();
    out.sort_by(|a, b| version_key(&b.version).cmp(&version_key(&a.version)));
    out
}

pub fn is_installed(version: &str) -> bool {
    fpm(version).exists() && cli(version).exists()
}

/// Series offered by the manifest: series -> newest patch.
pub async fn available() -> anyhow::Result<BTreeMap<String, String>> {
    let m = packages::manifest(false).await?;
    Ok(m.catalog().remove("php").unwrap_or_default().into_iter().collect())
}

pub async fn install(version: &str) -> anyhow::Result<String> {
    packages::install("php", version, None).await
}

/// Use PHP binaries you built yourself (a dir with `php` and `php-fpm`) for a
/// series. They become version `<patch>-local`.
pub fn import(version: &str, from: &std::path::Path) -> anyhow::Result<()> {
    let probe = std::process::Command::new(from.join(platform::exe("php"))).args(["-r", "echo PHP_VERSION;"]).output()
        .with_context(|| format!("no php binary in {}", from.display()))?;
    let patch = String::from_utf8_lossy(&probe.stdout).to_string();
    if !patch.starts_with(&format!("{version}.")) {
        bail!("that binary is PHP {patch}, expected {version}.x");
    }
    let name = format!("{patch}-local");
    let dir = packages::series_dir("php", version);
    let target = dir.join(&name);
    std::fs::create_dir_all(&target)?;
    let bins = if cfg!(windows) { ["php.exe", "php-cgi.exe"] } else { ["php", "php-fpm"] };
    for bin in bins {
        let dest = target.join(bin);
        let _ = std::fs::remove_file(&dest);
        std::fs::copy(from.join(bin), &dest).with_context(|| format!("{bin} not found in {}", from.display()))?;
        download::make_executable(&dest)?;
    }
    std::fs::write(target.join(".pack-package"), format!("{{\"name\":\"php\",\"version\":\"{name}\"}}"))?;
    packages::set_current("php", version, &name)
}

/// Write php.ini, conf.d and php-fpm.conf for a version.
pub fn write_config(version: &str, cfg: &Config) -> anyhow::Result<()> {
    let dir = paths::php_dir(version);
    let confd = dir.join("conf.d");
    std::fs::create_dir_all(&confd)?;
    let ini = &cfg.php_ini;
    let platform_ini = if cfg!(windows) {
        // Official Windows builds load extensions as DLLs from ext/.
        format!(
            "extension_dir = \"{}\"\n",
            platform::slash(&packages::current("php", version).join("ext"))
        )
    } else if cfg!(target_os = "macos") {
        "openssl.cafile = /etc/ssl/cert.pem\ncurl.cainfo = /etc/ssl/cert.pem\n".to_string()
    } else {
        // Static (musl) builds don't know the distro's CA bundle location.
        ["/etc/ssl/certs/ca-certificates.crt", "/etc/pki/tls/certs/ca-bundle.crt", "/etc/ssl/cert.pem"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .map(|p| format!("openssl.cafile = {p}\ncurl.cainfo = {p}\n"))
            .unwrap_or_default()
    };
    std::fs::write(
        dir.join("php.ini"),
        format!(
            "; Generated by Pack — put your own settings in conf.d/99-custom.ini\n\
             memory_limit = {}\n\
             upload_max_filesize = {}\n\
             post_max_size = {}\n\
             max_execution_time = {}\n\
             error_reporting = E_ALL\n\
             display_errors = On\n\
             display_startup_errors = On\n\
             log_errors = On\n\
             variables_order = \"EGPCS\"\n\
             opcache.enable = 1\n\
             opcache.enable_cli = 0\n\
             opcache.validate_timestamps = 1\n\
             opcache.revalidate_freq = 0\n\
             opcache.jit = off\n\
             realpath_cache_size = 4096K\n\
             {platform_ini}",
            ini.memory_limit, ini.upload_max_filesize, ini.post_max_size, ini.max_execution_time
        ),
    )?;
    let custom = confd.join("99-custom.ini");
    if !custom.exists() {
        std::fs::write(custom, "; Your php.ini overrides for this PHP version\n")?;
    }
    if cfg!(windows) {
        return Ok(());
    }
    let tag = version.replace('.', "");
    let prepend = if cfg.dumps {
        format!("php_admin_value[auto_prepend_file] = \"{}\"\n", paths::tools().join("dumps.php").display())
    } else {
        String::new()
    };
    std::fs::write(
        dir.join("php-fpm.conf"),
        format!(
            "[global]\n\
             error_log = \"{log}\"\n\
             daemonize = no\n\
             \n\
             [pack]\n\
             listen = \"{sock}\"\n\
             listen.mode = 0666\n\
             pm = ondemand\n\
             pm.max_children = 20\n\
             pm.process_idle_timeout = 60s\n\
             catch_workers_output = yes\n\
             decorate_workers_output = no\n\
             clear_env = no\n\
             request_terminate_timeout = 0\n\
             php_admin_value[error_log] = \"{log}\"\n\
             {prepend}",
            log = paths::logs().join(format!("php{tag}-fpm.log")).display(),
            sock = paths::php_socket(version).display(),
        ),
    )?;
    Ok(())
}

/// The supervised FastCGI processes for a version: one php-fpm master, or a
/// few php-cgi workers on Windows.
pub fn pool_specs(version: &str, cfg: &Config) -> Vec<Spec> {
    let dir = paths::php_dir(version);
    let mut env = BTreeMap::new();
    env.insert("PHP_INI_SCAN_DIR".into(), scan_dir(version));
    let log = paths::logs().join(format!("php{}-fpm.log", version.replace('.', "")));
    if cfg!(windows) {
        // php-cgi exits after PHP_FCGI_MAX_REQUESTS; the supervisor restarts it.
        env.insert("PHP_FCGI_MAX_REQUESTS".into(), "10000".into());
        return (0..WINDOWS_WORKERS)
            .map(|i| {
                let mut args = vec![
                    "-b".to_string(),
                    format!("127.0.0.1:{}", worker_port(version, i)),
                    "-c".into(),
                    dir.join("php.ini").display().to_string(),
                ];
                if cfg.dumps {
                    args.push("-d".into());
                    args.push(format!("auto_prepend_file={}", platform::slash(&paths::tools().join("dumps.php"))));
                }
                Spec { name: format!("php-{version}-{i}"), program: fpm(version), args, env: env.clone(), cwd: None, log: log.clone() }
            })
            .collect();
    }
    vec![Spec {
        name: format!("php-{version}"),
        program: fpm(version),
        args: vec![
            "--nodaemonize".into(),
            "--fpm-config".into(),
            dir.join("php-fpm.conf").display().to_string(),
            "-c".into(),
            dir.join("php.ini").display().to_string(),
        ],
        env,
        cwd: None,
        log,
    }]
}

/// Is the FastCGI side of a version ready for Caddy?
pub fn pool_ready(version: &str) -> bool {
    if cfg!(windows) {
        std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], worker_port(version, 0)).into(), std::time::Duration::from_millis(100)).is_ok()
    } else {
        paths::php_socket(version).exists()
    }
}

/// Which PHP version the CLI should use in `dir`: a site's isolated version
/// wins, then a `.php-version` file, then the default.
pub fn version_for_dir(cfg: &Config, dir: &std::path::Path) -> String {
    if let Some(site) = crate::sites::for_dir(cfg, dir) {
        return site.php;
    }
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if let Ok(v) = std::fs::read_to_string(d.join(".php-version")) {
            let v = v.trim();
            let minor: Vec<&str> = v.split('.').take(2).collect();
            if minor.len() == 2 {
                return minor.join(".");
            }
        }
        cur = d.parent();
    }
    cfg.php.clone()
}
