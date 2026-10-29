use super::widgets::*;
use super::{Palette, PackApp};
use crate::api::{Status, cmp_versions};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

impl PackApp {
    pub(super) fn render_php(&mut self, s: &Status, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let Some(d) = self.php.clone() else { return starting(cx).into_any_element() };
        let extensions: Vec<&str> = d.extensions.split(',').filter(|e| !e.is_empty()).collect();
        let mut available: Vec<(String, String)> = d.available.clone().into_iter().collect();
        available.sort_by(|a, b| cmp_versions(&b.0, &a.0));

        let mut table = table(cx).child(
            head_row(cx)
                .child(div().w(px(170.)).child("Version"))
                .child(div().flex_1().child("Installed"))
                .child(div().w(px(70.)).child("Status"))
                .child(div().w(px(260.)).child("")),
        );
        for (i, (v, latest)) in available.iter().enumerate() {
            let installed = d.installed.iter().find(|x| &x.version == v);
            let update = installed.is_some_and(|x| cmp_versions(latest, &x.patch).is_gt() && !x.patch.ends_with("-local"));
            let mut version = h_flex().gap_2().child(bold(format!("PHP {v}")));
            if *v == d.default {
                version = version.child(accent_tag("default", cx));
            }
            let mut patch = h_flex().gap_2().text_color(p.muted);
            let mut actions = h_flex().gap_1().justify_end();
            match installed {
                Some(x) => {
                    patch = patch.child(x.patch.clone());
                    if update {
                        patch = patch.child(accent_tag(format!("{latest} available"), cx));
                        let v = v.clone();
                        actions = actions.child(
                            Button::new(("php-update", i)).small().primary().label("Update").loading(self.is_busy(&format!("php-update-{v}"))).on_click(
                                cx.listener(move |this, _, window, cx| {
                                    let body = json!({ "name": "php", "series": v });
                                    this.post(format!("php-update-{v}"), "/api/packages/update".into(), body, format!("Updating PHP {v}…"), window, cx)
                                }),
                            ),
                        );
                    }
                    if *v != d.default {
                        let (v1, v2) = (v.clone(), v.clone());
                        actions = actions
                            .child(Button::new(("php-default", i)).small().label("Make default").on_click(cx.listener(move |this, _, window, cx| {
                                let v = v1.clone();
                                this.post(format!("php-default-{v}"), "/api/php/default".into(), json!({ "version": v }), format!("Default PHP is now {v}"), window, cx)
                            })))
                            .child(Button::new(("php-remove", i)).small().danger().label("Remove").on_click(cx.listener(move |this, _, window, cx| {
                                let v = v2.clone();
                                this.post(format!("php-remove-{v}"), "/api/php/uninstall".into(), json!({ "version": v }), "Removed", window, cx)
                            })));
                    }
                }
                None => {
                    patch = patch.child(format!("{latest} available"));
                    let v = v.clone();
                    actions = actions.child(
                        Button::new(("php-install", i)).small().primary().label("Install").loading(self.is_busy(&format!("php-install-{v}"))).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.post(format!("php-install-{v}"), "/api/php/install".into(), json!({ "version": v }), format!("Installing PHP {v}…"), window, cx)
                            }),
                        ),
                    );
                }
            }
            table = table.child(
                row(cx)
                    .child(div().w(px(170.)).child(version))
                    .child(div().flex_1().child(patch))
                    .child(div().w(px(70.)).when_some(installed, |c, x| c.child(dot(&x.status, cx))))
                    .child(div().w(px(260.)).child(actions)),
            );
        }
        if available.is_empty() {
            table = table.child(row(cx).justify_center().py_8().child(muted("The package manifest is not reachable.", cx)));
        }

        v_flex()
            .w_full()
            .gap_3()
            .child(header(
                "PHP",
                format!(
                    "Static PHP builds from the Pack repo, each with {} extensions. Every version runs its own FPM pool. Use php, php84, composer and laravel in your terminal.",
                    extensions.len()
                ),
                cx,
            ))
            .children(jobs(s, cx))
            .child(table)
            .child(card(cx).child(bold("Extensions")).child(h_flex().flex_wrap().gap_1().children(extensions.iter().map(|e| tag(e.to_string(), cx)))))
            .into_any_element()
    }
}
