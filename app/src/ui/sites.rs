use super::overview::lock_icon;
use super::widgets::*;
use super::{Page, Palette, PackApp};
use crate::api::{Site, Status};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

impl PackApp {
    pub(super) fn render_sites(&mut self, s: &Status, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let Some(d) = self.sites.clone() else { return starting(cx).into_any_element() };
        let versions: Vec<String> = self.php.as_ref().map(|p| p.installed.iter().map(|i| i.version.clone()).collect()).unwrap_or_default();

        let parked: Vec<AnyElement> = d
            .paths
            .iter()
            .zip(&d.paths_display)
            .enumerate()
            .map(|(i, (path, shown))| {
                let path = path.clone();
                h_flex()
                    .gap_1()
                    .pl_2()
                    .rounded_md()
                    .bg(p.ink.opacity(0.05))
                    .child(mono(shown.clone(), cx))
                    .child(
                        Button::new(("unpark", i)).ghost().xsmall().icon(IconName::X).tooltip("Stop serving this folder").on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.post(format!("unpark-{i}"), "/api/sites/unpark".into(), json!({ "path": path }), "Unparked", window, cx)
                            },
                        )),
                    )
                    .into_any_element()
            })
            .collect();

        let mut page = v_flex()
            .w_full()
            .gap_3()
            .child(header(
                "Sites",
                h_flex().gap_1().child("Folders in parked directories are served as").child(bold(format!("<folder>.{}", d.tld))),
                cx,
            ))
            .child(
                card(cx).child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_2()
                                .child(bold("Parked directories"))
                                .child(h_flex().flex_wrap().gap_2().children(parked).when(d.paths.is_empty(), |r| r.child(muted("none", cx)))),
                        )
                        .child(
                            h_flex().gap_2().child(div().w(px(200.)).child(Input::new(&self.inputs.park).small())).child(
                                Button::new("park").small().label("Park").loading(self.is_busy("park")).on_click(cx.listener(|this, _, window, cx| {
                                    let path = this.inputs.park.read(cx).value().to_string();
                                    this.post("park", "/api/sites/park".into(), json!({ "path": path }), "Parked", window, cx);
                                })),
                            ),
                        ),
                ),
            )
            .child(
                card(cx).child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(bold("Add site").mr_auto())
                        .child(div().w(px(220.)).child(Input::new(&self.inputs.link_path).small()))
                        .child(div().w(px(140.)).child(Input::new(&self.inputs.link_name).small()))
                        .child(Button::new("link").small().label("Link").loading(self.is_busy("link")).on_click(cx.listener(|this, _, window, cx| {
                            let path = this.inputs.link_path.read(cx).value().to_string();
                            let name = this.inputs.link_name.read(cx).value().to_string();
                            let name = (!name.is_empty()).then_some(name);
                            this.act(
                                "link",
                                move || crate::api::post("/api/sites/link", json!({ "path": path, "name": name })),
                                |r| format!("Linked {}", r["site"]["url"].as_str().unwrap_or("the site")),
                                window,
                                cx,
                            );
                        })))
                        .child(muted("or proxy", cx).mx_1())
                        .child(div().w(px(100.)).child(Input::new(&self.inputs.proxy_name).small()))
                        .child(div().w(px(130.)).child(Input::new(&self.inputs.proxy_upstream).small()))
                        .child(Button::new("proxy").small().label("Proxy").loading(self.is_busy("proxy")).on_click(cx.listener(|this, _, window, cx| {
                            let name = this.inputs.proxy_name.read(cx).value().to_string();
                            let upstream = this.inputs.proxy_upstream.read(cx).value().to_string();
                            this.post("proxy", "/api/sites/proxy".into(), json!({ "name": name, "upstream": upstream }), "Proxy added", window, cx);
                        }))),
                ),
            );
        if !s.ca_trusted {
            page = page.child(
                banner(p.accent, cx).child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(div().flex_1().child(bold("Trust Pack's HTTPS certificate")).child(muted(
                            format!("One click, so browsers trust every .{} site instead of showing a warning.", s.tld),
                            cx,
                        )))
                        .child(self.setup_button("Trust", cx)),
                ),
            );
        }

        let mut table = table(cx).child(
            head_row(cx)
                .child(div().flex_1().child("Site"))
                .child(div().w(px(64.)).child("HTTPS"))
                .child(div().w(px(150.)).child("PHP"))
                .child(div().w(px(170.)).child("Path"))
                .child(div().w(px(220.)).child("")),
        );
        for (i, site) in d.sites.iter().enumerate() {
            table = table.child(self.site_row(i, site, &versions, &s.default_php, cx));
        }
        if d.sites.is_empty() {
            table = table.child(row(cx).justify_center().py_8().child(muted("No sites yet.", cx)));
        }
        page.child(table).into_any_element()
    }

    fn site_row(&self, i: usize, x: &Site, versions: &[String], default_php: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let name = x.name.clone();
        let mut site = v_flex().flex_1().min_w_0().gap_0p5().child(
            h_flex()
                .gap_2()
                .child(lock_icon(x.secured, p))
                .child(link(("site", i), x.host.clone(), x.url.clone(), cx))
                .child(tag(x.framework.clone(), cx)),
        );
        if let Some(shared) = &x.shared {
            site = site.child(link(("shared", i), shared.clone(), shared.clone(), cx).text_xs().font_weight(FontWeight::NORMAL));
        }

        let secure = {
            let name = name.clone();
            let on = x.secured;
            Switch::new(("secure", i))
                .checked(on)
                .tooltip(if on { "Served over HTTPS — click for plain HTTP" } else { "Click to serve over HTTPS" })
                .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                    let msg = if *checked { "HTTPS on" } else { "HTTP only" };
                    this.post(format!("secure-{name}"), "/api/sites/secure".into(), json!({ "name": name, "secure": checked }), msg, window, cx);
                }))
        };

        let php = if x.proxy.is_some() {
            muted("—", cx).into_any_element()
        } else {
            let label = if x.isolated { x.php.clone() } else { format!("default ({default_php})") };
            let this = cx.entity().downgrade();
            let (name, isolated, current, versions, default_php) = (name.clone(), x.isolated, x.php.clone(), versions.to_vec(), default_php.to_string());
            Button::new(("php", i))
                .small()
                .outline()
                .label(label)
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    let choices = std::iter::once((None, format!("default ({default_php})"))).chain(versions.iter().map(|v| (Some(v.clone()), v.clone())));
                    for (version, label) in choices {
                        let checked = match &version {
                            None => !isolated,
                            Some(v) => isolated && *v == current,
                        };
                        let (this, name) = (this.clone(), name.clone());
                        menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(move |_, window, cx| {
                            let (name, version) = (name.clone(), version.clone());
                            this.update(cx, |app, cx| {
                                app.post(format!("php-{name}"), "/api/sites/isolate".into(), json!({ "name": name, "php": version }), "PHP updated", window, cx)
                            })
                            .ok();
                        }));
                    }
                    menu
                })
                .into_any_element()
        };

        let path = mono(x.proxy.as_ref().map(|u| format!("→ {u}")).unwrap_or_else(|| x.path_display.clone()), cx).text_color(p.muted).truncate();

        let mut actions = h_flex().gap_1().justify_end();
        actions = if x.shared.is_some() {
            let name = name.clone();
            actions.child(Button::new(("unshare", i)).small().label("Stop sharing").on_click(cx.listener(move |this, _, window, cx| {
                this.post(format!("share-{name}"), "/api/sites/unshare".into(), json!({ "name": name }), "Stopped sharing", window, cx)
            })))
        } else {
            let name = name.clone();
            let busy = self.is_busy(&format!("share-{name}"));
            actions.child(
                Button::new(("share", i))
                    .small()
                    .icon(IconName::Share2)
                    .label(if busy { "Starting tunnel…" } else { "Share" })
                    .loading(busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let n = name.clone();
                        this.act(
                            format!("share-{name}"),
                            move || crate::api::post("/api/sites/share", json!({ "name": n })),
                            |r| format!("Shared at {}", r["url"].as_str().unwrap_or("")),
                            window,
                            cx,
                        );
                    })),
            )
        };
        if x.proxy.is_none() {
            let name = name.clone();
            actions = actions.child(Button::new(("logs", i)).small().label("Logs").on_click(cx.listener(move |this, _, _, cx| {
                this.log_site = Some(name.clone());
                this.log_file = None;
                this.go(Page::Logs, cx);
            })));
        }
        if x.kind != "parked" {
            let name = name.clone();
            actions = actions.child(Button::new(("remove", i)).small().danger().label("Remove").on_click(cx.listener(move |this, _, window, cx| {
                this.post(format!("remove-{name}"), "/api/sites/unlink".into(), json!({ "name": name }), "Removed", window, cx)
            })));
        }

        row(cx)
            .child(site)
            .child(div().w(px(64.)).child(secure))
            .child(div().w(px(150.)).child(php))
            .child(div().w(px(170.)).min_w_0().child(path))
            .child(div().w(px(220.)).child(actions))
    }
}
