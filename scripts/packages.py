#!/usr/bin/env python3
"""Pack's package factory.

Every binary Pack installs (PHP, MySQL, Postgres, Valkey, ...) is built or
repackaged here and published as a GitHub release asset of the Pack repo.
`dist/manifest.json` lists them with SHA-256 so the app installs and updates
from one trusted place.

    packages.py plan  [--manifest M] [--only php,mysql]   # JSON build matrix of what's missing
    packages.py build NAME SERIES ARCH [--version V] --out DIR
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
import urllib.error
import urllib.request
from pathlib import Path

UA = {"User-Agent": "pack-packages"}
ARCHES = ["arm64", "x86_64"]
NATIVE_RUNNER = {"arm64": "macos-15", "x86_64": "macos-15-intel"}

# Extensions compiled into Pack's PHP — what Laravel apps need (Herd's set minus imagick/sqlsrv).
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
    cmd = ["tar", "-xf", str(archive), "-C", str(into)]
    if strip:
        cmd.append(f"--strip-components={strip}")
    run(cmd)


# ------------------------------------------------------------------ packages
#
# Each package: series (a list, or a function returning one), latest(series)
# -> version, arches, runner ("native" = must be built on that arch's macOS
# runner, else ubuntu repacks), and build(series, version, arch, root).

def php_latest(series, arch="arm64"):
    data = json.loads(text(f"https://www.php.net/releases/?json&version={series}"))
    return data["version"]


def php_build(series, version, arch, root):
    work = Path(os.environ.get("SPC_WORKDIR", Path.home() / ".cache/pack-spc"))
    work.mkdir(parents=True, exist_ok=True)
    spc = work / "spc"
    if not spc.exists():
        cpu = "aarch64" if arch == "arm64" else "x86_64"
        tag = latest_tag("crazywhalecc/static-php-cli")
        archive = download(f"https://github.com/crazywhalecc/static-php-cli/releases/download/{tag}/spc-macos-{cpu}.tar.gz", work / "spc.tar.gz")
        untar(archive, work)
        spc.chmod(0o755)
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="13.0")
    run([spc, "doctor", "--auto-fix"], cwd=work, env=env)
    # Always fetch php-src for this exact version (spc caches by name, not version).
    run([spc, "download", f"--for-extensions={PHP_EXTENSIONS}", f"--with-php={version}", "--prefer-pre-built", "--ignore-cache-sources=php-src"], cwd=work, env=env)
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
    out = subprocess.run([root / "php", "-r", "echo PHP_VERSION;"], capture_output=True, text=True, check=True).stdout
    if out != version:
        raise SystemExit(f"built PHP {out}, expected {version}")
    mods = subprocess.run([root / "php", "-m"], capture_output=True, text=True).stdout.lower()
    for ext in ["intl", "pdo_sqlite", "pdo_mysql", "pdo_pgsql", "redis", "sodium", "zend opcache"]:
        if ext not in mods:
            raise SystemExit(f"PHP build is missing {ext}")


def mysql_url(series, version, arch):
    cpu = "arm64" if arch == "arm64" else "x86_64"
    folder = ".".join(version.split(".")[:2])
    return f"https://cdn.mysql.com/Downloads/MySQL-{folder}/mysql-{version}-macos15-{cpu}.tar.gz"


def newest_published(tags, url_fn):
    """Newest tag whose download actually exists (tags can precede binaries)."""
    for v in sorted(tags, key=vkey, reverse=True)[:6]:
        if exists(url_fn(v)):
            return v
    return None


def mysql_latest(series, arch="arm64"):
    # Oracle doesn't publish every patch for Intel, so resolve per architecture.
    tags = [t[len("mysql-"):] for t in git_tags("mysql/mysql-server") if t.startswith(f"mysql-{series}.")]
    return newest_published(tags, lambda v: mysql_url(series, v, arch))


def mysql_build(series, version, arch, root):
    tmp = Path(tempfile.mkdtemp())
    untar(download(mysql_url(series, version, arch), tmp / "mysql.tar.gz"), root, strip=1)
    # Trim what a dev box never needs (~60% of the tarball).
    for junk in ["mysql-test", "docs", "man", "include", "lib/plugin/debug", "bin/mysqld-debug"]:
        p = root / junk
        shutil.rmtree(p, ignore_errors=True) if p.is_dir() else p.unlink(missing_ok=True)


def postgres_latest(series, arch="arm64"):
    tags = [t for t in git_tags("theseus-rs/postgresql-binaries") if t.split(".")[0] == series and re.fullmatch(r"[\d.]+", t)]
    return newest(tags)


def postgres_build(series, version, arch, root):
    cpu = "aarch64" if arch == "arm64" else "x86_64"
    url = f"https://github.com/theseus-rs/postgresql-binaries/releases/download/{version}/postgresql-{version}-{cpu}-apple-darwin.tar.gz"
    tmp = Path(tempfile.mkdtemp())
    untar(download(url, tmp / "pg.tar.gz"), root, strip=1)
    shutil.rmtree(root / "include", ignore_errors=True)


def valkey_latest(series, arch="arm64"):
    return newest([t for t in git_tags("valkey-io/valkey") if re.fullmatch(rf"{series}\.\d+\.\d+", t)])


def valkey_build(series, version, arch, root):
    tmp = Path(tempfile.mkdtemp())
    src = tmp / "src"
    untar(download(f"https://github.com/valkey-io/valkey/archive/refs/tags/{version}.tar.gz", tmp / "v.tar.gz"), src, strip=1)
    env = dict(os.environ, MACOSX_DEPLOYMENT_TARGET="13.0")
    run(["make", f"-j{os.cpu_count() or 4}", "BUILD_TLS=no"], cwd=src, env=env)
    for b in ["valkey-server", "valkey-cli", "valkey-benchmark"]:
        shutil.copy2(src / "src" / b, root / b)


def gh_latest(repo):
    return lambda series, arch="arm64": latest_tag(repo).lstrip("v")


def single_binary(url_fn, name):
    def build(series, version, arch, root):
        tmp = Path(tempfile.mkdtemp())
        url = url_fn(version, arch)
        f = download(url, tmp / url.rsplit("/", 1)[1])
        if url.endswith((".tar.gz", ".tgz", ".zip")):
            untar(f, tmp / "x")
            hits = [p for p in (tmp / "x").rglob(name) if p.is_file()]
            if not hits:
                raise SystemExit(f"{name} not found in {url}")
            shutil.copy2(hits[0], root / name)
        else:
            shutil.copy2(f, root / name)
        (root / name).chmod(0o755)
    return build


def goarch(arch):
    return "arm64" if arch == "arm64" else "amd64"


def mongodb_latest(series, arch="arm64"):
    tags = [t[1:] for t in git_tags("mongodb/mongo") if re.fullmatch(rf"r{re.escape(series)}\.\d+", t)]
    return newest_published(tags, lambda v: mongodb_url(v, arch))


def mongodb_url(version, arch):
    cpu = "arm64" if arch == "arm64" else "x86_64"
    return f"https://fastdl.mongodb.org/osx/mongodb-macos-{cpu}-{version}.tgz"


def mongodb_build(series, version, arch, root):
    tmp = Path(tempfile.mkdtemp())
    untar(download(mongodb_url(version, arch), tmp / "m.tgz"), root, strip=1)


def composer_latest(series, arch="any"):
    return json.loads(text("https://getcomposer.org/versions"))["stable"][0]["version"]


def composer_build(series, version, arch, root):
    download(f"https://getcomposer.org/download/{version}/composer.phar", root / "composer.phar")


def major(version):
    return version.split(".")[0]


PACKAGES = {
    "php": dict(series=["8.5", "8.4", "8.3", "8.2", "8.1"], latest=php_latest, build=php_build, native=True),
    "mysql": dict(series=["8.4", "9.7"], latest=mysql_latest, build=mysql_build),
    "postgres": dict(series=["18", "17", "16", "15", "14"], latest=postgres_latest, build=postgres_build),
    "redis": dict(series=["9", "8"], latest=valkey_latest, build=valkey_build, native=True),
    "meilisearch": dict(
        series=["1"], latest=gh_latest("meilisearch/meilisearch"),
        build=single_binary(lambda v, a: f"https://github.com/meilisearch/meilisearch/releases/download/v{v}/meilisearch-macos-{'apple-silicon' if a == 'arm64' else 'amd64'}", "meilisearch"),
    ),
    "typesense": dict(
        series=lambda: [major(latest_tag("typesense/typesense").lstrip("v"))],
        latest=gh_latest("typesense/typesense"),
        build=single_binary(lambda v, a: f"https://dl.typesense.org/releases/{v}/typesense-server-{v}-darwin-{goarch(a)}.tar.gz", "typesense-server"),
    ),
    "mail": dict(
        series=["1"], latest=gh_latest("axllent/mailpit"),
        build=single_binary(lambda v, a: f"https://github.com/axllent/mailpit/releases/download/v{v}/mailpit-darwin-{goarch(a)}.tar.gz", "mailpit"),
    ),
    "s3": dict(
        series=["1"], latest=gh_latest("rustfs/rustfs"), arches=["arm64"],
        build=single_binary(lambda v, a: f"https://github.com/rustfs/rustfs/releases/download/{v}/rustfs-macos-aarch64-v{v}.zip", "rustfs"),
    ),
    "mongodb": dict(series=["8.0", "7.0"], latest=mongodb_latest, build=mongodb_build),
    "caddy": dict(
        series=["2"], latest=gh_latest("caddyserver/caddy"),
        build=single_binary(lambda v, a: f"https://github.com/caddyserver/caddy/releases/download/v{v}/caddy_{v}_mac_{goarch(a)}.tar.gz", "caddy"),
    ),
    "cloudflared": dict(
        series=["stable"], latest=gh_latest("cloudflare/cloudflared"),
        build=single_binary(lambda v, a: f"https://github.com/cloudflare/cloudflared/releases/download/{v}/cloudflared-darwin-{goarch(a)}.tgz", "cloudflared"),
    ),
    "composer": dict(series=["2"], latest=composer_latest, build=composer_build, arches=["any"]),
}


def series_of(name):
    s = PACKAGES[name]["series"]
    return s() if callable(s) else s


def arches_of(name):
    return PACKAGES[name].get("arches", ARCHES)


# ------------------------------------------------------------------ commands

def load_manifest(path):
    if path and Path(path).exists():
        return json.loads(Path(path).read_text())
    return {"schema": 1, "packages": []}


def cmd_plan(args):
    manifest = load_manifest(args.manifest)
    have = {(p["name"], p["version"], p["arch"]) for p in manifest["packages"]}
    only = set(args.only.split(",")) if args.only else None
    matrix = []
    for name, pkg in PACKAGES.items():
        if only and name not in only:
            continue
        for series in series_of(name):
            for arch in arches_of(name):
                try:
                    version = pkg["latest"](series, arch)
                except SystemExit as e:
                    print(f"skip {name} {series} {arch}: {e}", file=sys.stderr)
                    continue
                if not version:
                    print(f"skip {name} {series} {arch}: not published", file=sys.stderr)
                    continue
                if (name, version, arch) in have and not args.force:
                    continue
                runner = NATIVE_RUNNER[arch] if pkg.get("native") else "ubuntu-latest"
                matrix.append(dict(name=name, series=series, version=version, arch=arch, runner=runner))
                print(f"need {name} {series} → {version} ({arch})", file=sys.stderr)
    print(json.dumps({"include": matrix}))


def cmd_build(args):
    pkg = PACKAGES[args.name]
    version = args.version or pkg["latest"](args.series, args.arch)
    if pkg.get("native") and args.arch != "any":
        host = "arm64" if platform.machine() == "arm64" else "x86_64"
        if host != args.arch or sys.platform != "darwin":
            raise SystemExit(f"{args.name} must be built on a {args.arch} Mac (this is {sys.platform}/{host})")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp()) / "pkg"
    root.mkdir()
    pkg["build"](args.series, version, args.arch, root)
    (root / ".pack-package").write_text(json.dumps({"name": args.name, "series": args.series, "version": version, "arch": args.arch}))
    asset = f"{args.name}-{version}-macos-{args.arch}.tar.gz"
    # System tar is much faster than tarfile for the big ones (MySQL, MongoDB).
    run(["tar", "-czf", str((out / asset).resolve()), "-C", str(root)] + sorted(p.name for p in root.iterdir()), env=dict(os.environ, COPYFILE_DISABLE="1"))
    sha = hashlib.sha256((out / asset).read_bytes()).hexdigest()
    meta = dict(name=args.name, series=args.series, version=version, arch=args.arch, asset=asset, sha256=sha, size=(out / asset).stat().st_size)
    (out / f"{asset}.json").write_text(json.dumps(meta, indent=2))
    print(json.dumps(meta))


def cmd_merge(args):
    manifest = load_manifest(args.manifest)
    pkgs = {(p["name"], p["version"], p["arch"]): p for p in manifest["packages"]}
    for meta_file in sorted(Path(args.dir).rglob("*.tar.gz.json")):
        m = json.loads(meta_file.read_text())
        tag = f"{m['name']}-{m['version']}"
        url = f"{args.base_url.rstrip('/')}/{tag}/{m['asset']}" if args.base_url else str(meta_file.resolve().with_suffix(""))
        pkgs[(m["name"], m["version"], m["arch"])] = dict(
            name=m["name"], series=m["series"], version=m["version"], arch=m["arch"], url=url, sha256=m["sha256"], size=m["size"]
        )
        print(f"+ {m['name']} {m['version']} {m['arch']}", file=sys.stderr)
    # Keep the newest 3 builds per (name, series, arch) so rollbacks stay possible.
    keep = []
    groups = {}
    for p in pkgs.values():
        groups.setdefault((p["name"], p["series"], p["arch"]), []).append(p)
    for group in groups.values():
        keep.extend(sorted(group, key=lambda p: vkey(p["version"]), reverse=True)[:3])
    keep.sort(key=lambda p: (p["name"], [-x for x in vkey(p["series"])] if p["series"][0].isdigit() else [0], [-x for x in vkey(p["version"])], p["arch"]))
    manifest = {"schema": 1, "generated": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"), "packages": keep}
    Path(args.manifest).write_text(json.dumps(manifest, indent=1) + "\n")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("plan")
    p.add_argument("--manifest", default="dist/manifest.json")
    p.add_argument("--only")
    p.add_argument("--force", action="store_true")
    b = sub.add_parser("build")
    b.add_argument("name", choices=PACKAGES)
    b.add_argument("series")
    b.add_argument("arch", choices=ARCHES + ["any"])
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
