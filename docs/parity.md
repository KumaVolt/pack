# Pack ↔ Laravel Herd parity

The full Herd inventory is in [herd-features.md](herd-features.md). Pack's rule: everything Herd Pro charges for has to be **free**, built from free, open-source binaries.

Legend: ✅ done and tested · 🟡 partial · ⬜ not yet

## Core (free in Herd)

| Herd | Pack | Status |
|---|---|---|
| nginx + dnsmasq | Caddy 2 + a built-in DNS responder (`/etc/resolver/test` → 127.0.0.1:53535) | ✅ |
| `park` / `unpark` / `paths` | same commands; new folders show up within 2 s | ✅ |
| `link` / `unlink` / `links` | same | ✅ |
| `secure` / `unsecure` | Caddy's local CA, trusted once via `pack install`; http→https redirect | ✅ |
| `isolate` / `unisolate` / `use` | per-site PHP, one FPM pool per version, `.php-version` for the CLI | ✅ |
| `proxy` / `unproxy` | Caddy `reverse_proxy` | ✅ |
| `share` (Expose, paid beyond the free tier) | free Cloudflare quick tunnel (`cloudflared`, no account) | ✅ |
| `open` | same | ✅ |
| PHP 7.4–8.5 | 8.2–8.5, built by this repo's CI with the full extension set | 🟡 no 7.4–8.1 (EOL; 8.1 fails on current Xcode) |
| `php`, `composer`, `laravel` on PATH | shims in `~/Library/Application Support/Pack/bin`, `php84` etc. | ✅ |
| Valet drivers | detection for Laravel, Statamic, Symfony, WordPress, Bedrock, Craft, static | 🟡 |
| `www.` and wildcard subdomains | `*.site.test` served by the site | ✅ |
| Node via nvm, `isolate-node` | — | ⬜ |
| `herd.yml` per-project config | — | ⬜ |
| New-project wizard (starter kits) | `laravel new` works from the shim; no GUI wizard | 🟡 |
| Menu bar app | Tauri tray: sites, mail, dumps, services, restart | ✅ |
| TLD switch, loopback IP | config fields exist (`tld`) but no command | ⬜ |
| Forge / Laravel Cloud deploy | — | ⬜ |
| MCP server for agents | — | ⬜ |
| Herd Studio / Workspaces (Herd 2) | — | ⬜ |

## Herd Pro → free in Pack

| Herd Pro | Pack | Status |
|---|---|---|
| MySQL 8.0/8.4/9.x | MySQL Community 8.4 / 9.7 (Oracle tarballs, GPL) | ✅ 8.4 tested |
| MariaDB | no official macOS binaries — needs our own CI builds | ⬜ |
| PostgreSQL 14–18 | theseus-rs/postgresql-binaries (no PostGIS/pgvector yet) | ✅ 18 tested |
| Redis / Valkey | Valkey 8/9 (BSD), prebuilt by Pack CI | ✅ |
| MongoDB | MongoDB Community tarballs (SSPL, free to use) | 🟡 untested |
| Meilisearch | Meilisearch CE (MIT) | ✅ |
| Typesense | Typesense (GPL-3) | ✅ |
| MinIO (frozen) | RustFS (Apache-2.0), S3-compatible + console | ✅ (arm64 only) |
| Reverb | `php artisan reverb:start` supervised per project | 🟡 untested |
| Mail (SMTP catcher) | Mailpit on :2525, inbox embedded in the app | ✅ |
| Dumps / Insights | `auto_prepend_file` overrides `dump()`/`dd()` and POSTs VarDumper HTML to Pack | ✅ dumps · ⬜ queries/jobs/HTTP/N+1 |
| Log viewer | Laravel `storage/logs` parser + Pack system logs | ✅ |
| Xdebug detection | needs Xdebug as a loadable extension in our PHP builds | ⬜ |
| Per-site services, "create DB" | `pack db <name>`, "New database" button | 🟡 |

## Roadmap (suggested order)

1. ✅ **Everything from this repo:** CI builds and repackages PHP, MySQL, Postgres, Valkey and the rest into `dist/manifest.json`. Installs, updates, side-by-side versions and `pack self-update` all use it.
2. **MariaDB, PostGIS/pgvector, and Xdebug as a shared extension** in the same package pipeline.
3. **Node versions** (download from nodejs.org, `.nvmrc`-aware shim).
4. **`pack.yml`/`herd.yml`** per project (name, PHP, secure, aliases, services).
5. **Insights:** queries, jobs, mail, HTTP and logs via the prepend hook (Laravel event listeners registered from the prepend file).
6. **Xdebug on demand:** a second FPM pool with Xdebug, routed by the `XDEBUG_SESSION` cookie.
7. **Signed `.app` bundle** with `pack` as a sidecar and a first-run wizard.
8. Orphan cleanup: if the daemon is SIGKILLed, its children keep running. Store PIDs and reap them on start.
