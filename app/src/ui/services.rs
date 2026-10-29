use super::widgets::*;
use super::{Palette, PackApp};
use crate::api::{self, Instance, Kind, Status, cmp_versions};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

impl PackApp {
    pub(super) fn render_services(&mut self, s: &Status, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(d) = self.services.clone() else { return starting(cx).into_any_element() };
        let instances: Vec<AnyElement> = d.instances.iter().enumerate().map(|(i, inst)| self.instance_card(i, inst, &d, cx).into_any_element()).collect();
        let kinds: Vec<AnyElement> = d.kinds.iter().enumerate().map(|(i, k)| self.kind_card(i, k, &d.lines(&k.id), cx).into_any_element()).collect();
        v_flex()
            .w_full()
            .gap_3()
            .child(header("Services", "Databases, caches, search, mail and storage — all free, open-source binaries.", cx))
            .children(jobs(s, cx))
            .children(instances)
            .child(div().mt_4().text_lg().font_weight(FontWeight::BOLD).child("Add a service"))
            .child(div().w_full().grid().grid_cols(3).gap_3().children(kinds))
            .into_any_element()
    }

    fn instance_card(&self, i: usize, inst: &Instance, d: &api::Services, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let state = api::state(&inst.status).to_string();
        let id = inst.id.clone();
        let mut title = h_flex()
            .gap_2()
            .flex_wrap()
            .child(dot(&inst.status, cx))
            .child(bold(inst.title()))
            .child(self.series_button(i, inst, &d.lines(&inst.kind), cx))
            .child(tag(inst.installed_version.clone().unwrap_or_else(|| "not installed".into()), cx));
        if let (Some(latest), Some(installed)) = (d.latest(&inst.kind, &inst.series), &inst.installed_version) {
            if cmp_versions(&latest, installed).is_gt() {
                let (kind, series, name) = (inst.kind.clone(), inst.series.clone(), inst.title());
                title = title.child(Button::new(("svc-update", i)).xsmall().primary().label(format!("Update to {latest}")).on_click(cx.listener(
                    move |this, _, window, cx| {
                        let body = json!({ "name": kind, "series": series });
                        this.post(format!("svc-update-{kind}"), "/api/packages/update".into(), body, format!("Updating {name}…"), window, cx)
                    },
                )));
            }
        }
        title = title.child(mono(format!("127.0.0.1:{}", inst.port), cx).text_color(p.muted));
        if let Some(url) = inst.dashboard() {
            title = title.child(
                Button::new(("svc-dash", i)).xsmall().ghost().icon(IconName::ExternalLink).label("Open dashboard").on_click(move |_, _, cx| cx.open_url(&url)),
            );
        }

        let mut actions = h_flex().gap_1().flex_none();
        if inst.kind == "mysql" || inst.kind == "postgres" {
            let (id, name) = (id.clone(), inst.title());
            actions = actions.child(
                Button::new(("svc-db", i)).small().icon(IconName::Database).label("New database").on_click(cx.listener(move |this, _, window, cx| {
                    this.new_database(id.clone(), name.clone(), window, cx)
                })),
            );
        }
        let busy = self.is_busy(&format!("svc-{id}"));
        actions = if state == "running" || state == "starting" {
            let id = id.clone();
            actions.child(Button::new(("svc-stop", i)).small().icon(IconName::Square).label("Stop").loading(busy).on_click(cx.listener(
                move |this, _, window, cx| this.post(format!("svc-{id}"), format!("/api/services/{id}/stop"), json!({}), "Stopped", window, cx),
            )))
        } else {
            let id = id.clone();
            actions.child(Button::new(("svc-start", i)).small().primary().icon(IconName::Play).label("Start").loading(busy).on_click(cx.listener(
                move |this, _, window, cx| this.post(format!("svc-{id}"), format!("/api/services/{id}/start"), json!({}), "Starting", window, cx),
            )))
        };
        let remove_id = id.clone();
        actions = actions.child(
            Button::new(("svc-remove", i))
                .small()
                .danger()
                .icon(IconName::Trash)
                .label("Remove")
                .on_click(cx.listener(move |this, _, window, cx| this.remove_service(remove_id.clone(), window, cx))),
        );

        let env_key = format!("env-{id}");
        let open = self.is_expanded(&env_key);
        let env = inst.env.clone();
        let mut card = card(cx).child(h_flex().justify_between().items_start().gap_3().child(title.flex_1()).child(actions));
        if state == "failed" {
            let detail = inst.status.as_ref().and_then(|s| s.detail.clone()).unwrap_or_default();
            card = card.child(pre(detail, cx).text_color(p.bad));
        }
        card.child(
            h_flex()
                .gap_2()
                .child(
                    div()
                        .id(("svc-env", i))
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_color(p.muted)
                        .cursor_pointer()
                        .child(gpui_kit::component::Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
                        .child(".env for Laravel")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle(env_key.clone(), cx))),
                )
                .when(open, |r| {
                    let env = env.clone();
                    r.child(Button::new(("svc-copy", i)).xsmall().ghost().icon(IconName::Copy).label("Copy").on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(env.clone()));
                        super::notify(window, cx, "Copied");
                    }))
                }),
        )
        .when(open, |c| c.child(pre(inst.env.clone(), cx)))
    }

    /// Version line picker; switching keeps the data and upgrades it in place.
    fn series_button(&self, i: usize, inst: &Instance, lines: &[String], cx: &mut Context<Self>) -> AnyElement {
        if lines.len() < 2 {
            return div().into_any_element();
        }
        let this = cx.entity().downgrade();
        let (id, current, lines) = (inst.id.clone(), inst.series.clone(), lines.to_vec());
        Button::new(("svc-series", i))
            .xsmall()
            .outline()
            .label(current.clone())
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                for line in &lines {
                    let (this, id, line) = (this.clone(), id.clone(), line.clone());
                    menu = menu.item(PopupMenuItem::new(line.clone()).checked(line == current).on_click(move |_, window, cx| {
                        if let Some(this) = this.upgrade() {
                            confirm_switch(this, id.clone(), line.clone(), window, cx);
                        }
                    }));
                }
                menu
            })
            .into_any_element()
    }

    fn kind_card(&self, i: usize, k: &Kind, lines: &[String], cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let mut ports = format!(":{}", k.port);
        if !lines.is_empty() {
            ports += &format!(" · {}", lines.join(", "));
        }
        let kind = k.clone();
        let lines = lines.to_vec();
        card(cx)
            .id(("kind", i))
            .cursor_pointer()
            .hover(move |s| s.border_color(p.accent))
            .gap_1()
            .child(h_flex().justify_between().child(bold(k.name.clone())).child(tag(k.license.clone(), cx)))
            .child(muted(k.description.clone(), cx))
            .child(mono(ports, cx).text_color(p.muted))
            .on_click(cx.listener(move |this, _, window, cx| this.add_service(kind.clone(), lines.clone(), window, cx)))
    }

    fn new_database(&mut self, id: String, service: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value("laravel"));
        input.update(cx, |i, cx| i.focus(window, cx));
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, input, id) = (this.clone(), input.clone(), id.clone());
            dialog.title(format!("New {service} database")).w(px(380.)).child(Input::new(&input)).footer(form_footer("Create", move |window, cx| {
                let name = input.read(cx).value().to_string();
                if name.is_empty() {
                    return false;
                }
                let (id, n) = (id.clone(), name.clone());
                this.update(cx, |app, cx| {
                    app.post(format!("svc-db-{id}"), format!("/api/services/{id}/databases"), json!({ "name": n }), format!("Created {name}"), window, cx)
                })
                .ok();
                true
            }))
        });
    }

    fn remove_service(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let remove = |data: bool| {
                let (this, id) = (this.clone(), id.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let id = id.clone();
                    window.close_dialog(cx);
                    this.update(cx, |app, cx| {
                        let path = format!("/api/services/{id}?data={data}");
                        app.act(format!("svc-{id}"), move || api::delete(&path), |_| "Removed".into(), window, cx)
                    })
                    .ok();
                }
            };
            dialog
                .title(format!("Remove {id}?"))
                .w(px(440.))
                .child("Its data is kept unless you also delete it.")
                .footer(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
                        .child(Button::new("remove-keep").label("Remove, keep data").on_click(remove(false)))
                        .child(Button::new("remove-data").danger().label("Remove and delete data").on_click(remove(true))),
                )
        });
    }

    fn add_service(&mut self, kind: Kind, lines: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.new(|cx| AddService::new(kind.clone(), lines, window, cx));
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, form) = (this.clone(), form.clone());
            dialog.title(format!("Add {}", kind.name)).w(px(420.)).child(form.clone()).footer(form_footer("Install", move |window, cx| {
                let Some(body) = form.read(cx).body(cx) else { return false };
                let name = form.read(cx).kind.name.clone();
                this.update(cx, |app, cx| {
                    app.act("add-service", move || api::post("/api/services", body), move |_| format!("Installing {name}…"), window, cx)
                })
                .ok();
                true
            }))
        });
    }
}

/// Cancel and OK buttons; `ok` returns whether to close the dialog.
fn form_footer(label: &'static str, ok: impl Fn(&mut Window, &mut App) -> bool + 'static) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_end()
        .gap_2()
        .child(Button::new("dialog-cancel").label("Cancel").on_click(|_, window, cx| window.close_dialog(cx)))
        .child(Button::new("dialog-ok").primary().label(label).on_click(move |_, window, cx| {
            if ok(window, cx) {
                window.close_dialog(cx);
            }
        }))
}

fn confirm_switch(this: Entity<PackApp>, id: String, line: String, window: &mut Window, cx: &mut App) {
    let this = this.downgrade();
    window.open_alert_dialog(cx, move |alert, _, _| {
        let (this, id, line) = (this.clone(), id.clone(), line.clone());
        alert
            .title(format!("Switch {id} to {line}?"))
            .description("Its data is kept and upgraded in place.")
            .show_cancel(true)
            .ok_text("Switch")
            .on_ok(move |_, window, cx| {
                let (id, line) = (id.clone(), line.clone());
                this.update(cx, |app, cx| {
                    let msg = format!("Switching {id} to {line}…");
                    app.post(format!("svc-{id}"), format!("/api/services/{id}/version"), json!({ "version": line }), msg, window, cx)
                })
                .ok();
                true
            })
    });
}

/// The form in the "Add a service" dialog.
struct AddService {
    kind: Kind,
    lines: Vec<String>,
    version: Option<String>,
    port: Entity<InputState>,
    /// Reverb runs inside a Laravel project.
    path: Option<Entity<InputState>>,
}

impl AddService {
    fn new(kind: Kind, lines: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let port = cx.new(|cx| InputState::new(window, cx).placeholder(format!("first free from {}", kind.port)));
        let path = (kind.id == "reverb").then(|| cx.new(|cx| InputState::new(window, cx).placeholder("~/Code/my-app")));
        AddService { version: lines.first().cloned(), kind, lines, port, path }
    }

    fn body(&self, cx: &App) -> Option<serde_json::Value> {
        let mut body = json!({ "kind": self.kind.id });
        if self.lines.len() > 1 {
            body["version"] = json!(self.version);
        }
        let port = self.port.read(cx).value();
        if !port.is_empty() {
            body["port"] = json!(port.parse::<u16>().ok()?);
        }
        if let Some(path) = &self.path {
            let path = path.read(cx).value();
            if path.is_empty() {
                return None;
            }
            body["path"] = json!(path.to_string());
        }
        Some(body)
    }
}

impl Render for AddService {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let label = |text: &str| div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.muted).child(text.to_string());
        let mut form = v_flex().gap_3().child(muted(self.kind.description.clone(), cx));
        if self.lines.len() > 1 {
            let buttons = self.lines.iter().enumerate().map(|(i, line)| {
                let line = line.clone();
                let selected = self.version.as_deref() == Some(line.as_str());
                let b = Button::new(("line", i)).small().label(line.clone());
                let b = if selected { b.primary() } else { b.outline() };
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.version = Some(line.clone());
                    cx.notify();
                }))
            });
            form = form.child(v_flex().gap_1().child(label("Version")).child(h_flex().gap_1().flex_wrap().children(buttons)));
        }
        form = form.child(v_flex().gap_1().child(label("Port")).child(Input::new(&self.port)));
        if let Some(path) = &self.path {
            form = form.child(v_flex().gap_1().child(label("Laravel project path")).child(Input::new(path)));
        }
        form
    }
}
