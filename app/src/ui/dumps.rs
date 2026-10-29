use super::widgets::*;
use super::{Palette, PackApp};
use crate::api;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

impl PackApp {
    pub(super) fn render_dumps(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::get(cx);
        let cards: Vec<AnyElement> = self
            .dumps
            .iter()
            .rev()
            .map(|d| {
                let place = d.file.as_deref().map(|f| short_path(f, 3) + &d.line.map(|l| format!(":{l}")).unwrap_or_default()).unwrap_or_default();
                card(cx)
                    .p_0()
                    .gap_0()
                    .child(
                        h_flex()
                            .px_4()
                            .py_2()
                            .gap_3()
                            .border_b_1()
                            .border_color(p.line)
                            .text_xs()
                            .text_color(p.muted)
                            .child(div().font_weight(FontWeight::BOLD).text_color(p.ink).child(d.site.clone().unwrap_or_else(|| "cli".into())))
                            .child(d.request.clone().unwrap_or_default())
                            .child(mono(place, cx).ml_auto())
                            .child(clock(d.time)),
                    )
                    .child(div().px_4().py_3().child(dump_text(&d.text, p, cx)))
                    .into_any_element()
            })
            .collect();
        let empty_state = self.dumps.is_empty();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                header(
                    "Dumps",
                    h_flex().gap_1().child("Every").child(mono("dump()", cx)).child("and").child(mono("dd()", cx)).child("from your sites lands here."),
                    cx,
                )
                .child(Button::new("clear-dumps").icon(IconName::Trash).label("Clear").on_click(cx.listener(|this, _, _, cx| {
                    this.dumps.clear();
                    cx.background_spawn(async { api::delete("/api/dumps") }).detach();
                    cx.notify();
                }))),
            )
            .children(cards)
            .when(empty_state, |c| {
                c.child(empty("", cx).child(h_flex().gap_1().child("No dumps yet. Call").child(mono("dump($anything)", cx)).child("in a site.")))
            })
            .into_any_element()
    }
}

/// VarDumper's text output with keys, strings and numbers coloured.
fn dump_text(text: &str, p: Palette, cx: &App) -> impl IntoElement {
    let mut runs = Vec::new();
    let mut plain = String::new();
    for line in text.trim_end().split_inclusive('\n') {
        for (span, kind) in tokens(line) {
            if kind == Token::Plain {
                plain.push_str(span);
                continue;
            }
            let start = plain.len();
            plain.push_str(span);
            let color = match kind {
                Token::Str => p.ok,
                Token::Num => p.info,
                Token::Key => p.accent,
                Token::Type => p.muted,
                Token::Plain => unreachable!(),
            };
            runs.push((start..plain.len(), HighlightStyle { color: Some(color), ..Default::default() }));
        }
    }
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .line_height(px(18.))
        .child(StyledText::new(plain).with_highlights(runs))
}

#[derive(PartialEq, Clone, Copy)]
enum Token {
    Plain,
    Str,
    Num,
    Key,
    Type,
}

/// Splits one CliDumper line into coloured spans: `"key" => "value"`,
/// numbers, and type headers like `array:3 [`.
fn tokens(line: &str) -> Vec<(&str, Token)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut plain_start = 0;
    let arrow = line.find(" => ");
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            let end = line[i + 1..].find('"').map(|e| i + 1 + e + 1).unwrap_or(bytes.len());
            out.push((&line[plain_start..i], Token::Plain));
            let is_key = arrow.is_some_and(|a| end <= a);
            out.push((&line[i..end], if is_key { Token::Key } else { Token::Str }));
            i = end;
            plain_start = i;
        } else if c.is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let end = line[i..].find(|ch: char| !(ch.is_ascii_digit() || ch == '.')).map(|e| i + e).unwrap_or(bytes.len());
            let is_key = arrow.is_some_and(|a| end <= a);
            out.push((&line[plain_start..i], Token::Plain));
            out.push((&line[i..end], if is_key { Token::Key } else { Token::Num }));
            i = end;
            plain_start = i;
        } else if c.is_ascii_alphabetic() && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()) {
            let end = line[i..].find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '\\')).map(|e| i + e).unwrap_or(bytes.len());
            let word = &line[i..end];
            let typed = line[end..].starts_with(':') || line[end..].starts_with(" {");
            if typed || matches!(word, "true" | "false" | "null") {
                out.push((&line[plain_start..i], Token::Plain));
                out.push((word, if typed { Token::Type } else { Token::Num }));
                plain_start = end;
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out.push((&line[plain_start..], Token::Plain));
    out.retain(|(s, _)| !s.is_empty());
    out
}
