# Captures the Pack UI from the daemon on :7171 in light and dark mode.
# scripts/screenshots.sh sets up the demo data first.
#
# Usage: python3 scripts/screenshots.py OUT_DIR
import sys
from pathlib import Path

from playwright.sync_api import sync_playwright

BASE = "http://127.0.0.1:7171/"
PAGES = ["overview", "sites", "services", "dumps", "mail"]
# What has to be on screen before the shot, besides the page title.
READY = {"sites": "td a", "services": ".dot.running", "dumps": ".dump", "mail": "iframe.mail"}


def finished_setup(route):
    # The demo is installed with --no-system, so there is no .test DNS, low
    # ports or trusted certificate. Show the app as it looks after Set up.
    res = route.fetch()
    status = res.json()
    status["dns"].update(resolver=True, low_ports=True)
    status["ca_trusted"] = True
    route.fulfill(response=res, json=status)


def main(out):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        for scheme in ["light", "dark"]:
            ctx = browser.new_context(viewport={"width": 1280, "height": 800}, device_scale_factor=2,
                                      color_scheme=scheme, locale="en-US", timezone_id="Europe/Amsterdam")
            ctx.route("**/api/status", finished_setup)
            for name in PAGES:
                page = ctx.new_page()
                page.goto(f"{BASE}#{name}")
                page.wait_for_selector("main h1")
                if name in READY:
                    page.wait_for_selector(READY[name])
                if name == "mail":
                    page.frame_locator("iframe.mail").get_by_text("has shipped").first.wait_for()
                page.wait_for_load_state("networkidle")
                page.wait_for_timeout(500)
                file = out / (name + ("-dark" if scheme == "dark" else "") + ".png")
                page.screenshot(path=file)
                print(file)
                page.close()
            ctx.close()
        browser.close()


if __name__ == "__main__":
    main(sys.argv[1])
