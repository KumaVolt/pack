use super::widgets::*;
use super::{Palette, PackApp, SYSTEM_LOGS};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::*;

impl PackApp {
    pub(super) fn render_logs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let sites: Vec<String> = self.sites.as_ref().map(|d| d.sites.iter().filter(|s| s.proxy.is_none()).map(|s| s.name.clone()).collect()).unwrap_or_default();
        let current = self.log_site.clone();
        let system = current.as_deref() == Some(SYSTEM_LOGS);
        let label = match current.as_deref() {
            Some(SYSTEM_LOGS) => "Pack system logs".to_string(),
            Some(s) => s.to_string(),
            None => "choose a site".to_string(),
        };

        let this = cx.entity().downgrade();
        let current_site = current.clone();
        let site_picker = Button::new("log-site").outline().label(label).dropdown_caret(true).dropdown_menu(move |mut menu, _, _| {
            let choices = sites.iter().map(|s| (s.clone(), s.clone())).chain([(SYSTEM_LOGS.to_string(), "Pack system logs".to_string())]);
            for (i, (value, label)) in choices.enumerate() {
                if value == SYSTEM_LOGS && i > 0 {
                    menu = menu.separator();
                }
                let this = this.clone();
                let checked = current_site.as_deref() == Some(value.as_str());
                menu = menu.item(PopupMenuItem::new(label).checked(checked).on_click(move |_, _, cx| {
                    let value = value.clone();
                    this.update(cx, |app, cx| {
                        app.log_site = Some(value);
                        app.log_file = None;
                        app.logs = None;
                        app.sys_logs = None;
                        app.expanded.clear();
                        app.refresh(cx);
                    })
                    .ok();
                }));
            }
            menu
        });

        // Log file picker: the site's log files, or Pack's own.
        let (files, file) = if system {
            (self.sys_logs.as_ref().map(|d| d.files.clone()).unwrap_or_default(), self.sys_log.clone())
        } else {
            match &self.logs {
                Some(Ok(d)) => (d.files.clone(), d.file.clone()),
                _ => (Vec::new(), None),
            }
        };
        let file_picker = if files.len() > 1 || (system && !files.is_empty()) {
            let this = cx.entity().downgrade();
            let selected = file.clone();
            Button::new("log-file")
                .outline()
                .label(file.clone().unwrap_or_else(|| "choose a log…".into()))
                .dropdown_caret(true)
                .dropdown_menu(move |mut menu, _, _| {
                    for f in &files {
                        let (this, f) = (this.clone(), f.clone());
                        menu = menu.item(PopupMenuItem::new(f.clone()).checked(selected.as_deref() == Some(f.as_str())).on_click(move |_, _, cx| {
                            let f = f.clone();
                            this.update(cx, |app, cx| {
                                if system {
                                    app.sys_log = Some(f);
                                } else {
                                    app.log_file = Some(f);
                                }
                                app.expanded.clear();
                                app.refresh(cx);
                            })
                            .ok();
                        }));
                    }
                    menu
                })
                .into_any_element()
        } else {
            mono(file.unwrap_or_else(|| if system { String::new() } else { "no log files".into() }), cx).text_color(p.muted).into_any_element()
        };

        let body: AnyElement = if system {
            match (&self.sys_log, &self.sys_logs) {
                (Some(_), Some(d)) => {
                    let text = if d.text.is_empty() { "empty".to_string() } else { d.text.clone() };
                    pre(text, cx).bg(p.card).border_1().border_color(p.line).into_any_element()
                }
                (Some(_), None) => muted("Loading…", cx).into_any_element(),
                _ => empty("Choose one of Pack's logs.", cx).into_any_element(),
            }
        } else {
            match &self.logs {
                None if current.is_none() && self.sites.is_some() => empty("No sites.", cx).into_any_element(),
                None => muted("Loading…", cx).into_any_element(),
                Some(Err(e)) => empty(e.clone(), cx).into_any_element(),
                Some(Ok(d)) if d.entries.is_empty() => empty("Nothing logged yet.", cx).into_any_element(),
                Some(Ok(d)) => {
                    let entries: Vec<AnyElement> = d.entries.iter().enumerate().map(|(i, e)| self.log_entry(i, e, cx).into_any_element()).collect();
                    v_flex().w_full().gap_2().children(entries).into_any_element()
                }
            }
        };

        v_flex()
            .w_full()
            .gap_3()
            .child(header("Logs", "Laravel logs per site, plus Pack's own process logs.", cx))
            .child(
                h_flex()
                    .gap_2()
                    .child(site_picker)
                    .child(file_picker)
                    .child(Button::new("log-refresh").ghost().icon(IconName::RefreshCw).label("Refresh").on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))),
            )
            .child(body)
            .into_any_element()
    }

    fn log_entry(&self, i: usize, e: &crate::api::LogEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let color = match e.level.as_str() {
            "ERROR" | "CRITICAL" | "ALERT" | "EMERGENCY" => p.bad,
            "WARNING" => p.warn,
            "INFO" | "DEBUG" | "NOTICE" => p.info,
            _ => p.line,
        };
        let key = format!("log-{i}");
        let open = self.is_expanded(&key);
        let mut card = banner(color, cx)
            .py_3()
            .gap_1()
            .child(h_flex().gap_2().child(tag(e.level.clone(), cx)).child(mono(e.time.clone(), cx).text_color(p.muted)))
            .child(div().child(e.message.clone()));
        if !e.context.is_empty() {
            card = card.child(
                div()
                    .id(("log-ctx", i))
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .child(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }).xsmall())
                    .child("stack / context")
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(key.clone(), cx))),
            );
            if open {
                card = card.child(pre(e.context.clone(), cx));
            }
        }
        card
    }
}
