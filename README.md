# Pack

A free, open-source local PHP development environment for macOS. It's a Laravel Herd alternative where the "Pro" parts (databases, Redis, search, mail, S3, dumps, log viewer) cost nothing.

- **Sites:** park a folder and every project in it is served at `http://<folder>.test`. You can also `link`, `secure` (HTTPS), `isolate` (PHP per site), `proxy` and `share` (public URL through a free Cloudflare tunnel).
- **In the box:** `pack install` sets up the web server, PHP, Composer, MySQL, PostgreSQL, Redis and Mail. Everything is prebuilt by this repo's CI, so nothing compiles on your Mac.
- **PHP 8.1–8.5:** static builds with intl, opcache, sodium, redis, pdo_* and more. Each version gets its own PHP-FPM pool. `php`, `php84`, `composer` and `laravel` are put on your PATH.
- **Services:** MySQL 8.4/9.7, PostgreSQL 14–18, Redis (Valkey 8/9), Meilisearch, Typesense, Mail (Mailpit), S3 (RustFS), MongoDB and Laravel Reverb.
- **Versions and updates:** several versions can be installed side by side. `pack update` (or the Update button in the app) moves each one to the newest patch of its version line. `pack service version mysql 9.7` upgrades a database in place and keeps its data. `pack self-update` updates Pack itself.
- **Dumps:** `dump()`/`dd()` output from your sites appears in the app, with file and line. `pack dumps` streams it to the terminal.
- **Logs:** a parsed viewer for Laravel's `storage/logs`, plus Pack's own process logs.
- **App:** a Tauri desktop window and menu-bar icon. `pack daemon` does the actual work, so closing the app keeps your sites running.

Screenshots are in [docs/screenshots](docs/screenshots). Feature parity and the roadmap are in [docs/parity.md](docs/parity.md).

## Install (from source)

```sh
cargo build --release
./target/release/pack install   # downloads Caddy/PHP/Composer, sudo once for .test DNS + HTTPS, LaunchAgent
```

Then:

```sh
cd ~/Pack && laravel new shop     # → http://shop.test
pack secure shop                  # → https://shop.test
pack service add mysql            # prints the .env lines
pack db shop
pack isolate 8.3 --site shop
pack share shop                   # → https://<random>.trycloudflare.com
pack dumps                        # stream dump() output
```

To open the app, run `cargo run -p pack-app`. The dashboard is also available at http://127.0.0.1:7171.

> **Coming from Herd?** Pack and Herd both claim ports 80/443 and `/etc/resolver/test`. Quit Herd before you run `pack install`. `pack uninstall` gives `.test` back.

## How it works

| Piece | What |
|---|---|
| `daemon/` (`pack`) | One Rust binary that is both the CLI and the daemon. The daemon includes a supervisor (restarts crashed processes, stops them with SIGTERM), a DNS responder, the HTTP API on `127.0.0.1:7171` and the dashboard (`daemon/ui/index.html`). |
| Caddy | Serves all sites and runs HTTPS with its internal CA. It talks FastCGI to `run/php84.sock` and so on. The Caddyfile is regenerated and hot-reloaded on every change. |
| Packages | Every binary comes from **this repo**. `scripts/packages.py` builds PHP (with static-php-cli) and Valkey from source, and repackages MySQL, Postgres, Caddy, Mailpit and the other upstream releases. CI (`.github/workflows/packages.yml`, daily) publishes them as release assets and records them with SHA-256 in [`dist/manifest.json`](dist/manifest.json). Pack installs into `packages/<name>/<series>/<version>` behind a `current` symlink, so an update is: download, verify the checksum, flip the symlink, restart. |
| Services | Data lives in `data/<id>` and survives updates and version switches. |
| `app/` | Tauri 2 shell with a tray menu. It starts the daemon if it isn't already running. |

Everything Pack owns lives in `~/Library/Application Support/Pack`. You can override that location with `PACK_HOME`, for example to test against ports 8080/8443 without touching your system.

## Publishing packages

```sh
python3 scripts/packages.py plan                         # what's new upstream / missing from the manifest
python3 scripts/packages.py build php 8.4 arm64 --out dist/out
python3 scripts/packages.py merge --dir dist/out --base-url https://github.com/KumaVolt/pack/releases/download
```

In CI the Packages workflow does all of this on a schedule. To test against locally built packages, run `merge` without `--base-url` (this writes local paths into the manifest) and point Pack at the result with `PACK_MANIFEST=/path/to/manifest.json`.

## Development

```sh
PACK_HOME=/tmp/packtest cargo run -p pack -- status   # auto-starts a sandboxed daemon
cargo test -p pack
```

MIT licensed.
