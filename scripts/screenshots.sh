#!/usr/bin/env bash
# Takes the README screenshots: sets up Pack with demo sites, services, dumps
# and mail in a throwaway home (like smoke.sh: own PACK_HOME, ports 8080/8443,
# no DNS/certificate/PATH changes), then has the desktop app render its pages
# offscreen in light and dark (`pack-app --screenshots`, which needs
# `cargo build -p pack-app --features screenshots`).
#
# Usage: scripts/screenshots.sh path/to/pack path/to/pack-app [out-dir, default docs/screenshots]
set -euo pipefail

abs() { echo "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
pack=$(abs "$1")
app=$(abs "$2")
out=${3:-docs/screenshots}
mkdir -p "$out"
export PACK_HOME=$(mktemp -d)
# Pack shows site paths relative to the home folder, so give it a clean one: ~/Code/shop.
demo=$(mktemp -d)
code="$demo/Code"
p() { HOME="$demo" "$pack" "$@"; }
echo "PACK_HOME=$PACK_HOME"

cat > "$PACK_HOME/config.json" <<'EOF'
{ "http_port": 8080, "https_port": 8443, "dns_port": 53599, "paths": [] }
EOF

cleanup() {
  curl -s -m 5 -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:7171/api/shutdown >/dev/null || true
  sleep 2
  echo "--- daemon log"; tail -n 40 "$PACK_HOME/logs/daemon.log" 2>/dev/null || true
}
trap cleanup EXIT

p install --no-agent --no-system --minimal
p php install 8.3
for kind in mysql postgres redis meilisearch mail; do p service add "$kind"; done

# Demo projects: a Laravel shop that dumps, a WordPress blog, an API, static
# docs, an old PHP site pinned to 8.3 over plain HTTP, and a Vite dev server.
mkdir -p "$code"/{shop/public,shop/app/Http/Controllers,api/public,blog,docs/dist,legacy}
touch "$code/shop/artisan" "$code/api/artisan"
cat > "$code/shop/public/index.php" <<'EOF'
<?php
require __DIR__.'/../vendor/autoload.php';
require __DIR__.'/../app/Http/Controllers/OrderController.php';
(new App\Http\Controllers\OrderController)(parse_url($_SERVER['REQUEST_URI'], PHP_URL_PATH));
EOF
cat > "$code/shop/app/Http/Controllers/OrderController.php" <<'EOF'
<?php
namespace App\Http\Controllers;

class OrderController
{
    public function __invoke(string $path): void
    {
        if ($path === '/checkout') {
            dump(['coupon' => 'AUTUMN10', 'subtotal' => 54.0, 'discount' => 5.4, 'total' => 48.6]);
        } else {
            dump([
                'id' => 1042,
                'customer' => 'Ada Lovelace',
                'items' => [
                    ['sku' => 'TEE-BLK-M', 'qty' => 2, 'price' => 24.0],
                    ['sku' => 'MUG-ORANGE', 'qty' => 1, 'price' => 6.0],
                ],
                'paid' => true,
            ]);
        }
        echo 'ok';
    }
}
EOF
echo '<?php echo "api";' > "$code/api/public/index.php"
echo '<?php // demo' > "$code/blog/wp-config.php"
echo '<?php echo "blog";' > "$code/blog/index.php"
echo '<h1>Docs</h1>' > "$code/docs/dist/index.html"
echo '<?php echo "legacy";' > "$code/legacy/index.php"
(cd "$code/shop" && "$PACK_HOME/bin/composer" require --no-interaction --quiet symfony/var-dumper)

p park "$code"
p isolate 8.3 --site legacy
p unsecure legacy
p proxy vite 127.0.0.1:5173
p db shop
sleep 2

# Dumps come from real requests, so they carry the site, URL and file:line.
for path in /orders/1042 /checkout; do
  curl -sS -k -m 30 -o /dev/null --resolve "shop.test:8443:127.0.0.1" "https://shop.test:8443$path"
done

py=$(command -v python3 || command -v python)
mail_port=$(curl -s http://127.0.0.1:7171/api/services | "$py" -c "import json,sys; print(next(i['port'] for i in json.load(sys.stdin)['instances'] if i['kind']=='mail'))")
"$py" - "$mail_port" <<'EOF'
import smtplib, sys
from email.message import EmailMessage
mails = [
    ("Welcome to Shop, Ada!", "Thanks for signing up."),
    ("Reset your password", "Click the link below to choose a new password."),
    ("Your order #1042 has shipped", "It's on its way and should arrive on Friday."),
]
with smtplib.SMTP("127.0.0.1", int(sys.argv[1])) as s:
    for subject, body in mails:
        m = EmailMessage()
        m["From"], m["To"], m["Subject"] = "Shop <hello@shop.test>", "ada@example.com", subject
        m.set_content(body)
        s.send_message(m)
EOF

# Never publish screenshots of a service that failed to start.
down=$(curl -s http://127.0.0.1:7171/api/services | "$py" -c "import json,sys; print(' '.join(i['id'] for i in json.load(sys.stdin)['instances'] if (i['status'] or {}).get('state') != 'running'))")
[ -z "$down" ] || { echo "Not running: $down"; exit 1; }

"$app" --screenshots "$out" overview sites services dumps mail
echo "SCREENSHOTS OK"
