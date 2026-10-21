#!/usr/bin/env python3
"""Pack's package factory.

Every binary Pack installs (PHP, MySQL, Postgres, Valkey, ...) is built or
repackaged here and published as a GitHub release asset of the Pack repo.
`dist/manifest.json` lists them with SHA-256 so the app installs and updates
from one trusted place.

    packages.py plan  [--manifest M] [--only php,mysql]   # JSON build matrix of what's missing
    packages.py build NAME SERIES TARGET [--version V] --out DIR   # TARGET: macos-arm64, linux-x86_64, windows-x86_64, any
    packages.py merge --manifest M --dir DIR --base-url URL

Upstream discovery avoids the GitHub REST API (rate limited): it uses the
/releases/latest redirect and git's smart-HTTP ref listing instead.
Stdlib only.
"""

import argparse
import datetime as dt
import functools
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

UA = {"User-Agent": "pack-packages"}
TARGETS = ["macos-arm64", "macos-x86_64", "linux-x86_64", "linux-arm64", "windows-x86_64"]
# Runners for packages that must be compiled on the platform itself.
NATIVE_RUNNER = {
    "macos-arm64": "macos-15",
    "macos-x86_64": "macos-15-intel",
    "linux-x86_64": "ubuntu-22.04",
    "linux-arm64": "ubuntu-22.04-arm",
    "windows-x86_64": "windows-latest",
}


class T:
    """A build target, e.g. T("linux-arm64")."""

    def __init__(self, target):
        self.name = target
        self.os, _, self.arch = target.partition("-")

    def exe(self, name):
        return name + ".exe" if self.os == "windows" else name

    # Vendor spellings of the architecture.
    @property
    def go(self):  # arm64 / amd64
        return "arm64" if self.arch == "arm64" else "amd64"

    @property
    def gnu(self):  # aarch64 / x86_64
        return "aarch64" if self.arch == "arm64" else "x86_64"

# Extensions compiled into Pack's PHP — what Laravel apps need.
PHP_EXTENSIONS = (
    "bcmath,bz2,calendar,ctype,curl,dba,dom,exif,fileinfo,filter,ftp,gd,gmp,iconv,igbinary,intl,"
    "mbstring,mysqli,mysqlnd,opcache,openssl,pcntl,pdo,pdo_mysql,pdo_pgsql,pdo_sqlite,pgsql,phar,"
    "posix,readline,redis,session,shmop,simplexml,soap,sockets,sodium,sqlite3,sysvmsg,sysvsem,"
    "sysvshm,tokenizer,xml,xmlreader,xmlwriter,xsl,zip,zlib,zstd"
)


# ------------------------------------------------------------------ http

def get(url, method="GET"):
    req = urllib.request.Request(url, headers=UA, method=method)
    return urllib.request.urlopen(req, timeout=60)


@functools.lru_cache(maxsize=None)
def text(url):
    with get(url) as r:
        return r.read().decode()


def exists(url):
    try:
        with get(url, "HEAD") as r:
            return r.status == 200
    except urllib.error.HTTPError:
        return False


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


@functools.lru_cache(maxsize=None)
def latest_tag(repo):
    opener = urllib.request.build_opener(NoRedirect)
    try:
        opener.open(urllib.request.Request(f"https://github.com/{repo}/releases/latest", headers=UA, method="HEAD"))
    except urllib.error.HTTPError as e:
        loc = e.headers.get("Location", "")
        if "/tag/" in loc:
            return loc.rsplit("/tag/", 1)[1]
    raise SystemExit(f"no latest release for {repo}")


@functools.lru_cache(maxsize=None)
def git_tags(repo):
    body = text(f"https://github.com/{repo}.git/info/refs?service=git-upload-pack")
    return sorted(set(re.findall(r"refs/tags/([^\s^\x00{}]+)", body)))


def vkey(v):
    return [int(x) for x in re.findall(r"\d+", v)]


def newest(versions):
    return max(versions, key=vkey) if versions else None


def download(url, dest):
    # curl copes with cookie-based redirect chains that urllib loops on.
    run(["curl", "-fsSL", "--retry", "3", "-A", UA["User-Agent"], "-o", str(dest), url])
    return dest


def run(cmd, **kw):
    print(f"  $ {' '.join(map(str, cmd))}", file=sys.stderr)
    subprocess.run(cmd, check=True, **kw)


def untar(archive, into, strip=0):
    os.makedirs(into, exist_ok=True)
    if str(archive).endswith(".zip"):
        # GNU tar (CI's ubuntu runners) can't read zips, and unzip can't strip.
        tmp = Path(tempfile.mkdtemp())
        run(["unzip", "-q", "-o", str(archive), "-d", str(tmp)])
        src = tmp
        for _ in range(strip):
            kids = list(src.iterdir())
            if len(kids) != 1 or not kids[0].is_dir():
                raise SystemExit(f"can't strip {archive}: expected one top-level directory")
            src = kids[0]
        for child in src.iterdir():
            shutil.move(str(child), str(Path(into) / child.name))
        return
    cmd = ["tar", "-xf", str(archive), "-C", str(into)]
    if strip:
        cmd.append(f"--strip-components={strip}")
    run(cmd)


# ------------------------------------------------------------------ packages

# Each package: series (list or function), latest(series, t) -> version or
# None when not published for that target, targets (default: all), native
# (must be built on the target's own runner, otherwise repackaged on
# ubuntu), and build(series, version, t, root) which fills `root`.

def php_latest(series, t):
    data = json.loads(text(f"https://www.php.net/releases/?json&version={series}"))
    return data["version"]


def spc_binary(work, t):
    spc = work / ("spc.exe" if t.os == "windows" else "spc")
    if not spc.exists():
        tag = latest_tag("crazywhalecc/static-php-cli")
        archive = download(f"https://github.com/crazywhalecc/static-php-cli/releases/download/{tag}/spc-{t.os}-{t.gnu}.tar.gz", work / "spc.tar.gz")
        untar(archive, work)
        spc.chmod(0o755)
    return spc


def php_build(series, version, t, root):
    if t.os == "windows":
        return php_build_windows(series, version, t, root)
    # macOS and Linux: fully static builds with static-php-cli (musl on Linux).
    work = Path(os.environ.get("SPC_WORKDIR", Path.home() / ".cache/pack-spc"))
    work.mkdir(parents=True, exist_ok=True)
    spc = spc_binary(work, t)
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="13.0")
    run([spc, "doctor", "--auto-fix"], cwd=work, env=env)
    # Always fetch php-src for this exact version (spc caches by name, not
    # version). Source mirrors (ftp.gnu.org, dl.static-php.dev) are flaky, so
    # retry, and drop the prebuilt-library shortcut on the last attempt.
    for attempt in range(3):
        cmd = [spc, "download", f"--for-extensions={PHP_EXTENSIONS}", f"--with-php={version}", "--ignore-cache-sources=php-src", "--retry=3"]
        if attempt < 2:
            cmd.append("--prefer-pre-built")
        try:
            run(cmd, cwd=work, env=env)
            break
        except subprocess.CalledProcessError:
            if attempt == 2:
                raise
            time.sleep(30)
    shutil.rmtree(work / "buildroot", ignore_errors=True)
    shutil.rmtree(work / "source/php-src", ignore_errors=True)
    # spc's post-build sanity check trips over the prebuilt SQLite's compile
    # options; the binaries are fine, so verify them ourselves below.
    subprocess.run([spc, "build", PHP_EXTENSIONS, "--build-cli", "--build-fpm"], cwd=work, env=env)
    for b in ["php", "php-fpm"]:
        src = work / "buildroot/bin" / b
        if not src.exists():
            raise SystemExit(f"spc did not produce {b}")
        shutil.copy2(src, root / b)
    verify_php(root / "php", version, ["intl", "pdo_sqlite", "pdo_mysql", "pdo_pgsql", "redis", "sodium", "zend opcache"])


def verify_php(php, version, required, args=()):
    if not os.access(php, os.X_OK) or (sys.platform == "win32") != str(php).endswith(".exe"):
        print(f"  (can't run {php} on this host; skipping verification)", file=sys.stderr)
        return
    out = subprocess.run([php, *args, "-r", "echo PHP_VERSION;"], capture_output=True, text=True, check=True).stdout
    if out != version:
        raise SystemExit(f"built PHP {out}, expected {version}")
    mods = subprocess.run([php, *args, "-m"], capture_output=True, text=True).stdout.lower()
    for ext in required:
        if ext not in mods:
            raise SystemExit(f"PHP build is missing {ext}")


# Windows: the official windows.php.net NTS builds (FastCGI via php-cgi.exe)
# plus PECL redis/igbinary DLLs. Extensions are enabled through conf.d.
WINDOWS_EXTENSIONS = [
    "bz2", "curl", "exif", "fileinfo", "ftp", "gd", "gmp", "intl", "mbstring", "mysqli", "openssl",
    "pdo_mysql", "pdo_pgsql", "pdo_sqlite", "pgsql", "soap", "sockets", "sodium", "sqlite3", "xsl", "zip",
]


def php_windows_url(version):
    vs = "vs17" if vkey(version) >= [8, 4] else "vs16"
    name = f"php-{version}-nts-Win32-{vs}-x64.zip"
    for folder in ["releases", "releases/archives"]:
        url = f"https://windows.php.net/downloads/{folder}/{name}"
        if exists(url):
            return url, vs
    raise SystemExit(f"no Windows build for PHP {version}")


def pecl_dll(ext, series, vs):
    """Newest PECL Windows DLL of `ext` for a PHP series, or None."""
    listing = text(f"https://downloads.php.net/~windows/pecl/releases/{ext}/")
    for v in sorted(set(re.findall(r'href="(\d+\.\d+\.\d+)/"', listing)), key=vkey, reverse=True)[:4]:
        url = f"https://downloads.php.net/~windows/pecl/releases/{ext}/{v}/php_{ext}-{v}-{series}-nts-{vs}-x64.zip"
        if exists(url):
            return url
    return None


def php_build_windows(series, version, t, root):
    url, vs = php_windows_url(version)
    tmp = Path(tempfile.mkdtemp())
    untar(download(url, tmp / "php.zip"), root)
    ini = [f"extension={e}" for e in WINDOWS_EXTENSIONS if (root / "ext" / f"php_{e}.dll").exists()]
    if (root / "ext/php_opcache.dll").exists():
        ini.append("zend_extension=opcache")
    for ext in ["igbinary", "redis"]:  # redis needs igbinary loaded first
        dll_zip = pecl_dll(ext, series, vs)
        if dll_zip:
            d = tmp / ext
            untar(download(dll_zip, tmp / f"{ext}.zip"), d)
            shutil.copy2(d / f"php_{ext}.dll", root / "ext" / f"php_{ext}.dll")
            ini.append(f"extension={ext}")
        else:
            print(f"  no Windows DLL for {ext} on PHP {series}", file=sys.stderr)
    (root / "conf.d").mkdir(exist_ok=True)
    (root / "conf.d/10-pack-extensions.ini").write_text("; Enabled by Pack\n" + "\n".join(ini) + "\n")
    for junk in ["php.ini-development", "php.ini-production", "dev", "extras", "lib"]:
        p = root / junk
        shutil.rmtree(p, ignore_errors=True) if p.is_dir() else p.unlink(missing_ok=True)
    if not (root / "php-cgi.exe").exists():
        raise SystemExit("php-cgi.exe missing from the Windows build")


def newest_published(tags, url_fn):
    """Newest tag whose download actually exists (tags can precede binaries)."""
    for v in sorted(tags, key=vkey, reverse=True)[:6]:
        if exists(url_fn(v)):
            return v
    return None


def mysql_url(version, t):
    folder = ".".join(version.split(".")[:2])
    base = f"https://cdn.mysql.com/Downloads/MySQL-{folder}/mysql-{version}"
    if t.os == "macos":
        return f"{base}-macos15-{t.arch}.tar.gz"
    if t.os == "linux":
        return f"{base}-linux-glibc2.28-{t.gnu}.tar.xz"
    return f"{base}-winx64.zip"


def mysql_latest(series, t):
    # Oracle doesn't publish every patch for every platform, so resolve per target.
    tags = [x[len("mysql-"):] for x in git_tags("mysql/mysql-server") if x.startswith(f"mysql-{series}.")]
    return newest_published(tags, lambda v: mysql_url(v, t))


def mysql_build(series, version, t, root):
    tmp = Path(tempfile.mkdtemp())
    url = mysql_url(version, t)
    untar(download(url, tmp / url.rsplit("/", 1)[1]), root, strip=1)
    # Trim what a dev box never needs (~60% of the archive).
    for junk in ["mysql-test", "docs", "man", "include", "lib/plugin/debug", "bin/mysqld-debug", "bin/mysqld-debug.exe", "lib/debug"]:
        p = root / junk
        shutil.rmtree(p, ignore_errors=True) if p.is_dir() else p.unlink(missing_ok=True)
    for pdb in root.rglob("*.pdb"):
        pdb.unlink()


PG_TRIPLE = {
    "macos-arm64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin",
    "linux-x86_64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu",
    "windows-x86_64": "x86_64-pc-windows-msvc",
}


def postgres_latest(series, t):
    tags = [x for x in git_tags("theseus-rs/postgresql-binaries") if x.split(".")[0] == series and re.fullmatch(r"[\d.]+", x)]
    return newest(tags)


def postgres_build(series, version, t, root):
    url = f"https://github.com/theseus-rs/postgresql-binaries/releases/download/{version}/postgresql-{version}-{PG_TRIPLE[t.name]}.tar.gz"
    tmp = Path(tempfile.mkdtemp())
    untar(download(url, tmp / "pg.tar.gz"), root, strip=1)
    shutil.rmtree(root / "include", ignore_errors=True)


def redis_latest(series, t):
    if t.os == "windows":
        # No native Valkey for Windows: use the redis-windows builds (Redis 8, AGPL).
        return latest_tag("redis-windows/redis-windows") if series == "8" else None
    return newest([x for x in git_tags("valkey-io/valkey") if re.fullmatch(rf"{series}\.\d+\.\d+", x)])


def redis_build(series, version, t, root):
    tmp = Path(tempfile.mkdtemp())
    if t.os == "windows":
        url = f"https://github.com/redis-windows/redis-windows/releases/download/{version}/Redis-{version}-Windows-x64-msys2.zip"
        untar(download(url, tmp / "r.zip"), tmp / "x")
        src = next(p.parent for p in (tmp / "x").rglob("redis-server.exe"))
        for f in src.iterdir():
            if f.suffix in (".exe", ".dll"):
                # Same binary names on every OS keeps the daemon simple.
                dest = f.name.replace("redis-", "valkey-") if f.suffix == ".exe" else f.name
                shutil.copy2(f, root / dest)
        return
    if t.os == "linux":
        # Official Valkey builds for Ubuntu 22.04 (glibc 2.35+).
        url = f"https://download.valkey.io/releases/valkey-{version}-jammy-{'arm64' if t.arch == 'arm64' else 'x86_64'}.tar.gz"
        untar(download(url, tmp / "v.tar.gz"), tmp / "x", strip=1)
        for b in ["valkey-server", "valkey-cli", "valkey-benchmark"]:
            shutil.copy2(next((tmp / "x").rglob(b)), root / b)
        return
    # macOS: no upstream binaries, compile (needs the macOS runner).
    src = tmp / "src"
    untar(download(f"https://github.com/valkey-io/valkey/archive/refs/tags/{version}.tar.gz", tmp / "v.tar.gz"), src, strip=1)
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="13.0")
    run(["make", f"-j{os.cpu_count() or 4}", "BUILD_TLS=no"], cwd=src, env=env)
    for b in ["valkey-server", "valkey-cli", "valkey-benchmark"]:
        shutil.copy2(src / "src" / b, root / b)


def gh_latest(repo):
    return lambda series, t: latest_tag(repo).lstrip("v")


def single_binary(url_fn, name):
    """Package one executable from a raw binary or an archive. url_fn(version, t)
    returns the URL, or None when the target isn't published."""
    def build(series, version, t, root):
        tmp = Path(tempfile.mkdtemp())
        url = url_fn(version, t)
        f = download(url, tmp / url.rsplit("/", 1)[1])
        exe = t.exe(name)
        if url.endswith((".tar.gz", ".tgz", ".zip")):
            untar(f, tmp / "x")
            hits = [p for p in (tmp / "x").rglob(exe) if p.is_file()]
            if not hits:
                raise SystemExit(f"{exe} not found in {url}")
            shutil.copy2(hits[0], root / exe)
        else:
            shutil.copy2(f, root / exe)
        (root / exe).chmod(0o755)
    return build


def published(url_fn, repo=None):
    """latest() for single_binary packages: newest release, if this target has an asset."""
    def latest(series, t):
        version = latest_tag(repo).lstrip("v")
        url = url_fn(version, t)
        return version if url and exists(url) else None
    return latest


def meili_url(v, t):
    asset = {"macos": "macos-apple-silicon" if t.arch == "arm64" else "macos-amd64",
             "linux": f"linux-{'aarch64' if t.arch == 'arm64' else 'amd64'}",
             "windows": "windows-amd64.exe"}[t.os]
    return f"https://github.com/meilisearch/meilisearch/releases/download/v{v}/meilisearch-{asset}"


def typesense_url(v, t):
    if t.os == "windows":
        return None  # Typesense has no Windows build
    return f"https://dl.typesense.org/releases/{v}/typesense-server-{v}-{'darwin' if t.os == 'macos' else 'linux'}-{t.go}.tar.gz"


def mailpit_url(v, t):
    os_ = {"macos": "darwin", "linux": "linux", "windows": "windows"}[t.os]
    ext = "zip" if t.os == "windows" else "tar.gz"
    return f"https://github.com/axllent/mailpit/releases/download/v{v}/mailpit-{os_}-{t.go}.{ext}"


def rustfs_url(v, t):
    if t.os == "macos":
        return f"https://github.com/rustfs/rustfs/releases/download/{v}/rustfs-macos-{t.gnu}-v{v}.zip" if t.arch == "arm64" else None
    if t.os == "linux":
        return f"https://github.com/rustfs/rustfs/releases/download/{v}/rustfs-linux-{t.gnu}-musl-v{v}.zip"
    return f"https://github.com/rustfs/rustfs/releases/download/{v}/rustfs-windows-x86_64-v{v}.zip"


def caddy_url(v, t):
    os_ = {"macos": "mac", "linux": "linux", "windows": "windows"}[t.os]
    ext = "zip" if t.os == "windows" else "tar.gz"
    return f"https://github.com/caddyserver/caddy/releases/download/v{v}/caddy_{v}_{os_}_{t.go}.{ext}"


def cloudflared_url(v, t):
    base = f"https://github.com/cloudflare/cloudflared/releases/download/{v}/cloudflared"
    return {"macos": f"{base}-darwin-{t.go}.tgz", "linux": f"{base}-linux-{t.go}", "windows": f"{base}-windows-amd64.exe"}[t.os]


def mongodb_url(version, t):
    if t.os == "macos":
        return f"https://fastdl.mongodb.org/osx/mongodb-macos-{t.arch}-{version}.tgz"
    if t.os == "linux":
        return f"https://fastdl.mongodb.org/linux/mongodb-linux-{t.gnu}-ubuntu2204-{version}.tgz"
    return f"https://fastdl.mongodb.org/windows/mongodb-windows-x86_64-{version}.zip"


def mongodb_latest(series, t):
    tags = [x[1:] for x in git_tags("mongodb/mongo") if re.fullmatch(rf"r{re.escape(series)}\.\d+", x)]
    return newest_published(tags, lambda v: mongodb_url(v, t))


def mongodb_build(series, version, t, root):
    tmp = Path(tempfile.mkdtemp())
    url = mongodb_url(version, t)
    untar(download(url, tmp / url.rsplit("/", 1)[1]), root, strip=1)
    for pdb in root.rglob("*.pdb"):
        pdb.unlink()


def composer_latest(series, t):
    return json.loads(text("https://getcomposer.org/versions"))["stable"][0]["version"]


def composer_build(series, version, t, root):
    download(f"https://getcomposer.org/download/{version}/composer.phar", root / "composer.phar")


def major(version):
    return version.split(".")[0]


def native_php(t):
    # Windows PHP is a repackaged official build; macOS/Linux compile on their own runner.
    return t.os != "windows"


def native_redis(t):
    return t.os == "macos"


PACKAGES = {
    # 8.1 is end-of-life and no longer compiles with current Xcode.
    "php": dict(series=["8.5", "8.4", "8.3", "8.2"], latest=php_latest, build=php_build, native=native_php),
    "mysql": dict(series=["8.4", "9.7"], latest=mysql_latest, build=mysql_build),
    "postgres": dict(series=["18", "17", "16", "15", "14"], latest=postgres_latest, build=postgres_build),
    "redis": dict(series=["9", "8"], latest=redis_latest, build=redis_build, native=native_redis),
    "meilisearch": dict(series=["1"], latest=published(meili_url, "meilisearch/meilisearch"), build=single_binary(meili_url, "meilisearch")),
    "typesense": dict(
        series=lambda: [major(latest_tag("typesense/typesense").lstrip("v"))],
        latest=published(typesense_url, "typesense/typesense"),
        build=single_binary(typesense_url, "typesense-server"),
    ),
    "mail": dict(series=["1"], latest=published(mailpit_url, "axllent/mailpit"), build=single_binary(mailpit_url, "mailpit")),
    "s3": dict(series=["1"], latest=published(rustfs_url, "rustfs/rustfs"), build=single_binary(rustfs_url, "rustfs")),
    "mongodb": dict(series=["8.0", "7.0"], latest=mongodb_latest, build=mongodb_build),
    "caddy": dict(series=["2"], latest=published(caddy_url, "caddyserver/caddy"), build=single_binary(caddy_url, "caddy")),
    "cloudflared": dict(series=["stable"], latest=published(cloudflared_url, "cloudflare/cloudflared"), build=single_binary(cloudflared_url, "cloudflared")),
    "composer": dict(series=["2"], latest=composer_latest, build=composer_build, targets=["any"]),
}


def series_of(name):
    s = PACKAGES[name]["series"]
    return s() if callable(s) else s


def targets_of(name):
    return PACKAGES[name].get("targets", TARGETS)


def is_native(name, t):
    n = PACKAGES[name].get("native")
    return bool(n(t)) if callable(n) else bool(n)


# ------------------------------------------------------------------ commands

def load_manifest(path):
    if path and Path(path).exists():
        return json.loads(Path(path).read_text())
    return {"schema": 1, "packages": []}


def entry_target(p):
    """Manifest entries before Windows/Linux support had no `os` (= macOS)."""
    if p["arch"] == "any":
        return "any"
    return f"{p.get('os', 'macos')}-{p['arch']}"


def cmd_plan(args):
    manifest = load_manifest(args.manifest)
    have = {(p["name"], p["version"], entry_target(p)) for p in manifest["packages"]}
    only = set(args.only.split(",")) if args.only else None
    matrix = []
    for name, pkg in PACKAGES.items():
        if only and name not in only:
            continue
        for series in series_of(name):
            for target in targets_of(name):
                if args.targets and target != "any" and target not in args.targets.split(","):
                    continue
                t = T(target)
                try:
                    version = pkg["latest"](series, t)
                except (SystemExit, urllib.error.URLError) as e:
                    print(f"skip {name} {series} {target}: {e}", file=sys.stderr)
                    continue
                if not version:
                    print(f"skip {name} {series} {target}: not published", file=sys.stderr)
                    continue
                if (name, version, target) in have and not args.force:
                    continue
                runner = NATIVE_RUNNER[target] if is_native(name, t) else "ubuntu-latest"
                matrix.append(dict(name=name, series=series, version=version, target=target, runner=runner))
                print(f"need {name} {series} → {version} ({target})", file=sys.stderr)
    print(json.dumps({"include": matrix}))


def cmd_build(args):
    pkg = PACKAGES[args.name]
    t = T(args.target)
    version = args.version or pkg["latest"](args.series, t)
    if not version:
        raise SystemExit(f"{args.name} {args.series} is not published for {args.target}")
    if is_native(args.name, t):
        host_os = {"darwin": "macos", "linux": "linux", "win32": "windows"}.get(sys.platform, sys.platform)
        host_arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x86_64"
        if f"{host_os}-{host_arch}" != args.target:
            raise SystemExit(f"{args.name} must be built on {args.target} (this is {host_os}-{host_arch})")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp()) / "pkg"
    root.mkdir()
    pkg["build"](args.series, version, t, root)
    (root / ".pack-package").write_text(json.dumps({"name": args.name, "series": args.series, "version": version, "target": args.target}))
    asset = f"{args.name}-{version}-{args.target}.tar.gz"
    # System tar is much faster than tarfile for the big ones (MySQL, MongoDB).
    run(["tar", "-czf", str((out / asset).resolve()), "-C", str(root)] + sorted(p.name for p in root.iterdir()), env=dict(os.environ, COPYFILE_DISABLE="1"))
    sha = hashlib.sha256((out / asset).read_bytes()).hexdigest()
    os_, _, arch = args.target.partition("-")
    meta = dict(name=args.name, series=args.series, version=version, os=os_ if arch else "any", arch=arch or "any", asset=asset, sha256=sha, size=(out / asset).stat().st_size)
    (out / f"{asset}.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta))


def cmd_merge(args):
    manifest = load_manifest(args.manifest)
    pkgs = {(p["name"], p["version"], entry_target(p)): p for p in manifest["packages"]}
    for meta_file in sorted(Path(args.dir).rglob("*.tar.gz.json")):
        m = json.loads(meta_file.read_text())
        tag = f"{m['name']}-{m['version']}"
        url = f"{args.base_url.rstrip('/')}/{tag}/{m['asset']}" if args.base_url else str(meta_file.resolve().with_suffix(""))
        entry = dict(name=m["name"], series=m["series"], version=m["version"], os=m.get("os", "macos"), arch=m["arch"], url=url, sha256=m["sha256"], size=m["size"])
        pkgs[(m["name"], m["version"], entry_target(entry))] = entry
        print(f"+ {m['name']} {m['version']} {entry_target(entry)}", file=sys.stderr)
    # Keep the newest 3 builds per (name, series, arch) so rollbacks stay possible.
    keep = []
    groups = {}
    for p in pkgs.values():
        p.setdefault("os", "macos")
        groups.setdefault((p["name"], p["series"], entry_target(p)), []).append(p)
    for group in groups.values():
        keep.extend(sorted(group, key=lambda p: vkey(p["version"]), reverse=True)[:3])
    keep.sort(key=lambda p: (p["name"], [-x for x in vkey(p["series"])] if p["series"][0].isdigit() else [0], [-x for x in vkey(p["version"])], entry_target(p)))
    manifest = {"schema": 1, "generated": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"), "packages": keep}
    Path(args.manifest).write_text(json.dumps(manifest, indent=1) + "\n")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("plan")
    p.add_argument("--manifest", default="dist/manifest.json")
    p.add_argument("--only")
    p.add_argument("--force", action="store_true")
    p.add_argument("--targets", help="limit to these targets, comma-separated")
    b = sub.add_parser("build")
    b.add_argument("name", choices=PACKAGES)
    b.add_argument("series")
    b.add_argument("target", choices=TARGETS + ["any"])
    b.add_argument("--version")
    b.add_argument("--out", default="dist/out")
    m = sub.add_parser("merge")
    m.add_argument("--manifest", default="dist/manifest.json")
    m.add_argument("--dir", default="dist/out")
    m.add_argument("--base-url", default="")
    args = ap.parse_args()
    {"plan": cmd_plan, "build": cmd_build, "merge": cmd_merge}[args.cmd](args)


if __name__ == "__main__":
    main()
