//! Building blocks shared by the pages.

use super::Palette;
use crate::api::{Proc, Status};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::*;

pub fn logo(p: Palette) -> impl IntoElement {
    div()
        .size(px(30.))
        .rounded(px(8.))
        .bg(p.accent)
        .flex()
        .items_center()
        .justify_center()
        .text_color(white())
        .child(svg().path("icons/package.svg").size(px(17.)).text_color(white()))
}

pub fn count_badge(n: usize, p: Palette) -> impl IntoElement {
    div().px_1p5().rounded_full().bg(p.accent).text_color(white()).text_xs().child(n.to_string())
}

pub fn starting(cx: &App) -> impl IntoElement {
    let p = Palette::get(cx);
    v_flex()
        .w_full()
        .pt(px(120.))
        .items_center()
        .gap_3()
        .text_color(p.muted)
        .child(Spinner::new().color(p.accent))
        .child("Starting Pack…")
}

/// Page title with a one-line description, and optional buttons on the right.
pub fn header(title: &str, sub: impl IntoElement, cx: &App) -> Div {
    let p = Palette::get(cx);
    h_flex().w_full().justify_between().items_start().mb_2().child(
        v_flex()
            .gap_1()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child(title.to_string()))
            .child(div().text_color(p.muted).child(sub)),
    )
}

pub fn card(cx: &App) -> Div {
    let p = Palette::get(cx);
    v_flex().w_full().p_4().gap_2().rounded_xl().border_1().border_color(p.line).bg(p.card)
}

/// A card with an accent stripe on the left, for calls to action and errors.
pub fn banner(color: Hsla, cx: &App) -> Div {
    card(cx).border_l_4().border_color(color)
}

pub fn muted(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_color(Palette::get(cx).muted).child(text.into())
}

pub fn mono(text: impl Into<SharedString>, cx: &App) -> Div {
    div().font_family(cx.theme().mono_font_family.clone()).text_xs().child(text.into())
}

pub fn bold(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub fn tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = Palette::get(cx);
    div().px_2().py(px(1.)).rounded_full().bg(p.line.opacity(0.7)).text_color(p.muted).text_xs().child(text.into())
}

pub fn accent_tag(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = Palette::get(cx);
    div().px_2().py(px(1.)).rounded_full().bg(p.accent_soft).text_color(p.accent).text_xs().child(text.into())
}

pub fn state_color(state: &str, p: Palette) -> Hsla {
    match state {
        "running" => p.ok,
        "starting" => p.warn,
        "failed" => p.bad,
        _ => p.muted.opacity(0.5),
    }
}

/// A status light for a supervised process.
pub fn dot(status: &Option<Proc>, cx: &App) -> Div {
    dot_color(state_color(crate::api::state(status), Palette::get(cx)))
}

pub fn dot_color(color: Hsla) -> Div {
    div().flex_none().size(px(9.)).rounded_full().bg(color)
}

pub fn empty(text: impl Into<SharedString>, cx: &App) -> Div {
    card(cx).items_center().py_10().text_color(Palette::get(cx).muted).child(text.into())
}

/// A clickable link that opens in the browser.
pub fn link(id: impl Into<ElementId>, label: impl Into<SharedString>, url: String, cx: &App) -> Stateful<Div> {
    let p = Palette::get(cx);
    div()
        .id(id)
        .text_color(p.accent)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(label.into())
        .on_click(move |_, _, cx| cx.open_url(&url))
}

/// Download progress, running and failed jobs.
pub fn jobs(status: &Status, cx: &App) -> Vec<AnyElement> {
    let p = Palette::get(cx);
    let mut out = Vec::new();
    for d in &status.downloads {
        let mb = |b: u64| format!("{:.0}", b as f64 / 1e6);
        let size = match d.total {
            Some(t) => format!("{} / {} MB", mb(d.done), mb(t)),
            None => format!("{} MB", mb(d.done)),
        };
        let pct = d.total.filter(|t| *t > 0).map(|t| d.done as f32 / t as f32 * 100.).unwrap_or(0.);
        out.push(
            card(cx)
                .child(h_flex().justify_between().child(bold(d.label.clone())).child(muted(size, cx)))
                .child(Progress::new(SharedString::from(format!("dl-{}", d.label))).value(pct).color(p.accent))
                .into_any_element(),
        );
    }
    if status.downloads.is_empty() {
        for j in status.jobs.iter().filter(|j| j.state == "running") {
            out.push(
                card(cx)
                    .child(h_flex().gap_2().child(dot_color(p.warn)).child(format!("{}…", j.label)))
                    .into_any_element(),
            );
        }
    }
    let failed: Vec<_> = status.jobs.iter().filter(|j| j.state == "failed").collect();
    for j in failed.iter().rev().take(2).rev() {
        out.push(
            banner(p.bad, cx)
                .child(bold(format!("{} failed", j.label)))
                .child(pre(j.error.clone().unwrap_or_default(), cx))
                .into_any_element(),
        );
    }
    out
}

/// Preformatted, selectable text.
pub fn pre(text: impl Into<SharedString>, cx: &App) -> Div {
    let p = Palette::get(cx);
    div()
        .w_full()
        .p_3()
        .rounded_lg()
        .bg(p.ink.opacity(0.04))
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .line_height(px(17.))
        .whitespace_normal()
        .child(text.into())
}

/// A table row: the header row is muted; others get a top border.
pub fn row(cx: &App) -> Div {
    let p = Palette::get(cx);
    h_flex().w_full().gap_3().px_3().py_2p5().border_t_1().border_color(p.line)
}

pub fn head_row(cx: &App) -> Div {
    let p = Palette::get(cx);
    h_flex()
        .w_full()
        .gap_3()
        .px_3()
        .py_2()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.muted)
}

/// A table: a card without padding.
pub fn table(cx: &App) -> Div {
    let p = Palette::get(cx);
    v_flex().w_full().py_1().rounded_xl().border_1().border_color(p.line).bg(p.card).overflow_hidden()
}

/// Local clock time of a unix-ms timestamp.
pub fn clock(ms: u64) -> String {
    chrono::DateTime::from_timestamp_millis(ms as i64)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// The last `n` path components.
pub fn short_path(path: &str, n: usize) -> String {
    let parts: Vec<&str> = path.split(['/', '\\']).collect();
    parts[parts.len().saturating_sub(n)..].join("/")
}
