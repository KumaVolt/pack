//! A native inbox over Mailpit's API. HTML mail opens in the browser.

use super::widgets::*;
use super::{Palette, PackApp};
use crate::api::{self, Instance, Mail, MailSummary};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

fn when(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| t.with_timezone(&chrono::Local).format("%b %-d, %H:%M").to_string())
        .unwrap_or_default()
}

impl PackApp {
    pub(super) fn render_mail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(services) = self.services.clone() else { return starting(cx).into_any_element() };
        let sub = "Catch every email your apps send.";
        let Some(mail) = services.instances.iter().find(|i| i.kind == "mail").cloned() else {
            return v_flex()
                .w_full()
                .gap_3()
                .child(header("Mail", sub, cx))
                .child(empty("Mail isn't set up yet.", cx).gap_4().child(
                    Button::new("enable-mail").primary().label("Enable Mail").loading(self.is_busy("enable-mail")).on_click(cx.listener(
                        |this, _, window, cx| this.post("enable-mail", "/api/services".into(), json!({ "kind": "mail" }), "Installing Mailpit…", window, cx),
                    )),
                ))
                .into_any_element();
        };
        let state = api::state(&mail.status).to_string();
        if state != "running" {
            let id = mail.id.clone();
            return v_flex()
                .w_full()
                .gap_3()
                .child(header("Mail", sub, cx))
                .child(empty(format!("Mail is {state}."), cx).gap_4().child(
                    Button::new("start-mail").primary().label("Start").loading(self.is_busy("start-mail")).on_click(cx.listener(move |this, _, window, cx| {
                        this.post("start-mail", format!("/api/services/{id}/start"), json!({}), "Starting", window, cx)
                    })),
                ))
                .into_any_element();
        }

        let ui = mail.ui_port.map(|p| format!("http://127.0.0.1:{p}")).unwrap_or_default();
        let messages = match &self.inbox {
            Some(Ok(inbox)) => inbox.messages.clone(),
            Some(Err(e)) => return v_flex().w_full().gap_3().child(self.mail_header(&mail, &ui, cx)).child(empty(e.clone(), cx)).into_any_element(),
            None => Vec::new(),
        };
        if self.mail_open.as_ref().is_none_or(|id| !messages.iter().any(|m| &m.id == id)) {
            if let Some(first) = messages.first() {
                self.open_mail(first.id.clone(), &ui, cx);
            }
        }

        let p = Palette::get(cx);
        let mut rows = Vec::new();
        for (i, m) in messages.iter().enumerate() {
            rows.push(self.mail_row(i, m, &ui, cx).into_any_element());
        }
        let list = v_flex()
            .id("mail-list")
            .w(px(330.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(p.line)
            .children(rows)
            .when(messages.is_empty(), |l| l.child(div().p_6().text_color(p.muted).child("No mail yet. Send some from your app.")))
            .overflow_y_scrollbar();

        let body = match &self.mail {
            Some(Ok(m)) if self.mail_open.as_deref() == Some(m.id.as_str()) => self.mail_view(m, &ui, cx).into_any_element(),
            Some(Err(e)) => div().p_6().text_color(p.bad).child(e.clone()).into_any_element(),
            _ => div().into_any_element(),
        };

        v_flex()
            .w_full()
            .gap_3()
            .child(self.mail_header(&mail, &ui, cx))
            .child(
                h_flex()
                    .w_full()
                    .h(px(560.))
                    .items_start()
                    .rounded_xl()
                    .border_1()
                    .border_color(p.line)
                    .bg(p.card)
                    .overflow_hidden()
                    .child(list)
                    .child(div().id("mail-body").flex_1().h_full().overflow_y_scrollbar().child(body)),
            )
            .into_any_element()
    }

    fn mail_header(&self, mail: &Instance, ui: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let (open, clear) = (ui.to_string(), ui.to_string());
        header(
            "Mail",
            h_flex()
                .gap_1()
                .child("SMTP on")
                .child(mono(format!("127.0.0.1:{}", mail.port), cx))
                .child("— set")
                .child(mono(format!("MAIL_PORT={}", mail.port), cx)),
            cx,
        )
        .child(
            h_flex()
                .gap_2()
                .child(Button::new("mail-clear").icon(IconName::Trash).label("Delete all").on_click(cx.listener(move |this, _, window, cx| {
                    this.delete_all_mail(clear.clone(), window, cx)
                })))
                .child(Button::new("mail-open").icon(IconName::ExternalLink).label("Open in browser").on_click(move |_, _, cx| cx.open_url(&open))),
        )
    }

    fn mail_row(&self, i: usize, m: &MailSummary, ui: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let selected = self.mail_open.as_deref() == Some(m.id.as_str());
        let from = if m.from.name.is_empty() { m.from.address.clone() } else { m.from.name.clone() };
        let (id, ui) = (m.id.clone(), ui.to_string());
        v_flex()
            .id(("mail", i))
            .w_full()
            .px_4()
            .py_3()
            .gap_0p5()
            .border_b_1()
            .border_color(p.line)
            .cursor_pointer()
            .when(selected, |r| r.bg(p.accent_soft))
            .when(!selected, |r| r.hover(|s| s.bg(p.ink.opacity(0.03))))
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .min_w_0()
                            .when(!m.read, |r| r.child(dot_color(p.accent).size(px(7.))))
                            .child(div().truncate().font_weight(if m.read { FontWeight::NORMAL } else { FontWeight::SEMIBOLD }).child(from)),
                    )
                    .child(div().flex_none().text_xs().text_color(p.muted).child(when(&m.created))),
            )
            .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(m.subject.clone()))
            .child(div().truncate().text_xs().text_color(p.muted).child(m.snippet.clone()))
            .on_click(cx.listener(move |this, _, _, cx| this.open_mail(id.clone(), &ui, cx)))
    }

    fn mail_view(&self, m: &Mail, ui: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let url = format!("{ui}/view/{}", m.id);
        let to = m.to.iter().map(|a| a.display()).collect::<Vec<_>>().join(", ");
        let field = |name: &str, value: String| {
            h_flex().gap_2().child(div().w(px(44.)).text_color(p.muted).child(name.to_string())).child(div().flex_1().child(value))
        };
        let text = if m.text.trim().is_empty() { None } else { Some(m.text.clone()) };
        v_flex()
            .p_6()
            .gap_4()
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(m.subject.clone()))
                    .child(Button::new("mail-html").small().icon(IconName::ExternalLink).label("Open in browser").on_click(move |_, _, cx| cx.open_url(&url))),
            )
            .child(
                v_flex()
                    .gap_1()
                    .pb_4()
                    .border_b_1()
                    .border_color(p.line)
                    .child(field("From", m.from.display()))
                    .child(field("To", to))
                    .child(field("Date", when(&m.date))),
            )
            .child(match text {
                Some(text) => div().whitespace_normal().line_height(px(21.)).child(text).into_any_element(),
                None => muted("This message has no plain-text part. Open it in the browser to see the HTML.", cx).into_any_element(),
            })
    }

    fn open_mail(&mut self, id: String, ui: &str, cx: &mut Context<Self>) {
        self.mail_open = Some(id.clone());
        let url = format!("{ui}/api/v1/message/{id}");
        let task = cx.background_spawn(async move { api::get::<Mail>(&url) });
        cx.spawn(async move |this, cx| {
            let mail = task.await;
            this.update(cx, |this, cx| {
                if this.mail_open.as_deref() == Some(id.as_str()) {
                    // Mailpit marks it read when it is fetched.
                    if let Some(Ok(inbox)) = &mut this.inbox {
                        inbox.messages.iter_mut().filter(|m| m.id == id).for_each(|m| m.read = true);
                    }
                    this.mail = Some(mail);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn delete_all_mail(&mut self, ui: String, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let (this, ui) = (this.clone(), ui.clone());
            alert.title("Delete all mail?").description("Every message in the inbox is deleted.").show_cancel(true).ok_text("Delete all").on_ok(
                move |_, window, cx| {
                    let url = format!("{ui}/api/v1/messages");
                    this.update(cx, |app, cx| {
                        app.mail_open = None;
                        app.mail = None;
                        app.act("mail-clear", move || api::delete(&url), |_| "Inbox emptied".into(), window, cx)
                    })
                    .ok();
                    true
                },
            )
        });
    }
}
