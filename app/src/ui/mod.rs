//! The Pack window: a sidebar with the pages, each rendered from data the
//! daemon's API returns. Data loads on the background executor; the current
//! page refreshes every 2 s while something can change and dumps every 1 s.

mod dumps;
mod logs;
mod mail;
mod overview;
mod php;
mod services;
mod settings;
mod sites;
mod widgets;

use crate::api::{self, Dump, Inbox, Mail, Packages, Php, Settings, SiteLogs, Services, Sites, Status, SystemLogs};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::InputState;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use std::collections::HashSet;
use std::time::Duration;

gpui_kit::assets::icon_assets!(
    pub Assets,
    [LayoutDashboard, Globe, CodeXml, Server, Mail, Bug, ScrollText, Settings, Lock, LockOpen, ExternalLink, RefreshCw, Plus, Trash, Share2, Play, Square, Database, Copy, Inbox, FileText, Package, Download, ShieldCheck, Folder, ArrowRight, ChevronDown, ChevronRight, Link, CircleAlert, X]
);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Sites,
    Php,
    Services,
    Mail,
    Dumps,
    Logs,
    Settings,
}

impl Page {
    pub const ALL: [Page; 8] =
        [Page::Overview, Page::Sites, Page::Php, Page::Services, Page::Mail, Page::Dumps, Page::Logs, Page::Settings];

    pub fn name(self) -> &'static str {
        match self {
            Page::Overview => "overview",
            Page::Sites => "sites",
            Page::Php => "php",
            Page::Services => "services",
            Page::Mail => "mail",
            Page::Dumps => "dumps",
            Page::Logs => "logs",
            Page::Settings => "settings",
        }
    }

    pub fn from_name(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }

    fn title(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Sites => "Sites",
            Page::Php => "PHP",
            Page::Services => "Services",
            Page::Mail => "Mail",
            Page::Dumps => "Dumps",
            Page::Logs => "Logs",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Page::Overview => IconName::LayoutDashboard,
            Page::Sites => IconName::Globe,
            Page::Php => IconName::CodeXml,
            Page::Services => IconName::Server,
            Page::Mail => IconName::Mail,
            Page::Dumps => IconName::Bug,
            Page::Logs => IconName::ScrollText,
            Page::Settings => IconName::Settings,
        }
    }
}

/// Pack's colours on top of the component theme, in light and dark.
#[derive(Clone, Copy)]
pub struct Palette {
    pub canvas: Hsla,
    pub card: Hsla,
    pub side: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub line: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub ok: Hsla,
    pub warn: Hsla,
    pub bad: Hsla,
    pub info: Hsla,
}

impl Palette {
    pub fn get(cx: &App) -> Palette {
        Self::for_mode(cx.theme().is_dark())
    }

    fn for_mode(dark: bool) -> Palette {
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        if dark {
            Palette {
                canvas: c(0x18171a),
                card: c(0x212024),
                side: c(0x1d1c1f),
                ink: c(0xecebe8),
                muted: c(0x9a968e),
                line: c(0x312f34),
                accent: c(0xff7a2f),
                accent_soft: c(0x3a2518),
                ok: c(0x40c057),
                warn: c(0xf59f00),
                bad: c(0xff6b6b),
                info: c(0x4dabf7),
            }
        } else {
            Palette {
                canvas: c(0xf6f5f2),
                card: c(0xffffff),
                side: c(0xefede8),
                ink: c(0x1c1b19),
                muted: c(0x77736b),
                line: c(0xe6e3dc),
                accent: c(0xe8590c),
                accent_soft: c(0xfff0e6),
                ok: c(0x2f9e44),
                warn: c(0xe67700),
                bad: c(0xe03131),
                info: c(0x1c7ed6),
            }
        }
    }
}

/// Switches to the light or dark theme and paints it in Pack's colours.
pub fn apply_theme(mode: ThemeMode, cx: &mut App) {
    Theme::change(mode, None, cx);
    let p = Palette::for_mode(mode.is_dark());
    Theme::update(cx, |t| {
        let c = &mut t.colors;
        c.background = p.card;
        c.foreground = p.ink;
        c.muted_foreground = p.muted;
        c.border = p.line;
        c.input = p.line;
        c.popover = p.card;
        c.popover_foreground = p.ink;
        c.primary = p.accent;
        c.primary_hover = p.accent.opacity(0.9);
        c.primary_active = p.accent.opacity(0.8);
        c.primary_foreground = gpui_kit::white();
        c.button_primary = p.accent;
        c.button_primary_hover = p.accent.opacity(0.9);
        c.button_primary_active = p.accent.opacity(0.8);
        c.button_primary_foreground = gpui_kit::white();
        c.ring = p.accent;
        c.link = p.accent;
        c.link_hover = p.accent.opacity(0.85);
        c.link_active = p.accent.opacity(0.7);
        c.sidebar = p.side;
        c.sidebar_foreground = p.ink;
        c.sidebar_border = p.line;
        c.sidebar_accent = p.card;
        c.sidebar_accent_foreground = p.ink;
        c.sidebar_primary = p.accent;
        c.sidebar_primary_foreground = gpui_kit::white();
        c.success = p.ok;
        c.danger = p.bad;
        c.warning = p.warn;
        c.info = p.info;
    });
}

fn sync_theme(window: &Window, cx: &mut App) {
    let dark = matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark);
    apply_theme(if dark { ThemeMode::Dark } else { ThemeMode::Light }, cx);
}

/// The open window, if any.
#[derive(Default)]
struct MainWindow(Option<(AnyWindowHandle, WeakEntity<PackApp>)>);
impl Global for MainWindow {}

/// Brings the window forward on `page`, opening it if it was closed.
pub fn show(page: Page, cx: &mut App) {
    if let Some((handle, view)) = cx.default_global::<MainWindow>().0.clone() {
        let shown = handle.update(cx, |_, window, cx| {
            window.activate_window();
            view.update(cx, |app, cx| app.go(page, cx)).is_ok()
        });
        if shown.unwrap_or(false) {
            cx.activate(true);
            return;
        }
    }
    let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(820.), px(540.))),
        titlebar: Some(TitlebarOptions {
            title: Some("Pack".into()),
            // macOS: the traffic lights sit over the sidebar.
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(14.), px(16.))),
        }),
        app_id: Some("dev.pack.app".into()),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        sync_theme(window, cx);
        cx.new(|cx| PackApp::new(page, window, cx))
    }) {
        Ok((handle, view)) => {
            cx.set_global(MainWindow(Some((handle, view.downgrade()))));
            cx.activate(true);
        }
        Err(e) => eprintln!("could not open the Pack window: {e:#}"),
    }
}

/// Text fields, kept across renders.
struct Inputs {
    park: Entity<InputState>,
    link_path: Entity<InputState>,
    link_name: Entity<InputState>,
    proxy_name: Entity<InputState>,
    proxy_upstream: Entity<InputState>,
    /// php.ini: memory_limit, upload_max_filesize, post_max_size, max_execution_time.
    ini: [Entity<InputState>; 4],
}

pub const INI_KEYS: [&str; 4] = ["memory_limit", "upload_max_filesize", "post_max_size", "max_execution_time"];

impl Inputs {
    fn new(window: &mut Window, cx: &mut Context<PackApp>) -> Self {
        let mut input = |placeholder: &str| {
            let placeholder = placeholder.to_string();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        Inputs {
            park: input("~/Code"),
            link_path: input("/path/to/project"),
            link_name: input("name (optional)"),
            proxy_name: input("name"),
            proxy_upstream: input("127.0.0.1:3000"),
            ini: [input("512M"), input("100M"), input("100M"), input("0")],
        }
    }
}

/// Everything one refresh loaded. `None`: the page didn't need it.
#[derive(Default)]
struct Loaded {
    status: Option<Result<Status, String>>,
    sites: Option<Sites>,
    php: Option<Php>,
    services: Option<Services>,
    packages: Option<Packages>,
    settings: Option<Settings>,
    inbox: Option<Result<Inbox, String>>,
    logs: Option<Result<SiteLogs, String>>,
    log_site: Option<String>,
    sys_logs: Option<SystemLogs>,
}

/// What a refresh needs to know about the page.
#[derive(Clone)]
struct Query {
    page: Page,
    log_site: Option<String>,
    log_file: Option<String>,
    sys_log: Option<String>,
}

pub const SYSTEM_LOGS: &str = "__system";

impl Loaded {
    fn fetch(q: Query) -> Loaded {
        use Page::*;
        let need = |pages: &[Page]| pages.contains(&q.page);
        let mut l = Loaded { status: Some(api::get("/api/status")), ..Default::default() };
        if matches!(l.status, Some(Err(_))) {
            return l;
        }
        if need(&[Overview, Sites, Logs]) {
            l.sites = api::get("/api/sites").ok();
        }
        if need(&[Sites, Php]) {
            l.php = api::get("/api/php").ok();
        }
        if need(&[Overview, Services, Mail]) {
            l.services = api::get("/api/services").ok();
        }
        if need(&[Overview, Php, Services]) {
            l.packages = api::get("/api/packages").ok();
        }
        if need(&[Settings]) {
            l.settings = api::get("/api/settings").ok();
        }
        if q.page == Mail {
            let port = l.services.as_ref().and_then(|s| s.instances.iter().find(|i| i.kind == "mail")).and_then(|m| m.ui_port);
            if let Some(port) = port {
                l.inbox = Some(api::get(&format!("http://127.0.0.1:{port}/api/v1/messages?limit=100")));
            }
        }
        if q.page == Logs {
            let first = l.sites.as_ref().and_then(|s| s.sites.iter().find(|s| s.proxy.is_none())).map(|s| s.name.clone());
            let site = q.log_site.clone().or(first);
            if site.as_deref() == Some(SYSTEM_LOGS) {
                let name = q.sys_log.as_deref().map(|n| format!("?name={}", api::enc(n))).unwrap_or_default();
                l.sys_logs = api::get(&format!("/api/logs/system{name}")).ok();
            } else if let Some(site) = &site {
                let file = q.log_file.as_deref().map(|f| format!("&file={}", api::enc(f))).unwrap_or_default();
                l.logs = Some(api::get(&format!("/api/logs?site={}{file}", api::enc(site))));
            }
            l.log_site = site;
        }
        l
    }
}

pub struct PackApp {
    page: Page,
    /// The daemon didn't answer the last status request.
    offline: bool,
    status: Option<Status>,
    sites: Option<Sites>,
    php: Option<Php>,
    services: Option<Services>,
    packages: Option<Packages>,
    settings: Option<Settings>,
    dumps: Vec<Dump>,
    last_dump: u64,
    seen_dump: u64,
    inbox: Option<Result<Inbox, String>>,
    mail_open: Option<String>,
    mail: Option<Result<Mail, String>>,
    log_site: Option<String>,
    log_file: Option<String>,
    logs: Option<Result<SiteLogs, String>>,
    sys_log: Option<String>,
    sys_logs: Option<SystemLogs>,
    /// Expanded log entries and service .env snippets.
    expanded: HashSet<String>,
    /// Buttons whose request is in flight.
    busy: HashSet<String>,
    inputs: Inputs,
    /// Settings fields were filled from the daemon once.
    ini_loaded: bool,
    pending_ini: Option<[String; 4]>,
    /// Screenshot mode: show the app as it looks after Set up.
    pub demo: bool,
    /// A refresh completed since the page was opened.
    pub loaded: bool,
    refresh_task: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PackApp {
    pub fn new(page: Page, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let poll = cx.spawn(async move |this, cx| {
            let mut tick = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                tick += 1;
                let alive = this.update(cx, |this, cx| {
                    this.poll_dumps(cx);
                    if tick % 2 == 0 && this.should_poll() {
                        this.refresh(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let appearance = cx.observe_window_appearance(window, |_, window, cx| {
            if !cx.global::<crate::Options>().fixed_theme {
                sync_theme(window, cx);
            }
        });
        let mut app = PackApp {
            page,
            offline: false,
            status: None,
            sites: None,
            php: None,
            services: None,
            packages: None,
            settings: None,
            dumps: Vec::new(),
            last_dump: 0,
            seen_dump: 0,
            inbox: None,
            mail_open: None,
            mail: None,
            log_site: None,
            log_file: None,
            logs: None,
            sys_log: None,
            sys_logs: None,
            expanded: HashSet::new(),
            busy: HashSet::new(),
            inputs: Inputs::new(window, cx),
            ini_loaded: false,
            pending_ini: None,
            demo: false,
            loaded: false,
            refresh_task: None,
            _tasks: vec![poll],
            _subscriptions: vec![appearance],
        };
        app.load_dumps(cx);
        app.refresh(cx);
        app
    }

    /// The page shows its data (screenshot mode waits for this).
    #[cfg_attr(not(feature = "screenshots"), allow(dead_code))]
    pub fn ready(&self) -> bool {
        if !self.loaded || self.offline {
            return false;
        }
        let all_running = |s: &Option<Services>| {
            s.as_ref().is_some_and(|s| !s.instances.is_empty() && s.instances.iter().all(|i| api::state(&i.status) == "running"))
        };
        match self.page {
            Page::Overview => all_running(&self.services) && self.sites.is_some() && self.packages.is_some(),
            Page::Sites => self.sites.as_ref().is_some_and(|s| !s.sites.is_empty()) && self.php.is_some(),
            Page::Php => self.php.is_some(),
            Page::Services => all_running(&self.services),
            Page::Mail => matches!(self.mail, Some(Ok(_))),
            Page::Dumps => !self.dumps.is_empty(),
            Page::Logs => self.logs.is_some() || self.sys_logs.is_some(),
            Page::Settings => self.settings.is_some() && self.pending_ini.is_none(),
        }
    }

    pub fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page != page {
            self.page = page;
            self.loaded = false;
            self.expanded.clear();
        }
        if page == Page::Dumps {
            self.seen_dump = self.last_dump;
        }
        self.refresh(cx);
        cx.notify();
    }

    fn should_poll(&self) -> bool {
        let working = self.status.as_ref().is_some_and(|s| !s.downloads.is_empty() || s.jobs.iter().any(|j| j.state == "running"));
        self.offline || working || matches!(self.page, Page::Overview | Page::Services | Page::Mail)
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let q = Query {
            page: self.page,
            log_site: self.log_site.clone(),
            log_file: self.log_file.clone(),
            sys_log: self.sys_log.clone(),
        };
        let task = cx.background_spawn(async move { Loaded::fetch(q) });
        self.refresh_task = Some(cx.spawn(async move |this, cx| {
            let loaded = task.await;
            this.update(cx, |this, cx| this.apply(loaded, cx)).ok();
        }));
    }

    fn apply(&mut self, l: Loaded, cx: &mut Context<Self>) {
        match l.status {
            Some(Ok(mut status)) => {
                if self.demo {
                    // Screenshots run without touching DNS or the keychain.
                    status.dns.resolver = true;
                    status.dns.low_ports = true;
                    status.ca_trusted = true;
                }
                self.status = Some(status);
                self.offline = false;
            }
            Some(Err(_)) => self.offline = true,
            None => {}
        }
        macro_rules! keep {
            ($($f:ident),*) => { $( if l.$f.is_some() { self.$f = l.$f; } )* };
        }
        keep!(sites, php, services, packages, inbox, logs, sys_logs);
        if let Some(settings) = l.settings {
            if !self.ini_loaded {
                self.ini_loaded = true;
                self.fill_ini(&settings);
            }
            self.settings = Some(settings);
        }
        if self.page == Page::Logs {
            self.log_site = l.log_site;
        }
        if !self.offline {
            self.loaded = true;
        }
        cx.notify();
    }

    fn fill_ini(&mut self, s: &Settings) {
        let values = [
            s.php_ini.memory_limit.clone(),
            s.php_ini.upload_max_filesize.clone(),
            s.php_ini.post_max_size.clone(),
            s.php_ini.max_execution_time.to_string(),
        ];
        // Setting a value needs the window: see `render`.
        self.pending_ini = Some(values);
    }

    fn load_dumps(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async { api::get::<api::Dumps>("/api/dumps") });
        self._tasks.push(cx.spawn(async move |this, cx| {
            if let Ok(d) = task.await {
                this.update(cx, |this, cx| {
                    this.last_dump = d.dumps.last().map(|d| d.id).unwrap_or(0);
                    this.seen_dump = this.last_dump;
                    this.dumps = d.dumps;
                    cx.notify();
                })
                .ok();
            }
        }));
    }

    fn poll_dumps(&mut self, cx: &mut Context<Self>) {
        let since = self.last_dump;
        let task = cx.background_spawn(async move { api::get::<api::Dumps>(&format!("/api/dumps?since={since}")) });
        cx.spawn(async move |this, cx| {
            let Ok(d) = task.await else { return };
            if d.dumps.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                this.dumps.extend(d.dumps.into_iter().filter(|d| d.id > this.last_dump));
                let excess = this.dumps.len().saturating_sub(300);
                this.dumps.drain(..excess);
                this.last_dump = this.dumps.last().map(|d| d.id).unwrap_or(this.last_dump);
                if this.page == Page::Dumps {
                    this.seen_dump = this.last_dump;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn unseen_dumps(&self) -> usize {
        self.dumps.iter().filter(|d| d.id > self.seen_dump).count()
    }

    /// Runs a request off the UI thread with `key` marked busy, then shows
    /// `done(response)` (or the error) and refreshes.
    pub fn act(
        &mut self,
        key: impl Into<String>,
        request: impl FnOnce() -> Result<Value, String> + Send + 'static,
        done: impl FnOnce(&Value) -> String + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = key.into();
        self.busy.insert(key.clone());
        cx.notify();
        let task = cx.background_spawn(async move { request() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.busy.remove(&key);
                match result {
                    Ok(v) => window.push_notification(Notification::success(done(&v)), cx),
                    Err(e) => window.push_notification(Notification::error(e).autohide(false), cx),
                }
                this.refresh(cx);
            })
            .ok();
        })
        .detach();
    }

    /// `act` for a POST with a fixed message.
    pub fn post(&mut self, key: impl Into<String>, path: String, body: Value, done: impl Into<String>, window: &mut Window, cx: &mut Context<Self>) {
        let done = done.into();
        self.act(key, move || api::post(&path, body), move |_| done, window, cx);
    }

    pub fn is_busy(&self, key: &str) -> bool {
        self.busy.contains(key)
    }

    pub fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        cx.notify();
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::get(cx);
        let unseen = self.unseen_dumps();
        let items = Page::ALL.into_iter().map(|page| {
            let mut item = SidebarMenuItem::new(page.title())
                .icon(page.icon())
                .active(self.page == page)
                .on_click(cx.listener(move |this, _, _, cx| this.go(page, cx)));
            if page == Page::Dumps && unseen > 0 {
                item = item.suffix(move |_, _| widgets::count_badge(unseen, p));
            }
            item
        });
        let footer = self.status.as_ref().map(|s| format!("v{} · API :{}", s.version, api::PORT)).unwrap_or_default();
        Sidebar::new("nav")
            .w(px(214.))
            .header(
                h_flex()
                    .w_full()
                    .gap_2p5()
                    .when(cfg!(target_os = "macos"), |d| d.pt(px(30.)))
                    .child(widgets::logo(p))
                    .child(
                        v_flex()
                            .child(div().text_base().font_weight(FontWeight::BOLD).child("Pack"))
                            .child(div().text_xs().text_color(p.muted).child("free local PHP dev")),
                    ),
            )
            .child(SidebarMenu::new().children(items))
            .footer(div().px_2().text_xs().text_color(p.muted).child(footer))
    }

    fn render_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(status) = self.status.clone().filter(|_| !self.offline) else {
            return widgets::starting(cx).into_any_element();
        };
        match self.page {
            Page::Overview => self.render_overview(&status, cx),
            Page::Sites => self.render_sites(&status, window, cx),
            Page::Php => self.render_php(&status, cx),
            Page::Services => self.render_services(&status, window, cx),
            Page::Mail => self.render_mail(cx),
            Page::Dumps => self.render_dumps(cx),
            Page::Logs => self.render_logs(cx),
            Page::Settings => self.render_settings(&status, cx),
        }
    }
}

impl Render for PackApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(values) = self.pending_ini.take() {
            for (input, value) in self.inputs.ini.clone().iter().zip(values) {
                input.update(cx, |i, cx| i.set_value(value, window, cx));
            }
        }
        let p = Palette::get(cx);
        let page = self.render_page(window, cx);
        h_flex()
            .id("pack")
            .size_full()
            .bg(p.canvas)
            .text_color(p.ink)
            .text_sm()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .id("main")
                    .flex_1()
                    .h_full()
                    .overflow_y_scrollbar()
                    .child(v_flex().w_full().max_w(px(1100.)).px_8().pt_7().pb_10().gap_3().child(page)),
            )
    }
}

/// Shows a notification from anywhere with a window.
pub fn notify(window: &mut Window, cx: &mut App, note: impl Into<Notification>) {
    window.push_notification(note, cx);
}
