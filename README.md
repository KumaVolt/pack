# Pack

A free, open-source local PHP development environment for macOS, Windows and Linux.

Put a project in a folder and it's live at `https://<folder>.test`: HTTPS, PHP, databases and mail included, nothing to configure.

- **Sites:** every folder becomes `https://<name>.test` with a trusted certificate. You can turn HTTPS off per site.
- **PHP:** versions 8.2 to 8.5 side by side, picked per site. Composer and the Laravel installer are included.
- **Services:** MySQL, PostgreSQL, Redis, Meilisearch, Typesense (search for Laravel Scout), MongoDB, S3 storage, Reverb. Each starts with one click, and switching versions keeps your data.
- **Mail and dumps:** a local inbox catches every email. `dump()` and `dd()` output shows up in the app.
- **Sharing:** you can share a site through a public link.
- **Updates:** PHP and every service update from the app or with `pack update`.

## Install

Download the app for your system from [Releases](https://github.com/KumaVolt/pack/releases):

| System  | File |
|---------|------|
| macOS   | `.dmg` (Apple Silicon or Intel) |
| Windows | `.msi` or `-setup.exe` |
| Linux   | `.deb` or `.AppImage` |

Open Pack and click **Set up**. Pack downloads what it needs and asks for your password once, so it can route `.test` domains to your machine and trust its HTTPS certificate.

> The app isn't code-signed yet. On macOS, right-click Pack.app → **Open** the first time. On Windows, choose **More info → Run anyway**.

## Command line

The app adds `pack`, `php`, `composer` and `laravel` to your terminal.

```sh
pack park ~/Code        # every folder in ~/Code becomes <folder>.test
pack link               # serve the current folder
pack open               # open it in the browser
pack use 8.3            # default PHP version
pack isolate 8.2        # PHP version for this site only
pack service add mysql  # add a service (postgres, redis, meilisearch, ...)
pack db shop            # create a database
pack share              # public link to this site
pack update             # update PHP and services
pack status
```

Run `pack --help` for everything.

## How it works

One small program (`pack`) runs in the background. It serves sites through [Caddy](https://caddyserver.com), runs a PHP-FPM pool per PHP version (php-cgi on Windows), and answers DNS for `.test`. It also keeps your services running.

Everything Pack installs is a prebuilt, checksummed package from this repository's releases, listed in [`dist/manifest.json`](dist/manifest.json). A daily workflow rebuilds them when new versions come out. All data lives in one folder, so removing that folder removes everything:

| System  | Data folder |
|---------|-------------|
| macOS   | `~/Library/Application Support/Pack` |
| Windows | `%LOCALAPPDATA%\Pack` |
| Linux   | `~/.local/share/pack` |

## Development

```sh
cargo run -p pack -- daemon       # run the daemon in the foreground
scripts/build-app.sh              # build the desktop app for this machine
scripts/smoke.sh target/release/pack   # end-to-end test in a throwaway home
```

The packages are built by `scripts/packages.py` (see `.github/workflows/packages.yml`).

## License

MIT
