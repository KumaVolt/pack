use super::widgets::*;
use super::{INI_KEYS, Palette, PackApp};
use crate::api::Status;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::*;
use serde_json::json;

impl PackApp {
    pub(super) fn render_settings(&mut self, s: &Status, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let Some(c) = self.settings.clone() else { return starting(cx).into_any_element() };
        let toggle = |this: &Self, key: &'static str, on: bool, title: &str, hint: &str, cx: &mut Context<Self>| {
            h_flex()
                .w_full()
                .justify_between()
                .gap_6()
                .py_1()
                .child(v_flex().flex_1().child(bold(title.to_string())).child(muted(hint.to_string(), cx)))
                .child(Switch::new(key).checked(on).disabled(this.is_busy(key)).on_click(cx.listener(move |this, checked: &bool, window, cx| {
                    // Show the new state right away; the refresh confirms it.
                    if let Some(s) = &mut this.settings {
                        match key {
                            "https" => s.https = *checked,
                            "dumps" => s.dumps = *checked,
                            "lan_access" => s.lan_access = *checked,
                            _ => s.auto_update = *checked,
                        }
                    }
                    this.post(key, "/api/settings".into(), json!({ key: checked }), "Saved", window, cx)
                })))
        };

        let ini_fields = INI_KEYS.iter().zip(self.inputs.ini.iter()).map(|(k, input)| {
            v_flex().gap_1().child(mono(k.to_string(), cx).text_color(p.muted)).child(Input::new(input)).into_any_element()
        });
        let ini_fields: Vec<AnyElement> = ini_fields.collect();

        v_flex()
            .w_full()
            .gap_3()
            .child(header("Settings", h_flex().gap_1().child("Stored in").child(mono(format!("{}/config.json", s.home), cx)), cx))
            .child(card(cx).child(toggle(self, "https", c.https, "Serve every site over HTTPS", "You can still switch single sites to plain HTTP on the Sites page.", cx)))
            .child(
                card(cx)
                    .child(toggle(self, "dumps", c.dumps, "Catch dump() output", "Send dump() and dd() to Pack instead of the page.", cx))
                    .child(toggle(self, "lan_access", c.lan_access, "Share with my network", "Allow other devices on my network to open my sites.", cx))
                    .child(toggle(
                        self,
                        "auto_update",
                        c.auto_update,
                        "Install updates automatically",
                        "Patch releases within the same version line, e.g. PHP 8.4.10 → 8.4.11.",
                        cx,
                    )),
            )
            .child(
                card(cx)
                    .child(bold("php.ini"))
                    .child(div().w_full().grid().grid_cols(4).gap_3().children(ini_fields))
                    .child(h_flex().mt_2().child(
                        Button::new("save-ini").primary().label("Save & restart PHP").loading(self.is_busy("save-ini")).on_click(cx.listener(|this, _, window, cx| {
                            let v: Vec<String> = this.inputs.ini.iter().map(|i| i.read(cx).value().to_string()).collect();
                            let php_ini = json!({
                                "memory_limit": v[0],
                                "upload_max_filesize": v[1],
                                "post_max_size": v[2],
                                "max_execution_time": v[3].trim().parse::<u32>().unwrap_or(0),
                            });
                            this.post("save-ini", "/api/settings".into(), json!({ "php_ini": php_ini }), "Saved — PHP restarted", window, cx)
                        })),
                    )),
            )
            .child(
                card(cx).child(
                    h_flex()
                        .justify_between()
                        .child(v_flex().child(bold("Restart everything")).child(muted("Restarts Caddy, PHP-FPM and all services.", cx)))
                        .child(Button::new("restart").label("Restart").loading(self.is_busy("restart")).on_click(cx.listener(|this, _, window, cx| {
                            this.post("restart", "/api/restart".into(), json!({}), "Restarted", window, cx)
                        }))),
                ),
            )
            .into_any_element()
    }
}
