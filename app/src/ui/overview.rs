use super::widgets::*;
use super::{Page, Palette, PackApp};
use crate::api::Status;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

impl PackApp {
    pub(super) fn render_overview(&mut self, s: &Status, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let sites = self.sites.as_ref().map(|d| d.sites.clone()).unwrap_or_default();
        let services = self.services.as_ref().map(|d| d.instances.clone()).unwrap_or_default();
        let updates: Vec<_> = self.packages.as_ref().map(|d| d.installed.iter().filter(|p| p.update).cloned().collect()).unwrap_or_default();
        let setup_needed = !s.caddy.installed || !s.dns.resolver || !s.dns.low_ports || !s.ca_trusted || !s.php.iter().all(|p| p.installed);

        let mut page = v_flex_page().child(header("Overview", "Your local PHP development environment.", cx)).children(jobs(s, cx));
        if setup_needed {
            let ask = if s.os == "windows" { "for permission" } else { "once for your password" };
            page = page.child(
                banner(p.accent, cx).child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(
                            div().flex_1().child(bold("Finish setting up Pack")).child(muted(
                                format!(
                                    "Installs the web server, PHP {}, Composer, MySQL, PostgreSQL, Redis and Mail, then asks {ask} to route .{} domains and trust HTTPS.",
                                    s.default_php, s.tld
                                ),
                                cx,
                            )),
                        )
                        .child(self.setup_button("Set up", cx)),
                ),
            );
        }
        if !updates.is_empty() {
            let list = updates.iter().map(|u| format!("{} {} → {}", u.name, u.version, u.latest.clone().unwrap_or_default())).collect::<Vec<_>>().join(" · ");
            let n = updates.len();
            page = page.child(
                banner(p.ok, cx).child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(div().flex_1().child(bold(format!("{n} update{} available", if n > 1 { "s" } else { "" }))).child(muted(list, cx)))
                        .child(
                            Button::new("update-all").primary().label("Update all").loading(self.is_busy("update-all")).on_click(cx.listener(
                                |this, _, window, cx| this.post("update-all", "/api/packages/update".into(), json!({}), "Updating…", window, cx),
                            )),
                        ),
                ),
            );
        }

        // Status tiles.
        let mut tiles = vec![
            tile(dot(&s.caddy.status, cx), "Web server", muted(if s.caddy.installed { "Caddy" } else { "Caddy (not installed)" }, cx), cx),
            tile(
                dot_color(if s.dns.resolver { p.ok } else { p.bad }),
                &format!(".{} domains", s.tld),
                muted(if s.dns.resolver { format!("DNS on port {}", s.dns.port) } else { "resolver not installed".into() }, cx),
                cx,
            ),
            tile(
                dot_color(if s.ca_trusted { p.ok } else { p.warn }),
                "HTTPS",
                if s.ca_trusted {
                    muted("Certificate trusted — sites are secure", cx)
                } else {
                    div().child(self.setup_link(cx))
                },
                cx,
            ),
        ];
        for php in &s.php {
            let title = format!("PHP {}", php.version);
            let mut t = tile(dot(&php.status, cx), &title, muted(if php.installed { "FPM pool" } else { "not installed" }, cx), cx);
            if php.version == s.default_php {
                t = t.child(div().absolute().top_3().right_3().child(accent_tag("default", cx)));
            }
            tiles.push(t);
        }
        page = page.child(grid(tiles));

        // Sites and services.
        let site_rows: Vec<AnyElement> = sites
            .iter()
            .take(6)
            .enumerate()
            .map(|(i, x)| {
                h_flex()
                    .gap_2()
                    .mt_2()
                    .child(lock_icon(x.secured, p))
                    .child(link(("ov-site", i), x.host.clone(), x.url.clone(), cx))
                    .child(tag(x.framework.clone(), cx))
                    .into_any_element()
            })
            .collect();
        page = page.child(
            card(cx)
                .child(h_flex().justify_between().child(bold("Sites")).child(self.go_button("ov-sites", format!("{} sites", sites.len()), Page::Sites, cx)))
                .children(site_rows)
                .when(sites.is_empty(), |c| c.child(muted("No sites yet — park a folder or link a project.", cx))),
        );
        let service_rows: Vec<AnyElement> = services
            .iter()
            .map(|i| {
                h_flex()
                    .gap_2()
                    .mt_2()
                    .child(dot(&i.status, cx))
                    .child(bold(i.title()))
                    .child(mono(format!(":{}", i.port), cx).text_color(p.muted))
                    .into_any_element()
            })
            .collect();
        page.child(
            card(cx)
                .child(h_flex().justify_between().child(bold("Services")).child(self.go_button("ov-services", "Manage".into(), Page::Services, cx)))
                .children(service_rows)
                .when(services.is_empty(), |c| {
                    c.child(muted("No services. MySQL, Postgres, Redis, Meilisearch, Mail, S3 and more are one click away.", cx))
                }),
        )
        .into_any_element()
    }

    fn go_button(&self, id: &'static str, label: String, page: Page, cx: &mut Context<Self>) -> Button {
        Button::new(id).small().ghost().label(label).icon(IconName::ArrowRight).on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)))
    }

    pub(super) fn setup_button(&self, label: &'static str, cx: &mut Context<Self>) -> Button {
        let busy = self.is_busy("setup");
        Button::new(SharedString::from(format!("setup-{label}")))
            .primary()
            .label(if busy { "Setting up…" } else { label })
            .loading(busy)
            .on_click(cx.listener(|this, _, window, cx| this.post("setup", "/api/setup".into(), json!({}), "Pack is ready", window, cx)))
    }

    fn setup_link(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        div()
            .id("trust")
            .text_color(p.accent)
            .cursor_pointer()
            .hover(|s| s.underline())
            .child("Trust the certificate")
            .on_click(cx.listener(|this, _, window, cx| this.post("setup", "/api/setup".into(), json!({}), "Pack is ready", window, cx)))
    }
}

fn v_flex_page() -> Div {
    gpui_kit::component::v_flex().w_full().gap_3()
}

fn tile(light: Div, title: &str, body: Div, cx: &App) -> Div {
    card(cx).relative().gap_1().child(h_flex().gap_2().child(light).child(bold(title.to_string()))).child(body)
}

fn grid(tiles: Vec<Div>) -> impl IntoElement {
    div().w_full().grid().grid_cols(4).gap_3().children(tiles)
}

pub fn lock_icon(secured: bool, p: Palette) -> impl IntoElement {
    svg()
        .path(if secured { "icons/lock.svg" } else { "icons/lock-open.svg" })
        .size(px(13.))
        .flex_none()
        .text_color(if secured { p.ok } else { p.muted.opacity(0.6) })
}
