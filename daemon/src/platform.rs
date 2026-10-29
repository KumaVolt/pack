//! Everything that differs per operating system: where Pack lives, how
//! `.test` names reach Pack's DNS server, how the HTTPS root is trusted,
//! starting at login, PATH, and process handling.
//!
//! | | macOS | Linux | Windows |
//! |---|---|---|---|
//! | DNS | `/etc/resolver/<tld>` → 127.0.0.1:53535 | systemd-resolved drop-in, `~<tld>` → 127.0.0.1:53535 | NRPT rule `.<tld>` → 127.0.0.1:53 |
//! | Ports 80/443 | allowed on the wildcard address | `ip_unprivileged_port_start=80` | allowed |
//! | HTTPS root | System keychain | system CA store (+ NSS for browsers) | CurrentUser\Root |
//! | Autostart | LaunchAgent | systemd user unit | HKCU Run key |

use crate::config::Config;
#[allow(unused_imports)]
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const LABEL: &str = "dev.pack.daemon";

pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_home() -> PathBuf {
    if cfg!(target_os = "macos") {
        user_home().join("Library/Application Support/Pack")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(user_home).join("Pack")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home().join(".local/share"))
            .join("pack")
    }
}

/// `name` with the platform's executable suffix.
pub fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// `os-arch` key used in the package manifest.
pub fn target() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "x86_64" };
    (os, arch)
}

/// Windows' NRPT can only point at port 53; elsewhere stay off the privileged port.
pub fn dns_port(cfg: &Config) -> u16 {
    if cfg!(windows) { 53 } else { cfg.dns_port }
}

/// Paths as Caddy, PHP and MSYS programs like them (forward slashes on Windows).
pub fn slash(path: &Path) -> String {
    let s = path.display().to_string();
    if cfg!(windows) { s.replace('\\', "/") } else { s }
}

// ------------------------------------------------------------------ checks
//
// Some checks shell out (PowerShell, certutil), so results are cached briefly:
// the desktop app polls status every couple of seconds.

static CHECKS: Mutex<Vec<(String, Instant, bool)>> = Mutex::new(Vec::new());

fn cached(key: &str, f: impl FnOnce() -> bool) -> bool {
    {
        let checks = CHECKS.lock().unwrap();
        if let Some((_, at, v)) = checks.iter().find(|(k, _, _)| k == key) {
            if at.elapsed() < Duration::from_secs(20) {
                return *v;
            }
        }
    }
    let v = f();
    let mut checks = CHECKS.lock().unwrap();
    checks.retain(|(k, _, _)| k != key);
    checks.push((key.to_string(), Instant::now(), v));
    v
}

pub fn forget_checks() {
    CHECKS.lock().unwrap().clear();
}

fn output_ok(cmd: &mut Command) -> Option<String> {
    no_window(cmd);
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "macos")]
fn resolver_file(cfg: &Config) -> PathBuf {
    PathBuf::from(format!("/etc/resolver/{}", cfg.tld))
}

#[cfg(target_os = "macos")]
fn resolver_contents(cfg: &Config) -> String {
    format!("# Added by Pack\nnameserver 127.0.0.1\nport {}\n", cfg.dns_port)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn resolved_file() -> PathBuf {
    PathBuf::from("/etc/systemd/resolved.conf.d/pack.conf")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn resolved_contents(cfg: &Config) -> String {
    format!("# Added by Pack\n[Resolve]\nDNS=127.0.0.1:{}\nDomains=~{}\n", cfg.dns_port, cfg.tld)
}

/// Do `<anything>.<tld>` lookups reach Pack?
pub fn dns_ok(cfg: &Config) -> bool {
    #[cfg(target_os = "macos")]
    {
        std::fs::read_to_string(resolver_file(cfg)).is_ok_and(|s| s == resolver_contents(cfg))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::fs::read_to_string(resolved_file()).is_ok_and(|s| s == resolved_contents(cfg))
    }
    #[cfg(windows)]
    {
        let tld = cfg.tld.clone();
        cached(&format!("nrpt-{tld}"), || {
            output_ok(Command::new("powershell").args([
                "-NoProfile",
                "-Command",
                &format!("if (Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -eq '.{tld}' }}) {{ 'yes' }}"),
            ]))
            .is_some_and(|o| o.contains("yes"))
        })
    }
}

/// Can Caddy bind ports 80/443 without root?
pub fn low_ports_ok() -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::fs::read_to_string("/proc/sys/net/ipv4/ip_unprivileged_port_start")
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .is_some_and(|p| p <= 80)
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        true
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
const LINUX_CA_PATHS: &[&str] = &[
    "/usr/local/share/ca-certificates/pack-local-ca.crt",
    "/etc/pki/ca-trust/source/anchors/pack-local-ca.crt",
    "/etc/ca-certificates/trust-source/anchors/pack-local-ca.crt",
];

pub fn ca_trusted(cert: &Path) -> bool {
    let Ok(pem) = std::fs::read(cert) else { return false };
    #[cfg(target_os = "macos")]
    {
        let _ = pem;
        cached("ca", || output_ok(Command::new("/usr/bin/security").arg("verify-cert").arg("-c").arg(cert)).is_some())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        LINUX_CA_PATHS.iter().any(|p| std::fs::read(p).is_ok_and(|c| c == pem))
    }
    #[cfg(windows)]
    {
        let _ = pem;
        let cert = cert.to_path_buf();
        cached("ca", || {
            output_ok(Command::new("certutil").args(["-user", "-verify"]).arg(&cert))
                .is_some_and(|o| !o.contains("CERT_TRUST_IS_UNTRUSTED_ROOT"))
        })
    }
}

// ------------------------------------------------------------------ privileged setup

/// The one-time admin step: route `.<tld>` to Pack, allow ports 80/443 and
/// trust the HTTPS root. `None` when nothing needs admin rights.
pub fn privileged_script(cfg: &Config, ca: Option<&Path>) -> Option<String> {
    let mut steps: Vec<String> = vec![];
    #[cfg(target_os = "macos")]
    {
        if !dns_ok(cfg) {
            steps.push(format!(
                "mkdir -p /etc/resolver && printf '%s' '{}' > {}",
                resolver_contents(cfg),
                resolver_file(cfg).display()
            ));
        }
        if let Some(ca) = ca {
            steps.push(format!(
                "security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '{}'",
                ca.display()
            ));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if !dns_ok(cfg) {
            steps.push(format!(
                "mkdir -p /etc/systemd/resolved.conf.d && printf '%s' '{}' > {} && (systemctl restart systemd-resolved || true)",
                resolved_contents(cfg),
                resolved_file().display()
            ));
        }
        if !low_ports_ok() {
            steps.push(
                "printf 'net.ipv4.ip_unprivileged_port_start=80\\n' > /etc/sysctl.d/50-pack.conf && sysctl -w net.ipv4.ip_unprivileged_port_start=80"
                    .into(),
            );
        }
        if let Some(ca) = ca {
            let ca = ca.display();
            steps.push(format!(
                "if [ -d /usr/local/share/ca-certificates ]; then cp '{ca}' {0} && update-ca-certificates; \
                 elif [ -d /etc/pki/ca-trust/source/anchors ]; then cp '{ca}' {1} && update-ca-trust; \
                 elif [ -d /etc/ca-certificates/trust-source/anchors ]; then cp '{ca}' {2} && trust extract-compat; fi",
                LINUX_CA_PATHS[0], LINUX_CA_PATHS[1], LINUX_CA_PATHS[2]
            ));
        }
    }
    #[cfg(windows)]
    {
        let _ = ca; // trusted per user, without admin: see trust_ca_user
        if !dns_ok(cfg) {
            steps.push(format!(
                "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq 'Pack' }} | Remove-DnsClientNrptRule -Force; \
                 Add-DnsClientNrptRule -Namespace '.{}' -NameServers '127.0.0.1' -Comment 'Pack'; Clear-DnsClientCache",
                cfg.tld
            ));
        }
    }
    let sep = if cfg!(windows) { "; " } else { " && " };
    (!steps.is_empty()).then(|| steps.join(sep))
}

/// Undo the admin step (used by `pack uninstall`).
pub fn uninstall_script(cfg: &Config) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        dns_ok(cfg).then(|| format!("rm -f {}", resolver_file(cfg).display()))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = cfg;
        Some(format!(
            "rm -f {} /etc/sysctl.d/50-pack.conf && (systemctl restart systemd-resolved || true)",
            resolved_file().display()
        ))
    }
    #[cfg(windows)]
    {
        let _ = cfg;
        Some("Get-DnsClientNrptRule | Where-Object { $_.Comment -eq 'Pack' } | Remove-DnsClientNrptRule -Force".into())
    }
}

/// Run an admin script with the OS's password/consent dialog (from the app).
pub async fn run_privileged_gui(script: &str) -> anyhow::Result<()> {
    let out = {
        #[cfg(target_os = "macos")]
        {
            let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
            let apple = format!(
                "do shell script \"{escaped}\" with administrator privileges with prompt \"Pack needs to set up .test domains and HTTPS.\""
            );
            tokio::process::Command::new("/usr/bin/osascript").arg("-e").arg(apple).output().await?
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            tokio::process::Command::new("pkexec").args(["/bin/sh", "-c", script]).output().await.context("pkexec is required")?
        }
        #[cfg(windows)]
        {
            elevated_powershell(script).await?
        }
    };
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    forget_checks();
    Ok(())
}

/// Run an admin script from a terminal (used by `pack install`).
pub fn run_privileged_tty(script: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let status = Command::new("sudo").args(["/bin/sh", "-c", script]).status()?;
        if !status.success() {
            bail!("sudo failed");
        }
    }
    #[cfg(windows)]
    {
        let rt = tokio::runtime::Handle::try_current();
        let out = match rt {
            Ok(h) => tokio::task::block_in_place(|| h.block_on(elevated_powershell(script)))?,
            Err(_) => tokio::runtime::Runtime::new()?.block_on(elevated_powershell(script))?,
        };
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
    }
    forget_checks();
    Ok(())
}

#[cfg(windows)]
async fn elevated_powershell(script: &str) -> anyhow::Result<std::process::Output> {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let launcher = format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-EncodedCommand','{encoded}'; exit $p.ExitCode"
    );
    Ok(tokio::process::Command::new("powershell").args(["-NoProfile", "-Command", &launcher]).output().await?)
}

/// Per-user trust that needs no admin rights: the Windows user root store
/// (shows a confirmation dialog) and, on Linux, the NSS database browsers use.
pub fn trust_ca_user(cert: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let ok = output_ok(Command::new("certutil").args(["-user", "-addstore", "-f", "Root"]).arg(cert));
        forget_checks();
        if ok.is_none() {
            bail!("adding the Pack root certificate to your certificate store was cancelled");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let nssdb = user_home().join(".pki/nssdb");
        if nssdb.exists() {
            let _ = output_ok(
                Command::new("certutil")
                    .arg("-d")
                    .arg(format!("sql:{}", nssdb.display()))
                    .args(["-A", "-t", "C,,", "-n", "Pack Local CA", "-i"])
                    .arg(cert),
            );
        }
    }
    let _ = cert;
    Ok(())
}

// ------------------------------------------------------------------ autostart

pub fn autostart_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        user_home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
    } else if cfg!(windows) {
        PathBuf::from(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Pack")
    } else {
        user_home().join(".config/systemd/user/pack.service")
    }
}

pub fn autostart_installed() -> bool {
    if cfg!(windows) {
        output_ok(Command::new("reg").args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Pack"])).is_some()
    } else {
        autostart_path().exists()
    }
}

/// Start the daemon at login (and now).
pub fn install_autostart(pack: &Path, log: &Path) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key><array><string>{}</string><string>daemon</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#,
            pack.display(),
            log.display(),
            log.display()
        );
        let path = autostart_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, plist)?;
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
        let out = Command::new("launchctl").args(["bootstrap", &domain]).arg(&path).output()?;
        if !out.status.success() {
            bail!("launchctl bootstrap failed: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let unit = format!(
            "[Unit]\nDescription=Pack local development environment\nAfter=network.target\n\n\
             [Service]\nExecStart={} daemon\nRestart=always\nStandardOutput=append:{}\nStandardError=append:{}\n\n\
             [Install]\nWantedBy=default.target\n",
            pack.display(),
            log.display(),
            log.display()
        );
        let path = autostart_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, unit)?;
        let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
        let st = Command::new("systemctl").args(["--user", "enable", "--now", "pack.service"]).status()?;
        if !st.success() {
            bail!("systemctl --user enable pack failed");
        }
    }
    #[cfg(windows)]
    {
        let _ = log;
        // `conhost --headless` keeps a console program from opening a window.
        let value = format!("conhost.exe --headless \"{}\" daemon", pack.display());
        let ok = output_ok(Command::new("reg").args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "Pack",
            "/t",
            "REG_SZ",
            "/d",
            &value,
            "/f",
        ]));
        if ok.is_none() {
            bail!("could not add Pack to the Run key");
        }
        let mut cmd = Command::new(pack);
        cmd.arg("daemon");
        detach(&mut cmd);
        cmd.spawn()?;
    }
    Ok(())
}

pub fn remove_autostart() {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("launchctl").args(["bootout", &format!("gui/{}/{LABEL}", unsafe { libc::getuid() })]).output();
        let _ = std::fs::remove_file(autostart_path());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("systemctl").args(["--user", "disable", "--now", "pack.service"]).status();
        let _ = std::fs::remove_file(autostart_path());
    }
    #[cfg(windows)]
    {
        let _ = output_ok(Command::new("reg").args(["delete", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "Pack", "/f"]));
    }
}

/// Start (or restart) the autostarted daemon.
pub fn kick_autostart(restart: bool) {
    #[cfg(target_os = "macos")]
    {
        let svc = format!("gui/{}/{LABEL}", unsafe { libc::getuid() });
        let mut args = vec!["kickstart"];
        if restart {
            args.push("-k");
        }
        args.push(&svc);
        let _ = Command::new("launchctl").args(&args).output();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = Command::new("systemctl").args(["--user", if restart { "restart" } else { "start" }, "pack.service"]).status();
    }
    #[cfg(windows)]
    {
        let _ = restart; // the caller spawns the daemon directly on Windows
    }
}

// ------------------------------------------------------------------ PATH

/// Put Pack's bin dir on the user's PATH (idempotent). True if it changed.
pub fn add_to_path(bin: &Path) -> anyhow::Result<bool> {
    #[cfg(windows)]
    {
        let bin = bin.display().to_string();
        let current = output_ok(Command::new("powershell").args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','User')",
        ]))
        .unwrap_or_default();
        if current.split(';').any(|p| p.trim().eq_ignore_ascii_case(&bin)) {
            return Ok(false);
        }
        let script = format!("[Environment]::SetEnvironmentVariable('Path', '{bin};' + [Environment]::GetEnvironmentVariable('Path','User'), 'User')");
        output_ok(Command::new("powershell").args(["-NoProfile", "-Command", &script])).context("updating PATH")?;
        Ok(true)
    }
    #[cfg(unix)]
    {
        let line = format!("export PATH=\"{}:$PATH\" # Pack", bin.display());
        let mut rcs = vec![".zshrc", ".bashrc"];
        if cfg!(not(target_os = "macos")) {
            rcs.push(".profile");
        }
        let mut changed = false;
        for rc in rcs {
            let path = user_home().join(rc);
            if !path.exists() && rc != ".zshrc" {
                continue;
            }
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            if current.contains(&line) {
                continue;
            }
            let mut next = current;
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(&format!("\n{line}\n"));
            std::fs::write(path, next)?;
            changed = true;
        }
        Ok(changed)
    }
}

pub fn on_path(bin: &Path) -> bool {
    let sep = if cfg!(windows) { ';' } else { ':' };
    std::env::var("PATH").unwrap_or_default().split(sep).any(|p| Path::new(p) == bin)
}

/// Open a URL in the default browser.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    no_window(&mut cmd);
    cmd.spawn().context("opening the browser")?;
    Ok(())
}

// ------------------------------------------------------------------ processes

/// Detach a background process from the terminal: own session on Unix, no
/// console window on Windows.
pub fn detach(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}

/// Never flash a console window for helper processes on Windows.
pub fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

pub fn no_window_tokio(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

/// Ask a process to stop gracefully (SIGTERM); on Windows there is no
/// equivalent for console-less processes, so the caller falls back to kill.
pub fn terminate(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, libc::SIGTERM) == 0 }
    }
    #[cfg(windows)]
    {
        let _ = pid;
        false
    }
}

/// Replace the current process with `cmd` (Unix) or run it and exit with its
/// status (Windows, which has no exec).
pub fn exec(mut cmd: Command) -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        eprintln!("could not run {:?}: {err}", cmd.get_program());
        std::process::exit(127);
    }
    #[cfg(windows)]
    {
        match cmd.status() {
            Ok(st) => std::process::exit(st.code().unwrap_or(1)),
            Err(e) => {
                eprintln!("could not run {:?}: {e}", cmd.get_program());
                std::process::exit(127);
            }
        }
    }
}
