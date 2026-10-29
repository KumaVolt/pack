#!/usr/bin/env bash
# End-to-end check of a pack binary on this machine, without touching the
# system: own PACK_HOME, ports 8080/8443, no DNS/certificate/PATH changes.
# Installs the web server, PHP, MySQL, PostgreSQL and Redis from the manifest,
# serves a site that talks to all three, and checks it over HTTP and HTTPS.
#
# Usage: scripts/smoke.sh path/to/pack
set -euo pipefail

pack=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
export PACK_HOME=$(mktemp -d)
work=$(mktemp -d)
# Git Bash on Windows: hand native programs C:/... paths.
if command -v cygpath >/dev/null; then PACK_HOME=$(cygpath -m "$PACK_HOME"); work=$(cygpath -m "$work"); fi
py=$(command -v python3 || command -v python)
echo "PACK_HOME=$PACK_HOME"

cat > "$PACK_HOME/config.json" <<'EOF'
{ "http_port": 8080, "https_port": 8443, "dns_port": 53599, "paths": [] }
EOF

cleanup() {
  curl -s -m 5 -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:7171/api/shutdown >/dev/null || true
  sleep 2
  for log in "$PACK_HOME"/logs/*.log; do echo "--- $(basename "$log")"; tail -n 40 "$log"; done 2>/dev/null || true
}
trap cleanup EXIT

"$pack" install --no-agent --no-system --minimal
"$pack" service add mysql
"$pack" service add postgres
"$pack" service add redis

port() { curl -s http://127.0.0.1:7171/api/services | "$py" -c "import json,sys; print(next(i['port'] for i in json.load(sys.stdin)['instances'] if i['kind']=='$1'))"; }
mysql_port=$(port mysql)
pg_port=$(port postgres)
redis_port=$(port redis)
echo "mysql :$mysql_port  postgres :$pg_port  redis :$redis_port"

mkdir -p "$work/hello/public"
cat > "$work/hello/public/index.php" <<EOF
<?php
\$db = new PDO('mysql:host=127.0.0.1;port=$mysql_port', 'root', '');
\$mysql = \$db->query('select version()')->fetchColumn();
\$pg = (new PDO('pgsql:host=127.0.0.1;port=$pg_port;dbname=postgres', 'root', ''))->query('show server_version')->fetchColumn();
\$r = fsockopen('127.0.0.1', $redis_port);
fwrite(\$r, "PING\r\n");
\$redis = trim(fgets(\$r));
echo 'php=' . PHP_VERSION . ' mysql=' . \$mysql . ' pg=' . \$pg . ' redis=' . \$redis . ' ext=' . (extension_loaded('redis') ? 'redis' : 'none');
EOF
(cd "$work/hello" && "$pack" link hello)
sleep 2

fetch() { curl -sS -k -m 30 --resolve "hello.test:$2:127.0.0.1" "$1://hello.test:$2/"; }
out=$(fetch https 8443)
echo "https: $out"
echo "$out" | grep -q "php=8.4" || { echo "PHP did not answer over HTTPS"; exit 1; }
echo "$out" | grep -q "redis=+PONG" || { echo "Redis check failed"; exit 1; }
echo "$out" | grep -q "mysql=[0-9]" || { echo "MySQL check failed"; exit 1; }
echo "$out" | grep -q "pg=[0-9]" || { echo "PostgreSQL check failed"; exit 1; }
code=$(curl -s -o /dev/null -w '%{http_code}' -m 10 --resolve "hello.test:8080:127.0.0.1" http://hello.test:8080/)
echo "http: $code (expect a redirect to https)"
[ "$code" = 308 ] || [ "$code" = 301 ] || [ "$code" = 302 ] || { echo "HTTP did not redirect"; exit 1; }

# The php/composer shims.
ext=""; case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) ext=".cmd";; esac
"$PACK_HOME/bin/php$ext" -r 'echo "cli ", PHP_VERSION, "\n";'
"$PACK_HOME/bin/composer$ext" --version
echo "SMOKE OK"
