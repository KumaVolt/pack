# Laravel Herd (macOS) feature inventory, for Pack

Researched 2026-10-21. Sources:
- Official docs: `https://herd.laravel.com/docs/macos/...`. Every page also has a markdown copy at `<url>.md`, and the index is at `https://herd.laravel.com/docs/llms.txt`.
- The public services catalog API at `https://herdphp.com/api/services/{arm64,x86}`.
- The pricing page.
- A read-only inspection of the local install: Herd **1.26.1** at `/Applications/Herd.app` and `~/Library/Application Support/Herd`.

**The current release is Herd 2.0.2** (changelog). Herd 2 is a big redesign: Applications window, Herd Studio, Workspaces, Insights, automatic `artisan dev`, Laravel Cloud. **Herd 2 requires macOS 14.6+ and Apple Silicon.** Version 1.x also shipped x86 binaries (`nginx-x86`, `dnsmasq-x86`, `*-x64.so`).

Legend: **[local]** = I confirmed it on the installed 1.26.1. **[docs]** = it comes from the official docs. **[unverified]** = I inferred it or could not confirm it.

---

## 0. Pricing

| Plan | Price | Notes |
|---|---|---|
| Herd | Free | PHP, nginx, DNS, Node, sites, per-project config, Herd Studio, BYO coding agent, PHP Profiler |
| Herd Pro | EUR 99/yr | 1 license, 2 devices. Adds services (MySQL, PG, Redis...), Mail, Insights (dumps), Tinker-in-Studio, runtime context for agents, Xdebug detection, Log Viewer [the docs mark `herd logs` as Pro] |
| Herd for Teams | EUR 299/yr | 10 Pro licenses plus a license portal |

Insights has a trial that is metered per captured request or job.

---

## 1. Free features

### 1.1 Sites
| Feature | Details |
|---|---|
| Park | `herd park [dir]` makes every subdirectory `<dir-name>.<tld>`. `~/Herd` is parked by default. `herd forget` (alias `unpark`) removes it. `herd parked` and `herd paths` list them. |
| Link | `herd link [name] [--secure] [--isolate=] [--update-env]` serves the cwd as `name.test`. You can link one app under several names (aliases, multi-tenant). `herd links` lists links and `herd unlink [name]` removes one. |
| Secure | `herd secure [site]` creates a cert signed by the local CA and writes a per-site nginx conf with an HTTP->HTTPS 301 plus `listen 443 ssl; http2 on`. `herd unsecure` removes it. `herd secured` lists certs with expiry dates. |
| Isolate PHP | `herd isolate 8.3 [--site=]` writes a per-site nginx conf that points at the `herd83.sock` FPM pool. `herd unisolate` and `herd isolated` manage and list these. `herd use 8.4` sets the global version. |
| Isolate Node | `herd isolate-node 22` writes `.nvmrc`, and a zsh `chpwd` hook auto-runs `nvm use`. Also `unisolate-node` and `isolated-node`. |
| Proxy | `herd proxy name http://localhost:8080 [--secure]` creates an nginx reverse-proxy vhost (for Reverb, Docker, Next.js). Also `unproxy` and `proxies`. |
| Drivers | Valet drivers for Laravel, Symfony, WordPress, Bedrock, Craft, Drupal, Statamic (v1/v2/v3), Magento2, Joomla, Kirby, Contao, Concrete5, Cake, Jigsaw, Katana, Neos, Sculpin, Typo3, plus Basic and BasicWithPublic [local]. A project can add a custom `LocalValetDriver.php`. Driver hooks: `serves`, `isStaticFile`, `frontControllerPath`, `logFilesPaths`, `siteInformation`. |
| TLD | `herd tld` (alias `domain`). Allowed values: `test`, `wip`, `herd`, `local`, `localhost` [local, `Herd::AVAILABLE_TLDS`]. |
| Loopback | `herd loopback [ip]`. Default `127.0.0.1`. |
| Directory listing | `herd directory-listing on|off`. |
| `www.` | A `www.` prefix is stripped, so `www.x.test` and `x.test` serve the same site. |
| Worktree subdomains | `feature.shop.test` maps to a git worktree or workspace (Herd 2). |

### 1.2 Applications window (GUI, Herd 2)
- **Dashboard:** a "build with AI" prompt, favorites, and recent apps with live preview thumbnails.
- **All Applications:** grid and list views, sorting, search, groups ("Add Site To"), aliases ("Add Alias"), favorites, and a refresh button.
- **Inspector:**
  - Preview, URL, path, and an Expose share button.
  - "Deploy to Laravel Cloud".
  - "Build with AI" and a Workspaces list.
  - Quick actions: browser, editor, terminal, Finder.
  - Per-site Services (Pro).
  - Environment: PHP version (isolate), Node version (writes `.nvmrc`), and an HTTPS toggle.
  - Delete: a parked site goes to the Trash; a linked site is only unlinked.
- **New application wizard:** the Laravel installer with starter kits (None/React/Vue/Svelte/Livewire), auth options (incl. none and teams), test framework (Pest/PHPUnit), and DB choice. Pro users can install MySQL/MariaDB/PG from the wizard. It shows whether the domain is available and has a "Link Existing App" option.

### 1.3 Menu bar (tray) and settings
- **Tray menu [docs + local strings]:**
  - Applications, New Application, Insights, Logs, Mail.
  - Global PHP version dropdown.
  - Restart, Check for Updates (Herd, PHP and Node), Settings, Quit.
  - Holding the Option key shows "Force Stop All" and "Start All" when Debug Mode is on (About tab).
  - The Dock icon is optional ("Always Show Dock Icon").
- **Settings tabs [docs]:**
  - General: parked paths, Integrations, Configuration Files, server port and status.
  - PHP: install, update and delete versions; memory_limit and upload_max_filesize globally or per version; Laravel installer update.
  - JavaScript: nvm and Node versions.
  - Services (Pro).
  - Sharing: Expose account, default server and domain.
  - Herd Pro: license, Mail port, Insights server, Debugger.
  - AI.
  - Shortcuts.
  - About.
- **Global shortcuts:** Tinker/Tinkerwell for the active project, terminal at the active project, Applications window, Insights, toggle runtime context, clear Insights, Mails, new application. "Active project" means the last site served to a browser.
- **Terminal choice** for shortcuts: Terminal, iTerm or Warp.

### 1.4 PHP
- **Versions:** 7.4, 8.0, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6 (beta). The docs call 8.5 the default; the local 1.26.1 has `LATEST_PHP_VERSION='8.4'`.
- **Builds are Herd's own.** `herd-php-builds` CI produces statically linked binaries with `--enable-shared=no --enable-static=yes`. The banner reads "Built by Laravel Herd". They are **not** static-php-cli, although the approach is similar [local, `php -i`]. They can still load `.so` extensions such as Xdebug and Herd's own extension.
- **Bundled extensions (about 55):**
  - bcmath, bz2, calendar, ctype, curl, dba, dom, exif, ffi, fileinfo, filter, ftp, gd (freetype/jpeg/webp), gettext, gmp, iconv, igbinary, imagick, imap, intl, ldap, lz4.
  - mbstring, mongodb, mysqli, opcache (+JIT), openssl, pcntl, pdo, pdo_mysql, pdo_pgsql, pdo_sqlite, pdo_sqlsrv, pgsql, phar, posix, readline (libedit).
  - redis (igbinary/zstd/lz4), session, shmop, simplexml, soap, sockets, sodium, sqlsrv, sqlite3, sysv*, tokenizer, xml*, xsl, zip, zlib, zstd.
- **Extra extensions** come from Homebrew PHP plus `pecl install`, then an `extension=` line in php.ini.
- **php.ini:** one file per version at `config/php/<84>/php.ini`. Herd injects:
  - `curl.cainfo` and `openssl.cafile` pointing at a bundled `cacert.pem`.
  - `auto_prepend_file=/Applications/Herd.app/.../valet/dump-loader.php`.
  - `extension=.../herd-ext/herd-84-arm64.so`.
  - Defaults `memory_limit=128M` and `upload_max_filesize=post_max_size=2M`.
  - `herd ini [ver]` opens the file.
- **FPM:** one pool per version. Pool `[herd]` runs as the user, listens on `~/Library/Application Support/Herd/herd84.sock` with `pm=dynamic`, max_children 5. A second "debug" pool uses `herd-debug84.sock` and loads `debug.ini` (Xdebug).
- **CLI shims:** `php`, `php84`, `composer`, `laravel`, `expose`, `forge`, `herd`, `node` and `npm` (via nvm) all live in `~/Library/Application Support/Herd/bin`, which is added to PATH through `~/.zshrc` with a `# Herd` marker.
- **Proxies:** `herd php|composer|which-php` use the site's isolated PHP version.

### 1.5 Bundled tools
| Tool | Notes |
|---|---|
| Composer | `bin/composer`. Update with `composer self-update`. |
| Laravel installer | `bin/laravel`. `herd laravel:update`. |
| Expose | `bin/expose` (beyondcode/expose client, MIT). Needs an expose.dev account; free tier available. `herd share [url] [--subdomain --server --basicAuth]` and `herd fetch-share-url`. ngrok is documented as the alternative. |
| Forge CLI | `bin/forge`. With `herd.yml` integration it needs no server or site id. Commands: deploy, ssh, env:pull/push, logs, and more. |
| CPX | `laravel/cpx`, added in 1.30. |
| nvm + Node | Herd's own nvm copy lives in `config/nvm`. Node 16/18/20/22/24. `herd node:list` and `herd node:install`. |
| Xdebug | `.so` files ship for every PHP version and arch in `Resources/xdebug/xdebug-84-arm64.so`. Manual enabling is free. `herd debug <cmd>` loads debug.ini and `herd coverage <cmd>` sets the coverage mode. Bundled Xdebug is 3.5.3. |
| SPX profiler (free) | A fork at `beyondcode/php-spx`, installed with its `install.sh`. Web UI at `https://site.test/herd-profiler`. CLI: `herd profile [--live] artisan ...`. |
| Tinker | `herd tinker`. If Tinkerwell is installed it opens there. |
| AdminerEvo | Served at `database.herd.test` (`config/default-sites/adminer.conf`). Used by "open DB" when TablePlus is absent. |
| `herd db` | Opens the site's DB in TablePlus using `.env` credentials. Herd 2 adds `db:connection`, `db:list`, `db:use [--create]`, `db:create` and `db:drop` (MySQL/MariaDB/PG/SQLite). |
| fwd.host | A free Herd-run redirect proxy for OAuth callbacks: `https://fwd.host/http://site.test/cb`. |

### 1.6 `herd.yml` (project manifest)
`herd init` runs a wizard when the file is absent and applies it when present. `herd init --fresh` (also `init:fresh`) regenerates it. `.env.example` is copied if `.env` is missing.

```yaml
name: herd-website
php: '8.3'
secured: true
aliases: [herd-laravel]
services:                 # Pro. Missing services are skipped with a warning.
  mysql:  { version: 8.0.36, port: '${DB_PORT}' }   # ${VAR} resolved from .env
  redis:  { version: 7.0.0,  port: '${REDIS_PORT}' }
integrations:
  forge:
    herd.laravel.com: { server-id: 12345, site-id: 67890 }
workspaces:               # Herd 2
  database: clone         # shared | clone | empty
  migrate: true
  app_dir: apps/backend
  copy: [.env.testing, storage/app/public]
  env: { APP_URL: "{{scheme}}://app.{{domain}}", MAIL_FROM_NAME: "{{branch}}" }
  hooks: { after_create: [composer install], before_delete: php artisan queue:clear }
```

Port placeholders map to env keys: `redis`->`REDIS_PORT`; `mysql`, `mariadb`, `postgresql` and `mongodb`->`DB_PORT`; `typesense`->`TYPESENSE_PORT`; `meilisearch`->`MEILISEARCH_HOST` (port parsed after the last `:`); `minio`->`AWS_ENDPOINT`; `reverb`->`REVERB_PORT` [local, `Manifest.php`]. A git-ignored `herd.local.yml` can override values.

### 1.7 Workspaces (Herd 2, free)
- **What it is:** a copy-on-write (APFS clone) copy of the project on a new git branch, served as `<branch>.<site>.test`. It inherits the PHP version and HTTPS setting.
- **Commands:** `herd workspace [name] [--database=shared|clone|empty] [--no-migrate] [--local] [--secure|--no-secure] [--json]`, `workspaces [--site=]`, `workspace:switch|open|merge [--push --delete]|delete [--force --keep-database --keep-files]`.
- **Database copy:** MySQL uses `mysqldump`, PG uses `CREATE DATABASE ... TEMPLATE`, and SQLite copies the file. The new DB is named `parent_db_branch`.
- **`.env` rewrite:** `APP_URL`, `SESSION_DOMAIN`, `SANCTUM_STATEFUL_DOMAINS` and `DB_DATABASE` are updated.
- **Hook environment:** `HERD_WORKSPACE_*`.
- **Existing worktrees:** `herd workspace` inside a `git worktree` registers it, and `vendor`/`node_modules` are reused when the lockfiles match.
- **State file:** `config/valet/workspaces.json`.

### 1.8 Herd Studio and AI (Herd 2, free except Tinker and Insights tabs)
- **Badge:** an "Edit with Herd" badge is injected into every page of every `.test` site. `/herd-studio` on any site, or `herd studio [site]`, opens a browser IDE next to a live preview.
- **Studio tabs:**
  - Agent chat, Code editor (PHP completion), Changes (diff, git init), Terminal.
  - Dev server, Logs (Pail), Queue (`queue:listen`).
  - Site settings, Tinker (Pro), Insights (Pro), Share (Expose), Cloud deploy.
- **Unknown `.test` domains** show a "launcher" with recent projects instead of a 404.
- **Agents:** bundled **opencode** is used with BYO provider (Anthropic, OpenAI, OpenCode Zen free models, Copilot, Google, OpenRouter...). A Claude Pro/Max plan works through the `opencode-claude` plugin and the local `claude` CLI. Dictation uses Apple speech on macOS 26.
- **Ports:** the Devtools helper listens on `127.0.0.1:4421` and opencode on `4422`.
- **Files:**
  - `bin/tools` holds opencode, the devtools helper and the mail server.
  - `herd-ai-widget` holds the UI.
  - `devtools` holds sessions.
- **Auto dev server:**
  - On the first page hit, Herd runs `php artisan dev` (Laravel 13.25+) or the package.json `dev` script with the detected package manager (Vite).
  - It holds the first request for up to 15 s if `public/build` is missing.
  - It stops 1 min after the last Studio tab closes and 5 min after the last request.
- **"Fix with AI"** in the Log Viewer sends the entry to an agent.

### 1.9 MCP server (free)
- **Setup:** `php /Applications/Herd.app/Contents/Resources/herd-mcp.phar` over stdio with env `SITE_PATH`. Laravel Boost installs it automatically, or the AI settings toggle "Use Herd in your coding agents (MCP)".
- **Tools:** `find_available_services`, `install_service(type,port)`, `start_or_stop_service(shouldStart,type,port,version)`, `get_all_php_versions`, `install_php_version`, `get_all_sites`, `secure_or_unsecure_site`, `isolate_or_unisolate_site`, `get_last_deployment_information` (Forge).
- **Prompt:** `debug_site`.
- **Resource:** `site_information`.

### 1.10 Integrations
- **Forge:**
  - OAuth (v2 API, organizations, multiple accounts).
  - Link a local site to one or more Forge sites. The site and server ids are stored in `herd.yml`.
  - Deploy, open in Forge, SSH in a terminal, view the remote `.env`, and the Forge CLI without ids.
  - The MCP server exposes the last deployment.
- **Laravel Cloud (Herd 2):**
  - OAuth per organization; the token is also written to `~/.config/cloud/config.json`.
  - "Ship/Deploy to Laravel Cloud" wraps the `cloud` CLI and shows Prepare/Build/Release progress.
  - Detection works through the GitHub `origin` remote.
- **Expose:** OAuth "Connect with Expose", plan and server selection, custom domain (Expose Pro).
- **Vapor:** API strings are present in the binary [local]; the UI is unclear [unverified].

### 1.11 Full `herd` CLI command list
From `herd list` on 1.26.1 [local] and the Herd 2 CLI docs [docs]. Commands marked "Pro" need a license, and "2.0" marks commands that are new in Herd 2.

- **Sites:**
  - `park`, `parked`, `forget` (unpark), `paths`.
  - `link`, `links`, `unlink`, `sites`.
  - `open`, `edit`, `which`.
  - `site-information`, `site:info` (2.0), `site:env list|get|set|unset` (2.0), `site:services [category] [value]` (2.0, Pro).
  - `secure`, `unsecure`, `secured`.
  - `proxy`, `unproxy`, `proxies`.
  - `tld` (domain), `loopback`, `directory-listing`, `studio` (2.0).
- **Workspaces (2.0):** `workspace`, `workspaces`, `workspace:switch`, `workspace:open`, `workspace:merge`, `workspace:delete`.
- **Databases:** `db`, `db:connection`, `db:list`, `db:use`, `db:create`, `db:drop` (2.0).
- **PHP:** `use`, `isolate`, `unisolate`, `isolated`, `php`, `composer`, `which-php`, `ini`, `php:list`, `php:install`, `php:update [ver|all]`.
- **Node:** `isolate-node`, `unisolate-node`, `isolated-node`, `node:list` (2.0), `node:install` (2.0).
- **Services (Pro):** `services:list|available|versions|create|start|stop|clone|delete`.
- **Mail (Pro, 2.0):**
  - `mail:list|show|latest|wait|links|attachments|attachment|mailboxes|delete|clear|config`.
  - Filters: `--mailbox`, `--to`, `--from`, `--subject`, `--since`, `--limit`, `--format=html|raw|json`, `--timeout`.
- **Insights (Pro, 2.0):** `insights:list|watch|analyze [--after=]|clear|status|capture on|off`. Options: `--site`, `--all`, `--type`.
- **Debug:**
  - `debug`, `coverage`, `debug:start` (Pro), `debug:stop [--json]` (Pro).
  - `profile [--live]`.
  - `log [service]`, `logs` (Pro Log Viewer).
  - `tinker`.
- **Share:** `share`, `fetch-share-url`.
- **Core:** `status [--json]` (2.0), `start [nginx|dnsmasq|php]`, `stop`, `restart`.
- **Init:** `init`, `init:fresh`, `laravel:update`, `completion`.
- **Global:** `--json` works on many commands. `status`, `site:info`, `site:services`, `node:list`, `mail:*` and `insights:*` talk to the running app.

---

## 2. Herd Pro (paid) features

### 2.1 Services
- **Service manager (Settings -> Services):**
  - Add a service type and version. Herd suggests a name and a free port, so several instances can run side by side.
  - Per-instance settings: name, port, autostart.
  - The detail view shows the binary and data directories, ready-to-copy `.env` vars, and a log tail.
  - Context menu: force quit, open the bin or data dir, copy the connection URL, clone (with data), update, delete.
  - Each instance's config file lives in its data dir: `my.cnf`, `postgresql.conf`, `redis.conf`, `valkey.conf`, `mongodb.conf`.
- **Storage location:** `~/Library/Application Support/Herd/config/services`.
- **Catalog source:** `https://herdphp.com/api/services/{arm64|x86}` returns `{type,label,version,default_port,categories,download_path}`. The zips come from `download.herdphp.com/services/...`.

Catalog as fetched on 2026-10-21 (arm64):

| type | versions in API | default port | category | Herd download (arm64) |
|---|---|---|---|---|
| mysql | 8.4.2, 8.0.36 (docs matrix also lists 9.4.x, 9.7.x) | 3306 | database | `services/mysql/8.4.2-arm64.zip` (x86: `-x86_64.zip`) |
| mariadb | 10.11.6 (docs: 11.8.x too) | 3306 | database | `services/mariadb/mariadb-10.11.6-arm64.zip` |
| postgresql | 14, 15, 16, 17, 18 (bundles PostGIS, pgrouting, pgvector) | 5432 | database | `services/postgres/18-universal.zip` |
| mongodb | 7.0.45 | **27020** | database | `services/mongodb/7.0.45-arm64.zip` |
| redis | 7.4.11 (docs: 7.0.x too) | 6379 | queue, cache | `services/redis/7.4.11-universal.zip` |
| valkey | 8.1.10, 7.2.14 (docs: 8.0.x) | 6379 | queue, cache | `services/valkey/8.1.10-universal.zip` |
| reverb | 1.x (composer app plus dashboard at `reverb-dashboard.herd.test`) | 8080 | realtime | `services/reverb/reverb-dashboard-2.zip` |
| meilisearch | 1.55.0 | 7700 | search | `services/meilisearch/1.55.0-arm64.zip` |
| typesense | 30.2, 29.0, 0.26 | 8108 | search | `services/typesense/30.2-arm64.zip` |
| minio | RELEASE.2025-09-07 | 9000 | storage | `services/minio/RELEASE.2025-09-07-arm64.zip` |
| rustfs | 1.0.3 (x86: 1.0.0.89) | 9000 (console = port+1) | storage | `services/rustfs/1.0.3-arm64.zip` |

Default credentials and env from the docs:
- MySQL/MariaDB/PG: `root` with no password.
- Mongo: `DB_URI=mongodb://127.0.0.1:27020/laravel`.
- Meilisearch: `MEILISEARCH_KEY=LARAVEL-HERD`.
- Typesense: `TYPESENSE_API_KEY=LARAVEL-HERD`.
- MinIO/RustFS: `herd` / `secretkey`, path-style. Optional "Serve over HTTPS" gives `https://<name>.herd.test`.
- Reverb: `REVERB_APP_ID=1001`, `REVERB_APP_KEY=laravel-herd`, `REVERB_APP_SECRET=secret`.
- Herd symlinks the `mysql` and `mariadb` clients into PATH.
- There is **no** Horizon, Memcached, Elasticsearch/OpenSearch, RabbitMQ or SQL Server service.

**Per-site services (Pro):**
- One dropdown per Laravel app category:
  - DB: SQLite, MySQL, MariaDB, PG, Mongo.
  - Queue: sync, database, Redis, Valkey.
  - Cache: array, database, file, Redis, Valkey.
  - Broadcasting: log, null, Reverb.
- Picking a service that is not installed installs it first.
- Herd rewrites only that category's `.env` keys and backs up the originals to `.env.herd-services.bak`. "Not managed by Herd" restores them.
- CLI: `herd site:services`.

### 2.2 Mail
- **Server:** an SMTP catcher on `127.0.0.1:2525` (configurable `smtpPort` in `config/herd.json`). The binary is `Resources/mailserver-{arm64,x64}`, which looks like a Node single-executable app based on its strings [local].
- **Mailboxes:** the SMTP **username** becomes the mailbox, so `MAIL_USERNAME="${APP_NAME}"` gives one inbox per app.
- **Storage:** messages go to `HerdCoreData.sqlite` (Core Data) and attachments to `~/Library/Application Support/Herd/Log/mail/<mail>/`.
- **UI:** a native mail client with a header sidebar, HTML source and raw source windows, and delete or clear per inbox.
- **CLI and setup:** `herd mail:*` (see 1.11). Laravel installs from `laravel new` or the wizard get the `.env` filled in automatically.

### 2.3 Dumps and Insights
- **Hook:** `php.ini` sets `auto_prepend_file=dump-loader.php` [local].
  - The loader reads `config/herd.json`.
  - If interception is on, it includes `dump.phar`, class-aliases `Symfony\Component\VarDumper\VarDumper` to `Herd\HerdDumper\VarDumper`, and defines global `dump()`/`dd()`.
  - It chains the user's own `herd_auto_prepend_file` ini value.
- **Herd extension:** `herd-ext/herd-84-arm64.so` (functions `herd_inject_cb`, `__herd_closure`) injects watchers early in bootstrapping. The watchers are QueryWatcher, LogWatcher, JobWatcher, HttpClientWatcher, CacheWatcher and ViewWatcher, plus events.
- **Transport:** payloads go out as **HTTP POST (curl, form-encoded) to `http://127.0.0.1:2304/dumps`** (`apiPort`, default 2304), tagged with `request_id` and `dump_id` [local, `dump.phar`]. The `?herd=preview` request is ignored.
- **Watcher toggles in `herd.json`:** `watchDumps`, `watchQueries`, `watchLogs`, `watchHttpClient`, `watchViews`, `watchCache`, `watchEvents`, `watchViaCLI`, `minimumQueryDuration`, `showNewDumpsOnTop`, and per-tab colors.
- **Herd 2 Insights window:** tabs for Dumps, Queries, Jobs, HTTP, Logs and Cache. There is an antenna toggle, and history can be kept per site.
- **Analyzer:** finds N+1 queries (`nPlusOneMinRepeats` 3), duplicate queries (3), slow queries (100 ms), errors, and slow requests (1000 ms). It is configurable under `insights` in `herd.json`. CLI and agents use `insights:*`.
- **Not captured:** CLI scripts are only captured with "Include php CLI commands", and tests are never captured.

### 2.4 Xdebug detection
- **Routing:** nginx with njs (`config/pro/nginx/debug.conf` plus `njs/detect.js`) picks the FPM socket for each request [local]. It routes to `herd-debugXX.sock` (the FPM pool with `debug.ini` = Xdebug `mode=debug,develop`, `start_with_request=yes`) when either:
  - a cookie `XDEBUG_SESSION`, `XDEBUG_PROFILE` or `XDEBUG_TRACE` is present (browser helper extension), or
  - `herd.json` has `debug:true`, or the site's `.idea/workspace.xml` contains `<breakpoint-manager>` (PhpStorm breakpoints saved).
- **Everything else** goes to the fast non-Xdebug pool.
- **CLI:** `herd debug:start` and `herd debug:stop` run a capture session.

### 2.5 Log Viewer
- **Window:** a per-project sidebar, live tailing, an "All files" merged timeline, level filters, grouping of identical entries, and "Hide entries like this".
- **Search:** cross-file search, plus correlation-id search (`request_id`, `trace_id`, `job_id`...).
- **Entry detail:** stack trace with clickable editor links and collapsed vendor frames, copy as Markdown, and "Fix with AI".
- **Keys:** `j`/`k`/`e`/`/`.
- **Requirements:** PHP 8.1+. A custom driver can set `logFilesPaths()`. The bundled parser is `Resources/logs.phar`.

### 2.6 Other Pro features
- Tinker tab in Herd Studio.
- Runtime context for agents (Insights CLI and MCP).
- Service install from the new-app wizard.
- `herd.yml` services.
- Email support.

---

## 3. Architecture notes (from the local 1.26.1 install)

| Item | Detail |
|---|---|
| App | Native Swift/SwiftUI app at `/Applications/Herd.app`, bundle id `de.beyondco.herd` (Beyond Code). Libraries: Alamofire, Sentry, Aptabase analytics, CodeMirror, ZIPFoundation, KeyboardShortcuts. Core Data store at `HerdCoreData.sqlite`. |
| CLI | `herd.phar` is a **fork of Laravel Valet**: `Resources/valet/cli/Valet/{Site,Nginx,DnsMasq,PhpFpm,Configuration,Manifest,Herd,Expose}.php`, plus `Brew.php` vestigial. Stubs: `secure.valet.conf`, `proxy.valet.conf`, `openssl.conf`. CLI and app share `config/herd.json`. Many Herd 2 commands call the app's local API. |
| nginx | `Resources/nginx-arm64` is **nginx 1.25.4** (OpenSSL 3.2.1, http_v2, realip, **njs 0.8.3**), built by Herd. Config lives in `config/nginx/{nginx.conf,herd.conf}`. It listens on `127.0.0.1:80` and `:443`. Every request is rewritten to `Resources/valet/server.php`, the Valet front controller that resolves the site from the Host header and the parked/linked paths. Per-site confs go in `config/valet/Nginx/<site>.test`. Error pages are in `config/error_pages`. |
| DNS | `Resources/dnsmasq-arm64` (**dnsmasq 2.89**) uses `config/dnsmasq/dnsmasq.conf` = `address=/.test/127.0.0.1` and `listen-address=127.0.0.1` on port 53. **`/etc/resolver/test`** contains `nameserver 127.0.0.1`, which is macOS per-domain resolution. |
| TLS | A Valet-style CA in `config/valet/CA/LaravelValetCASelfSigned.{pem,key,srl}`, valid 20 years. It is trusted with `sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain`. Per-site leaf certs are RSA-2048 with SAN `site.test` and `*.site.test`, valid 396 days, stored in `config/valet/Certificates/<site>.{crt,key,csr,conf}`. 1.29 added X509v3 extensions. |
| PHP-FPM | `bin/php84-fpm` with `config/fpm/8.4-fpm.conf` and `8.4-fpm-debug.conf`. Unix sockets `herd84.sock` and `herd-debug84.sock`. `herd.sock` is a symlink to the global version's socket. |
| Privilege | A privileged helper `de.beyondco.herd.helper` (SMJobBless, root LaunchDaemon `/Library/LaunchDaemons/de.beyondco.herd.helper.plist`, binary in `/Library/PrivilegedHelperTools/`, XPC Mach service) runs nginx and dnsmasq as root (ports 80, 443, 53, and the trust store). There is also `/etc/sudoers.d/herd` (root-only), which lets the CLI run specific commands with sudo [contents unverified]. The installer asks for admin once during onboarding. |
| Shell | Herd appends to `~/.zshrc`: PATH for `Herd/bin`, `NVM_DIR`, and a source of `Resources/config/shell/zshrc.zsh` (an nvm `.nvmrc` chpwd hook). |
| Paths | `~/Library/Application Support/Herd/`: `bin/` (php*, composer, laravel, expose, forge, herd shims, `tools/`), `config/{nginx,dnsmasq,fpm,php/<ver>,valet,nvm,services,certificates,error_pages,herd.json}`, `Log/` (nginx-error.log, php-fpm.log, mail/), `.pid/`, `herd-ai-widget/`, `devtools/`. `config/valet/config.json` holds `{tld, loopback, paths[], share-tool}`. |
| Updates | Herd uses its own updater. Endpoints: `herdphp.com/api/php-update/`, `herdphp.com/api/resource-update/laravel/`, `herdphp.com/api/licenses`, `download.herdphp.com/...` [local strings; exact PHP zip URL pattern unverified]. |
| Ports | 80/443 nginx; 53 dnsmasq; **2525** SMTP; **2304** Herd app API (dumps/insights); 4421 devtools helper and 4422 opencode (Herd 2); service defaults as in the table above; MongoDB **27020**; RustFS console = port+1; Reverb 8080; 9912 = Symfony var-dump-server default (present in the phar). `localhost:8969/stream` appears in the binary, purpose unknown [unverified]. |
| Valet migration | On first run Herd imports `~/.config/valet` sites and certs and asks you to stop Valet. |
| Uninstall | `Resources/uninstall` script. Manual steps: remove `/etc/sudoers.d/herd`, `/etc/resolver/test`, the app support dir, zshrc lines, and `defaults delete de.beyondco.herd`. |

---

## 4. Free and open-source binaries Pack can download (macOS arm64 and x86_64, no Homebrew)

I checked the status codes with `curl -I`/`-L` on 2026-10-21. "✅" means HTTP 200 on the exact URL shown.

### 4.1 Replacements for Pro services

| Herd service | Pack alternative | URL pattern (✅ = verified) | License | Notes |
|---|---|---|---|---|
| Mail | **Mailpit** v1.31.6 | ✅ `https://github.com/axllent/mailpit/releases/download/v{V}/mailpit-darwin-{arm64,amd64}.tar.gz` (also ✅ `/releases/latest/download/...`) | MIT | SMTP `:1025` and web/API `:8025` by default (`--smtp :2525` to match Herd). Has a REST API for `mail:*`-style CLI commands, tagging by SMTP username (`--tag`/auth), and a webhook. Can replace all of Herd Mail. |
| PostgreSQL | **theseus-rs/postgresql-binaries** 18.6.1 | ✅ `https://github.com/theseus-rs/postgresql-binaries/releases/download/{V}/postgresql-{V}-{aarch64,x86_64}-apple-darwin.tar.gz` (+ `.sha256`) | PostgreSQL License (MIT-like) | Tags cover 14.18–18.6 (14/15/16/17/18 all present). **No PostGIS, pgvector or pgrouting.** Pack would have to build pgvector itself (easy, needs only `pg_config`). PostGIS is hard (GEOS/PROJ/GDAL). |
| MySQL | **Oracle MySQL Community tarballs** | ✅ `https://cdn.mysql.com/Downloads/MySQL-8.4/mysql-8.4.11-macos15-{arm64,x86_64}.tar.gz`; ✅ `.../MySQL-9.7/mysql-9.7.2-macos15-arm64.tar.gz` | GPLv2 (+FOSS exception) | Pattern: `MySQL-{major.minor}/mysql-{V}-macos{15}-{arch}.tar.gz`. Older patches move to `downloads.mysql.com/archives/get/p/23/file/...` [unverified]. The tarballs are self-contained (`bin/mysqld`, `mysql`, `mysqldump`). Initialize with `mysqld --initialize-insecure`. |
| MariaDB | **No official macOS binaries.** Options: (a) build in Pack CI and host on Pack's GitHub Releases (Herd does this too); (b) Homebrew bottles via GHCR without brew (see the note below), which link to `/opt/homebrew` dylibs and need `install_name_tool` relocation | n/a | GPLv2 | Lowest priority; MySQL covers most needs. |
| Redis | **Valkey** (preferred) or Redis built from source | Valkey publishes **Linux-only** tarballs (✅ `https://download.valkey.io/releases/valkey-9.1.2-noble-arm64.tar.gz`; there is no macOS artifact, and the guess `...-macos-arm64.tar.gz` gives 403) | Valkey BSD-3; Redis 8 is tri-licensed AGPLv3/RSALv2/SSPL | **Build in Pack CI**: `make BUILD_TLS=no` has zero deps and makes a universal binary via `lipo`. Herd ships `valkey/*-universal.zip` itself. Fallback: GHCR bottles (`ghcr.io/v2/homebrew/core/valkey`, ✅ anonymous token works; tags 7.2.4…9.1.2), but those are **arm64 only** (no x86_64 macOS bottle in 9.1.2). |
| MongoDB | **MongoDB Community** | ✅ `https://fastdl.mongodb.org/osx/mongodb-macos-{arm64,x86_64}-{V}.tgz` (7.0.28, 8.0.17, 8.2.3 checked) | **SSPL** (free to use and redistribute unmodified; not OSI) | Herd's default port is 27020. OSI alternative: **FerretDB** v2 (Apache-2.0) runs on Postgres plus the DocumentDB extension, but its latest release (v2.7.0, 2025-11) has no macOS assets; Docker or a build would be needed. |
| Meilisearch | **Meilisearch CE** v1.55.0 | ✅ `https://github.com/meilisearch/meilisearch/releases/download/v{V}/meilisearch-macos-{apple-silicon,amd64}` (raw binary) | MIT (CE). The `meilisearch-enterprise-*` assets are BUSL, so avoid them | `--master-key`, `--http-addr 127.0.0.1:7700`, `--db-path`. |
| Typesense | **Typesense** | ✅ `https://dl.typesense.org/releases/{V}/typesense-server-{V}-darwin-{arm64,amd64}.tar.gz` (30.2, 30.1) | GPL-3.0 | `--data-dir --api-key --api-port 8108`. |
| MinIO | **Do not use.** `dl.min.io/server/minio/release/darwin-{arm64,amd64}/minio` and `mc` both return **HTTP 410 Gone**. The `minio/minio` GitHub repo is **archived** (last tag RELEASE.2025-10-15). MinIO stopped community binary distribution in 2025 and went source-only/AGPL, then archived. | ❌ | AGPLv3 | Herd still ships a frozen RELEASE.2025-09-07 build and added RustFS in 1.27. |
| S3 storage | **RustFS** 1.0.3 (what Herd now offers) | ✅ `https://github.com/rustfs/rustfs/releases/download/{V}/rustfs-macos-aarch64-v{V}.zip` (and `-latest.zip`) | Apache-2.0 | **arm64 only on macOS** (no x86_64 darwin asset). The console runs on port+1. Defaults in Herd: `herd`/`secretkey`. |
| S3 storage (x86 fallback) | **SeaweedFS** 4.48 (`weed server -s3` or `weed mini`) | ✅ `https://github.com/seaweedfs/seaweedfs/releases/download/{V}/darwin_{arm64,amd64}.tar.gz` | Apache-2.0 | Single Go binary with an S3 gateway on :8333. |
| S3 storage | Garage | Official binaries are Linux musl only [unverified for macOS] | AGPL-3.0 | Not recommended for Pack. |
| Reverb | **laravel/reverb** via Composer (`php artisan reverb:start`) on Pack's PHP | composer package | MIT | Herd's "service" is a small Laravel app plus dashboard. Pack can ship the same thing or just proxy the project's own `reverb:start`. Pusher-compatible alternative: soketi (Node) [license unverified]. |
| Insights/Dumps | Own implementation: `auto_prepend_file` loader plus a VarDumper alias, POSTing to a local Pack HTTP endpoint (the same design as Herd). Alternatives: Symfony `var-dump-server` (MIT, `VAR_DUMPER_FORMAT=server`, `127.0.0.1:9912`), Spatie Ray protocol (port 23517, client MIT), Buggregator server (MIT, PHP/RoadRunner) | — | MIT | A Laravel-only watcher set can use `DB::listen`, `Event::listen`, and `Http` client events with no C extension. |
| Xdebug detection | nginx+njs (as Herd does) or Caddy matchers on the `XDEBUG_SESSION` cookie that route to a second FPM pool with Xdebug | — | — | Xdebug `.so` has **no official macOS binaries**; build it in CI against each PHP minor (`phpize` + make) or use static-php-cli shared-extension builds [unverified]. Xdebug License (BSD-style). |
| Profiler | **SPX** (`NoiseByNorthwest/php-spx`, GPL-3.0) or the `beyondcode/php-spx` fork | build per PHP | GPL-3.0 | Herd's profiler is free anyway. |
| Log viewer | Own UI; opcodesio/log-viewer (MIT) is a reference parser | — | — | — |
| DB GUI | **Adminer** 6.1.2 | ✅ `https://github.com/vrana/adminer/releases/download/v{V}/adminer-{V}.php` | Apache-2.0 / GPL-2.0 | Herd uses AdminerEvo at `database.herd.test`. |

### 4.2 Free core equivalents (what Herd Free bundles)

| Component | Pack source | URL (✅ = verified) | License | Notes |
|---|---|---|---|---|
| PHP CLI+FPM | **static-php-cli prebuilt** (`dl.static-php.dev`) | ✅ listing `https://dl.static-php.dev/static-php-cli/{common,bulk,minimal}/php-{V}-{cli,fpm}-macos-{aarch64,x86_64}.tar.gz` (8.0–8.5 present, e.g. 8.4.23, 8.5.8) | PHP License; spc MIT | `bulk` is the closest match to Herd's extension set. 7.4 and 8.6 are not in the prebuilt dirs. Pack would need to build those with `spc` (✅ `.../static-php-cli/releases/download/2.8.5/spc-macos-{aarch64,x86_64}.tar.gz`). Static builds may not `dlopen` Xdebug/SPX [unverified]. |
| Alternative web server | **Caddy** v2.11.7 | ✅ `https://github.com/caddyserver/caddy/releases/download/v{V}/caddy_{V}_mac_{arm64,amd64}.tar.gz` | Apache-2.0 | Replaces nginx and openssl/CA handling. `tls internal` plus `caddy trust` installs a local CA into the System keychain. Has an admin API for hot reload. nginx has **no official macOS binaries**. |
| PHP server (alt) | **FrankenPHP** v1.13.1 | ✅ `https://github.com/php/frankenphp/releases/download/v{V}/frankenphp-mac-{arm64,x86_64}` | MIT | Caddy with embedded PHP. Single version per binary, so per-site isolation is harder. |
| DNS | **No dnsmasq needed.** `/etc/resolver/test` supports `nameserver 127.0.0.1` **and `port 5300`**, so Pack can run a tiny built-in DNS responder on an unprivileged port | — | — | dnsmasq has no official macOS binaries. |
| Tunnels (Expose alt) | **cloudflared** quick tunnels | ✅ `https://github.com/cloudflare/cloudflared/releases/download/{V}/cloudflared-darwin-{arm64,amd64}.tgz` (also `/latest/download/`) | Apache-2.0 | `cloudflared tunnel --url http://127.0.0.1:80 --http-host-header site.test` gives `*.trycloudflare.com`, with no account needed. Expose's client itself is MIT (beyondcode/expose) and self-hostable. |
| Node | nodejs.org dist | ✅ `https://nodejs.org/dist/v{V}/node-v{V}-darwin-{arm64,x64}.tar.gz` (v24.22.0 checked); version index `https://nodejs.org/dist/index.json` | MIT | Pack can manage Node versions directly instead of bundling nvm. |
| Composer | getcomposer.org | ✅ `https://getcomposer.org/download/latest-stable/composer.phar` (also `/download/{V}/composer.phar`) | MIT | — |
| Laravel installer | `composer global require laravel/installer` (v5.32.0); there is no phar release asset | — | MIT | — |

**Pulling Homebrew bottles without brew** (fallback for MariaDB, Valkey and Redis):
1. Get an anonymous token: `GET https://ghcr.io/token?scope=repository:homebrew/core/<formula>:pull`.
2. Fetch the manifest: `GET https://ghcr.io/v2/homebrew/core/<formula>/manifests/<ver>` with `Accept: application/vnd.oci.image.index.v1+json`. This lists `arm64_{sonoma,sequoia,tahoe,golden_gate}` and `x86_64_linux` bottles.
3. Fetch the blob by digest.

Bottles use `@@HOMEBREW_PREFIX@@` placeholders and depend on other formula dylibs (e.g. openssl@3), so prefer building in your own CI.

### 4.3 Privilege notes for Pack
- I tested this on this Mac (macOS 26). An unprivileged process **can bind `0.0.0.0:80/443`** but **cannot bind `127.0.0.1:<1024` or `::1:<1024`** (EACCES).
- So Pack can serve without root either by:
  - binding 0.0.0.0 and rejecting non-loopback peers or firewalling them, or
  - running a small privileged helper (SMAppService LaunchDaemon) only for the 127.0.0.1:80/443 listener, `/etc/resolver/<tld>` and the keychain trust.
- `/etc/resolver/*` and System keychain trust always need admin once.
