#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Love愛ドーナツ (Kneekura Prompt Wheeler) — a translucent prompt wheel that floats on the desktop.
//!
//! No GPU and no webview: egui lays the interface out, a CPU rasteriser paints it into a
//! 32-bit bitmap, and Windows composites that bitmap as a per-pixel-alpha layered window.
//! Fully transparent pixels are click-through for free, so one fixed-size window can hold the
//! wheel plus room for the prompt panel on either side without blocking the desktop.

mod data;
mod win;

use data::{Category, EditRec, Item, Origin, Saved, UserPrompt};
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use egui_software_backend::{BufferMutRef, ColorFieldOrder, EguiSoftwareRender};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId, WindowLevel};

const TITLE: &str = "Love愛ドーナツ";
const WIN: Vec2 = Vec2::new(960.0, 600.0);
const C: Pos2 = Pos2::new(480.0, 300.0);
const R_OUT: f32 = 150.0;
const R_IN: f32 = 98.0;
const R_CORE: f32 = 86.0;
const R_PUCK: f32 = 26.0;
const GAP_DEG: f32 = 1.6;
const PANEL_W: f32 = 300.0;
const PANEL_GAP: f32 = 16.0;
const PANEL_H: f32 = 416.0;
const DWELL: f64 = 1.5;
// A hard spin must be held: the wheel has to keep turning fast for the whole of TURBO_HOLD.
// That window is cut into TURBO_SLICES equal slices and every slice needs TURBO_PER_SLICE notches,
// so one big flick (which empties within a slice or two) never qualifies. 12 notches per 0.5 s is
// 24 a second, held for 3 s: a wheel that is being spun on purpose, not scrolled.
const TURBO_HOLD: f64 = 3.0;
const TURBO_SLICES: usize = 6;
const TURBO_PER_SLICE: f32 = 12.0;
const CORNERS: [(f32, f32); 4] = [(-139.0, -139.0), (139.0, -139.0), (139.0, 139.0), (-139.0, 139.0)];

// Sumi-ink ground with warm paper-coloured type; the genre colours (data/prompts.json) are
// traditional Japanese pigments and carry all the saturation.
const INK: Color32 = Color32::from_rgb(23, 21, 18);
const INK2: Color32 = Color32::from_rgb(40, 36, 30);
const DEEP: Color32 = Color32::from_rgb(13, 12, 10);
const DARK: Color32 = Color32::from_rgb(20, 18, 15);
const TEXT: Color32 = Color32::from_rgb(242, 234, 216);
const SOFT: Color32 = Color32::from_rgb(214, 205, 186);
const MUTED: Color32 = Color32::from_rgb(150, 140, 122);
const LINE: Color32 = Color32::from_rgb(64, 58, 49);
const GOLD: Color32 = Color32::from_rgb(236, 186, 72);
const BAD: Color32 = Color32::from_rgb(234, 124, 98);
const SETTINGS_H: f32 = 362.0;

const REQ_SHOW: u8 = 1;
const REQ_QUIT: u8 = 2;

fn main() {
    if !win::claim_single_instance("Local\\KneekuraPromptWheeler.single-instance") {
        return;
    }
    let event_loop: EventLoop<Duration> = EventLoop::with_user_event().build().expect("event loop");
    let mut runner = Runner { proxy: event_loop.create_proxy(), live: None, next_repaint: None };
    let _ = event_loop.run_app(&mut runner);
}

// ---------- window, event loop, presentation ----------

struct Live {
    window: Arc<Window>,
    ctx: egui::Context,
    input: egui_winit::State,
    renderer: EguiSoftwareRender,
    surface: win::Layered,
    app: App,
}

struct Runner {
    /// Carries egui's "repaint after this long" requests back onto the event loop.
    proxy: EventLoopProxy<Duration>,
    live: Option<Live>,
    next_repaint: Option<Instant>,
}

impl ApplicationHandler<Duration> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        let saved = Saved::load();
        let attributes = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(winit::dpi::LogicalSize::new(WIN.x * saved.scale, WIN.y * saved.scale))
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_window_level(if saved.pinned { WindowLevel::AlwaysOnTop } else { WindowLevel::Normal })
            .with_skip_taskbar(true)
            .with_undecorated_shadow(false);
        let window = Arc::new(event_loop.create_window(attributes).expect("window"));
        let hwnd = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
            _ => 0,
        };
        let ctx = egui::Context::default();
        let proxy = self.proxy.clone();
        ctx.set_request_repaint_callback(move |info| {
            let _ = proxy.send_event(info.delay);
        });
        let input = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, None);
        let app = App::new(&ctx, saved, window.clone(), hwnd);
        // A layered window shows nothing until its first bitmap arrives, so it can be made visible
        // straight away; a hidden window would never receive the paint message that draws that bitmap.
        win::make_layered(hwnd);
        win::show_no_activate(hwnd);
        self.live = Some(Live { window: window.clone(), ctx, input, renderer: EguiSoftwareRender::new(ColorFieldOrder::Bgra), surface: win::Layered::new(hwnd), app });
        window.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, delay: Duration) {
        let due = Instant::now() + delay;
        self.next_repaint = Some(self.next_repaint.map_or(due, |at| at.min(due)));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.next_repaint {
            Some(at) if at <= Instant::now() => {
                self.next_repaint = None;
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(live) = self.live.as_mut() else { return };
        // A redraw is not input: feeding it to egui would make every frame ask for the next one.
        let wants_repaint = !matches!(event, WindowEvent::RedrawRequested) && live.input.on_window_event(&live.window, &event).repaint;
        if wants_repaint {
            live.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                live.app.saved.store();
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let size = live.window.inner_size();
                let (w, h) = (size.width.max(1) as usize, size.height.max(1) as usize);
                let raw = live.input.take_egui_input(&live.window);
                let app = &mut live.app;
                let output = live.ctx.run_ui(raw, |ui| app.frame(ui));
                live.input.handle_platform_output(&live.window, output.platform_output);
                let primitives = live.ctx.tessellate(output.shapes, output.pixels_per_point);
                let pixels = live.surface.pixels(w, h);
                pixels.fill([0; 4]);
                live.renderer.render(&mut BufferMutRef::new(pixels, w, h), &primitives, &output.textures_delta, output.pixels_per_point);
                // The idle fade is a single constant alpha applied by the compositor: no re-raster needed.
                live.surface.present((live.app.op.clamp(0.0, 1.0) * 255.0).round() as u8);
                if live.app.quit {
                    live.app.saved.store();
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

// ---------- small helpers ----------

fn font(size: f32, bold: bool) -> FontId {
    FontId::new(size, if bold { FontFamily::Name("bold".into()) } else { FontFamily::Proportional })
}
/// Yu Mincho Demibold: prompt titles and headings.
fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}
/// Bahnschrift: counters and values.
fn num(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("num".into()))
}
fn hex(code: &str) -> Color32 {
    let v = u32::from_str_radix(code.trim_start_matches('#'), 16).unwrap_or(0x8395ff);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}
/// Ease-out with a slight overshoot, for things that should "arrive".
fn ease_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.2 * t * t * t + 1.2 * t * t
}
fn polar(radius: f32, deg: f32) -> Vec2 {
    let rad = deg.to_radians();
    vec2(radius * rad.cos(), radius * rad.sin())
}
fn arc_points(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = (((to - from).abs() / 2.5).ceil() as usize).max(2);
    (0..=steps).map(|i| center + polar(radius, lerp(from, to, i as f32 / steps as f32))).collect()
}
/// Exponential approach to `target`; returns true while still moving.
fn approach(value: &mut f32, target: f32, dt: f32, speed: f32) -> bool {
    *value += (target - *value) * (1.0 - (-speed * dt).exp());
    if (target - *value).abs() < 0.002 {
        *value = target;
        false
    } else {
        true
    }
}
fn panel_rect(side: f32) -> Rect {
    let edge = C.x + side * (R_OUT + PANEL_GAP);
    let (left, right) = if side > 0.0 { (edge, edge + PANEL_W) } else { (edge - PANEL_W, edge) };
    Rect::from_min_max(pos2(left, C.y - PANEL_H / 2.0), pos2(right, C.y + PANEL_H / 2.0))
}
fn galley(ui: &egui::Ui, text: &str, id: FontId, color: Color32, width: f32, rows: usize, line: Option<f32>, center: bool) -> Arc<egui::Galley> {
    let format = egui::TextFormat { font_id: id, color, line_height: line, ..Default::default() };
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    job.wrap = egui::text::TextWrapping { max_width: width, max_rows: rows, break_anywhere: false, overflow_character: Some('…') };
    if center {
        job.halign = egui::Align::Center;
    }
    ui.fonts_mut(|f| f.layout_job(job))
}

// ---------- play: sparks and the burst ----------

/// A hot fleck thrown off the dial; drawn as a short streak along its velocity.
struct Spark {
    pos: Pos2,
    vel: Vec2,
    age: f32,
    life: f32,
}

/// How one part of the wheel moves when the wheel comes apart.
/// Explode: `off` is where it is flung, `curl` bends the path, `spin` tumbles it.
/// Fall: `off.x` is sideways drift, `off.y` the drop to where it lands, `spin` its resting tilt.
struct Shard {
    off: Vec2,
    curl: f32,
    spin: f32,
    delay: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum BurstKind {
    /// Ten quick clicks: everything is flung outwards.
    Explode,
    /// Shaking: parts rattle loose one by one and drop, like things off a shelf.
    Fall,
}

/// The wheel coming apart and then putting itself back. Each part's place is a pure function of
/// one clock that runs forward, holds, and then runs backward, so the return is an exact reverse
/// playback of the way out rather than a second animation.
struct Burst {
    start: f64,
    kind: BurstKind,
    /// Length of the outward motion; the clock never goes past it.
    span: f32,
    back: f32,
    shards: Vec<Shard>,
}

const BURST_OUT: f32 = 0.62;
const BURST_STAGGER: f32 = 0.16;
const BURST_HOLD: f32 = 0.75;
const FALL_GRAVITY: f32 = 1900.0;

impl Burst {
    /// Position on the outward timeline at `t` seconds, or None once everything is home again.
    fn clock(&self, t: f32) -> Option<f32> {
        if t < self.span {
            Some(t)
        } else if t < self.span + BURST_HOLD {
            Some(self.span)
        } else if t < self.span + BURST_HOLD + self.back {
            let u = (t - self.span - BURST_HOLD) / self.back;
            Some(self.span * (1.0 - u * u * (3.0 - 2.0 * u)))
        } else {
            None
        }
    }
    /// Move a point that belongs to part `index`, whose resting centre is `home`.
    fn place(&self, index: usize, clock: f32, home: Pos2, point: Pos2) -> Pos2 {
        let shard = &self.shards[index];
        let turn = |v: Vec2, angle: f32| vec2(v.x * angle.cos() - v.y * angle.sin(), v.x * angle.sin() + v.y * angle.cos());
        let t = clock - shard.delay;
        match self.kind {
            BurstKind::Explode => {
                let k = ease(t / BURST_OUT);
                home + turn(point - home, shard.spin * k) + turn(shard.off * k, shard.curl * (1.0 - k))
            }
            BurstKind::Fall if t <= 0.0 => {
                // Still on the shelf: it rattles harder as its moment comes, and is still again
                // at the instant it lets go so the drop starts cleanly.
                let near = (1.0 + t / 0.5).clamp(0.0, 1.0);
                let shake = near * (1.0 - near) * 4.0;
                let phase = clock * 36.0 + index as f32 * 1.7;
                home + turn(point - home, phase.sin() * 0.075 * shake) + vec2((phase * 0.83).cos() * 2.4 * shake, 0.0)
            }
            BurstKind::Fall => {
                let drop = shard.off.y;
                let landing = (2.0 * drop / FALL_GRAVITY).sqrt();
                let fallen = if t < landing {
                    0.5 * FALL_GRAVITY * t * t
                } else {
                    // One small bounce off the floor, then rest.
                    let after = t - landing;
                    let hop = FALL_GRAVITY * landing * 0.26 * after - 0.5 * FALL_GRAVITY * after * after;
                    drop - hop.max(0.0)
                };
                let settle = ease(t / (landing + 0.3));
                home + turn(point - home, shard.spin * settle) + vec2(shard.off.x * settle, fallen)
            }
        }
    }
}

/// Tracks a hand-dragged window and counts direction reversals to notice a shake.
struct Drag {
    cursor0: (i32, i32),
    window0: (i32, i32),
    last: (i32, i32),
    heading: [i32; 2],
    travel: [f32; 2],
    flips: Vec<f64>,
}

// ---------- app state ----------

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Closed,
    List,
    Edit,
    Settings,
}

enum Row {
    Head(String),
    Item(usize),
}

#[derive(Default)]
struct EditBuf {
    /// None = a new prompt.
    target: Option<String>,
    category: String,
    title: String,
    sub: String,
    body: String,
    message: String,
    confirm_delete: bool,
}

enum Action {
    ToggleMini,
    ToggleList,
    ToggleSettings,
    ClosePanel,
    MoveTo(usize),
    Pick(usize),
    Copy(usize),
    CopyEn(usize),
    ToggleFav(usize),
    ToggleFavOnly,
    StartEdit(Option<usize>),
    SaveEdit,
    RevertEdit,
    DeleteEdit,
    Scale(f32),
    Pin(bool),
    Quit,
}

struct Io {
    ptr: Option<Pos2>,
    click: Option<Pos2>,
    hand: bool,
}
impl Io {
    fn rect(&mut self, r: Rect) -> (bool, bool) {
        let hovered = self.ptr.is_some_and(|p| r.contains(p));
        self.hand |= hovered;
        (hovered, self.click.is_some_and(|p| r.contains(p)))
    }
    fn circle(&mut self, c: Pos2, radius: f32) -> (bool, bool) {
        let hovered = self.ptr.is_some_and(|p| p.distance(c) <= radius);
        self.hand |= hovered;
        (hovered, self.click.is_some_and(|p| p.distance(c) <= radius))
    }
}

struct Fx {
    anim: bool,
    actions: Vec<Action>,
}

struct App {
    cats: Vec<Category>,
    base: Vec<Item>,
    items: Vec<Item>,
    saved: Saved,
    dirty: Option<f64>,
    window: Arc<Window>,
    quit: bool,
    placed: bool,
    started: Instant,
    last_frame: f64,
    request: Arc<AtomicU8>,
    hwnd: isize,
    _tray: Option<tray_icon::TrayIcon>,

    turn: i32,
    rot: f32,
    side: f32,
    sel: HashMap<String, String>,
    mini: bool,
    mini_t: f32,
    mini_shift: (i32, i32),
    puck_hover: f32,
    /// Whole-widget opacity; applied by the compositor when the frame is presented.
    op: f32,

    panel: Panel,
    panel_t: f32,
    panel_h: f32,
    scale_draft: Option<f32>,
    locked: bool,
    dwell: Option<f64>,
    content_at: f64,
    sel_at: f64,
    last_hot: f64,
    hl_y: f32,
    scroll: f32,
    search: String,
    fav_only: bool,
    edit: EditBuf,

    flash: Option<(f64, String, bool)>,
    rng: u32,
    sparks: Vec<Spark>,
    burst: Option<Burst>,
    snap: Option<f64>,
    taps: Vec<f64>,
    drag: Option<Drag>,
    turbo: bool,
    prev_rot: f32,
    /// Recent raw wheel notches (time, amount), for telling a flick from ordinary scrolling.
    notches: Vec<(f64, f32)>,
    wheel_acc: f32,
    wheel_at: f64,
}

impl App {
    fn new(ctx: &egui::Context, saved: Saved, window: Arc<Window>, hwnd: isize) -> Self {
        install_fonts(ctx);
        install_style(ctx);
        ctx.set_zoom_factor(saved.scale);
        ctx.options_mut(|o| o.zoom_with_keyboard = false);

        let (cats, base) = data::load_catalog();
        let items = data::merge(&cats, &base, &saved);
        let request = Arc::new(AtomicU8::new(0));
        let tray = build_tray(ctx, request.clone());
        let mini = saved.mini;
        Self {
            cats,
            base,
            items,
            saved,
            dirty: None,
            window,
            quit: false,
            placed: false,
            started: Instant::now(),
            last_frame: 0.0,
            request,
            hwnd,
            _tray: tray,
            turn: 0,
            rot: 0.0,
            side: 1.0,
            sel: HashMap::new(),
            mini,
            mini_t: if mini { 0.0 } else { 1.0 },
            mini_shift: (0, 0),
            puck_hover: 0.0,
            op: 1.0,
            panel: Panel::Closed,
            panel_t: 0.0,
            panel_h: PANEL_H,
            scale_draft: None,
            locked: false,
            dwell: None,
            content_at: 0.0,
            sel_at: 0.0,
            last_hot: 0.0,
            hl_y: -1.0,
            scroll: 0.0,
            search: String::new(),
            fav_only: false,
            edit: EditBuf::default(),
            flash: None,
            rng: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9e37_79b9, |d| d.subsec_nanos() | 1),
            sparks: Vec::new(),
            burst: None,
            snap: None,
            taps: Vec::new(),
            drag: None,
            turbo: false,
            prev_rot: 0.0,
            notches: Vec::new(),
            wheel_acc: 0.0,
            wheel_at: 0.0,
        }
    }

    // ----- model -----

    fn active(&self) -> usize {
        self.turn.rem_euclid(self.cats.len() as i32) as usize
    }
    fn is_fav(&self, k: usize) -> bool {
        self.saved.favorites.contains(&self.items[k].id)
    }
    /// Items of a genre, grouped by sub-genre in order of first appearance.
    fn ordered(&self, cat: &str) -> Vec<usize> {
        let mut subs: Vec<&str> = Vec::new();
        for item in self.items.iter().filter(|i| i.category == cat) {
            if !subs.contains(&item.sub.as_str()) {
                subs.push(&item.sub);
            }
        }
        let mut out = Vec::new();
        for sub in subs {
            out.extend(self.items.iter().enumerate().filter(|(_, i)| i.category == cat && i.sub == sub).map(|(k, _)| k));
        }
        out
    }
    fn current(&self) -> Option<usize> {
        let cat = &self.cats[self.active()];
        let order = self.ordered(&cat.id);
        let wanted = self.sel.get(&cat.id);
        order.iter().copied().find(|&k| Some(&self.items[k].id) == wanted).or(order.first().copied())
    }
    fn filtering(&self) -> bool {
        self.fav_only || !self.search.trim().is_empty()
    }
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if !self.filtering() {
            let cat = &self.cats[self.active()];
            let mut last: Option<&str> = None;
            for k in self.ordered(&cat.id) {
                let sub = self.items[k].sub.as_str();
                if last != Some(sub) {
                    rows.push(Row::Head(if sub.is_empty() { "その他".to_owned() } else { sub.to_owned() }));
                    last = Some(sub);
                }
                rows.push(Row::Item(k));
            }
            return rows;
        }
        let needle = self.search.trim().to_lowercase();
        for cat in &self.cats {
            let mut first = true;
            for k in self.ordered(&cat.id) {
                let item = &self.items[k];
                if self.fav_only && !self.is_fav(k) {
                    continue;
                }
                if !needle.is_empty() {
                    let hay = format!("{} {} {} {}", item.title, item.sub, item.body, cat.label).to_lowercase();
                    if !hay.contains(&needle) {
                        continue;
                    }
                }
                if first {
                    rows.push(Row::Head(cat.label.clone()));
                    first = false;
                }
                rows.push(Row::Item(k));
            }
        }
        rows
    }
    fn turn_to(&mut self, index: usize) {
        let n = self.cats.len() as i32;
        let mut delta = (index as i32 - self.active() as i32).rem_euclid(n);
        if delta > n / 2 {
            delta -= n;
        }
        self.turn += delta;
    }
    fn select(&mut self, k: usize, now: f64) {
        let (cat, id) = (self.items[k].category.clone(), self.items[k].id.clone());
        if let Some(index) = self.cats.iter().position(|c| c.id == cat) {
            self.turn_to(index);
        }
        self.sel.insert(cat, id);
        self.sel_at = now;
    }
    fn step_selection(&mut self, dir: i32, now: f64) {
        let list: Vec<usize> = self.rows().iter().filter_map(|r| if let Row::Item(k) = r { Some(*k) } else { None }).collect();
        if list.is_empty() {
            return;
        }
        let at = self.current().and_then(|cur| list.iter().position(|&k| k == cur));
        let next = match at {
            Some(p) => (p as i32 + dir).rem_euclid(list.len() as i32) as usize,
            None => 0,
        };
        self.select(list[next], now);
    }
    fn spin_genre(&mut self, dir: i32, now: f64) {
        self.turn += dir;
        self.genre_changed(now);
    }
    fn genre_changed(&mut self, now: f64) {
        self.content_at = now;
        self.sel_at = now;
        self.hl_y = -1.0;
        self.scroll = 0.0;
        if self.panel == Panel::Closed {
            self.dwell = Some(now);
        }
    }
    fn open_list(&mut self, now: f64, locked: bool) {
        if self.panel != Panel::List {
            self.panel = Panel::List;
            self.content_at = now;
            self.hl_y = -1.0;
        }
        self.locked |= locked;
        self.dwell = None;
    }
    fn close_panel(&mut self) {
        self.panel = Panel::Closed;
        self.locked = false;
        self.dwell = None;
    }
    fn touch(&mut self, now: f64) {
        self.dirty = Some(now);
    }
    fn rebuild(&mut self) {
        self.items = data::merge(&self.cats, &self.base, &self.saved);
    }

    // ----- play -----

    /// True when the wheel has been turned fast for the whole of the last TURBO_HOLD seconds.
    fn spin_sustained(&self, now: f64) -> bool {
        let slice = TURBO_HOLD / TURBO_SLICES as f64;
        let mut counts = [0.0_f32; TURBO_SLICES];
        for (at, amount) in &self.notches {
            let age = now - at;
            if (0.0..TURBO_HOLD).contains(&age) {
                counts[(age / slice) as usize] += amount;
            }
        }
        counts.iter().all(|count| *count >= TURBO_PER_SLICE)
    }

    /// Uniform random number in 0..1 (xorshift; quality is irrelevant here, only variety).
    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
    fn spark(&mut self, pos: Pos2, vel: Vec2, life: f32) {
        if self.sparks.len() < 400 {
            self.sparks.push(Spark { pos, vel, age: 0.0, life });
        }
    }
    /// A hard flick of the mouse wheel: several whole turns to a random genre, with sparks.
    fn turbo_spin(&mut self, dir: i32, now: f64) {
        let n = self.cats.len() as i32;
        let extra = n * 3 + (self.rand() * n as f32) as i32;
        self.turn += dir * extra;
        self.turbo = true;
        self.close_panel();
        self.genre_changed(now);
    }
    /// Throw every part of the wheel somewhere new. Targets, arcs, tumble and timing are all random.
    fn start_explode(&mut self, now: f64) {
        self.close_panel();
        let parts = self.cats.len() + 1 + CORNERS.len();
        let mut shards = Vec::with_capacity(parts);
        for _ in 0..parts {
            // Aim anywhere in the window, but always a real distance from the centre.
            let mut off = Vec2::ZERO;
            for _ in 0..8 {
                let target = pos2(lerp(70.0, WIN.x - 70.0, self.rand()), lerp(60.0, WIN.y - 60.0, self.rand()));
                off = target - C;
                if off.length() > 150.0 {
                    break;
                }
            }
            let sign = if self.rand() < 0.5 { -1.0 } else { 1.0 };
            shards.push(Shard { off, curl: (self.rand() - 0.5) * 2.2, spin: sign * lerp(0.8, 3.4, self.rand()), delay: self.rand() * BURST_STAGGER });
        }
        self.burst = Some(Burst { start: now, kind: BurstKind::Explode, span: BURST_OUT + BURST_STAGGER, back: 1.05, shards });
        for _ in 0..70 {
            let (angle, speed) = (self.rand() * 360.0, lerp(160.0, 720.0, self.rand()));
            let life = lerp(0.35, 0.9, self.rand());
            let from = C + polar(lerp(20.0, R_OUT, self.rand()), angle);
            self.spark(from, polar(speed, angle), life);
        }
    }
    /// Shaken loose: parts let go at random moments and drop to the floor, which is the bottom of
    /// the screen's work area when the wheel sits low, otherwise the bottom of the window.
    fn start_fall(&mut self, ctx: &egui::Context, now: f64) {
        self.close_panel();
        let ppp = ctx.pixels_per_point();
        let mut floor = WIN.y - 8.0;
        if let (Some(r), Some(w)) = (win::window_rect(self.hwnd), win::work_area(self.hwnd)) {
            floor = floor.min((w[3] - r[1]) as f32 / ppp - 4.0);
        }
        let n = self.cats.len();
        let step = 360.0 / n as f32;
        let mut shards = Vec::with_capacity(n + 1 + CORNERS.len());
        let mut span: f32 = 0.0;
        for index in 0..n + 1 + CORNERS.len() {
            // (resting centre, half height, how far it may wander sideways, how much it may tilt)
            let (home, half, wander, tilt) = if index < n {
                (C + polar((R_IN + R_OUT) / 2.0, self.rot + index as f32 * step), 30.0, 70.0, 1.3)
            } else if index == n {
                (C, R_CORE, 26.0, 0.0)
            } else {
                let (dx, dy) = CORNERS[index - n - 1];
                (C + vec2(dx, dy), 17.0, 90.0, 2.6)
            };
            // Things land at slightly different heights, as if on top of one another.
            let pile = if index == n { 0.0 } else { self.rand() * 44.0 };
            let drop = (floor - half - pile - home.y).max(6.0);
            let delay = 0.12 + self.rand() * 0.6;
            let shard = Shard { off: vec2((self.rand() - 0.5) * 2.0 * wander, drop), curl: 0.0, spin: (self.rand() - 0.5) * 2.0 * tilt, delay };
            span = span.max(delay + (2.0 * drop / FALL_GRAVITY).sqrt() + 0.42);
            shards.push(shard);
        }
        self.burst = Some(Burst { start: now, kind: BurstKind::Fall, span, back: 1.35, shards });
    }
    /// Move the window with the pointer ourselves (no OS move loop), so frames keep coming and a
    /// shake can be seen as it happens. Returns true when the motion amounts to a shake.
    fn follow_drag(&mut self, now: f64) -> bool {
        let Some(cursor) = win::cursor_pos() else { return false };
        let Some(drag) = self.drag.as_mut() else { return false };
        win::move_to(self.hwnd, drag.window0.0 + cursor.0 - drag.cursor0.0, drag.window0.1 + cursor.1 - drag.cursor0.1);
        let delta = [cursor.0 - drag.last.0, cursor.1 - drag.last.1];
        drag.last = cursor;
        for axis in 0..2 {
            if delta[axis].abs() < 2 {
                continue;
            }
            let heading = delta[axis].signum();
            if drag.heading[axis] != 0 && heading != drag.heading[axis] {
                // Only a decent stroke counts, so an ordinary wobbly drag is not a shake.
                if drag.travel[axis] >= 26.0 {
                    drag.flips.push(now);
                }
                drag.travel[axis] = 0.0;
            }
            drag.heading[axis] = heading;
            drag.travel[axis] += delta[axis].abs() as f32;
        }
        drag.flips.retain(|at| now - at < 1.0);
        if drag.flips.len() >= 5 {
            drag.flips.clear();
            return true;
        }
        false
    }
    fn draw_sparks(&mut self, painter: &egui::Painter, dt: f32) -> bool {
        for spark in &mut self.sparks {
            spark.age += dt;
            spark.vel.y += 640.0 * dt;
            spark.vel *= 1.0 - (1.4 * dt).min(0.9);
            spark.pos += spark.vel * dt;
        }
        self.sparks.retain(|spark| spark.age < spark.life);
        let ember = Color32::from_rgb(238, 112, 48);
        for spark in &self.sparks {
            let t = spark.age / spark.life;
            let color = if t < 0.35 { mix(Color32::WHITE, GOLD, t / 0.35) } else { mix(GOLD, ember, (t - 0.35) / 0.65) };
            let tail = spark.pos - spark.vel * 0.04;
            painter.line_segment([tail, spark.pos], Stroke::new(lerp(2.4, 0.8, t), fade(color, (1.0 - t).powf(1.4))));
        }
        !self.sparks.is_empty()
    }

    // ----- native window -----

    fn restore_window(&mut self, ctx: &egui::Context) {
        // Layout 2 made the window 160 points taller with the wheel still in its middle; shift
        // positions saved by the older layout so the wheel stays where the user left it.
        if self.saved.layout < 2 {
            if let Some(pos) = self.saved.pos.as_mut() {
                pos[1] -= (80.0 * ctx.pixels_per_point()).round() as i32;
            }
            self.saved.layout = 2;
            self.dirty = Some(0.0);
        }
        if let Some([x, y]) = self.saved.pos {
            win::move_to(self.hwnd, x, y);
        }
        // An open wheel must be fully on screen; this is a correction, not a temporary nudge.
        if !self.mini {
            self.fit_wheel(ctx);
            self.mini_shift = (0, 0);
        }
        // If the saved spot is no longer on any screen (monitor unplugged), come back to the middle.
        if let (Some(r), Some(w)) = (win::window_rect(self.hwnd), win::work_area(self.hwnd)) {
            let ppp = ctx.pixels_per_point();
            let (cx, cy) = (r[0] as f32 + C.x * ppp, r[1] as f32 + C.y * ppp);
            if cx < w[0] as f32 || cx > w[2] as f32 || cy < w[1] as f32 || cy > w[3] as f32 {
                self.center_window(ctx);
            }
        }
    }
    fn center_window(&mut self, ctx: &egui::Context) {
        if let Some(w) = win::work_area(self.hwnd) {
            let ppp = ctx.pixels_per_point();
            let x = (w[0] + w[2]) as f32 / 2.0 - C.x * ppp;
            let y = (w[1] + w[3]) as f32 / 2.0 - C.y * ppp;
            win::move_to(self.hwnd, x.round() as i32, y.round() as i32);
            self.mini_shift = (0, 0);
        }
    }
    /// The puck may sit right at a screen edge. When it opens, nudge the window so the whole
    /// wheel is on screen, and remember the nudge so collapsing puts the puck back.
    fn fit_wheel(&mut self, ctx: &egui::Context) {
        let (Some(r), Some(w)) = (win::window_rect(self.hwnd), win::work_area(self.hwnd)) else { return };
        let ppp = ctx.pixels_per_point();
        let (cx, cy) = (r[0] as f32 + C.x * ppp, r[1] as f32 + C.y * ppp);
        let reach = (R_OUT + 26.0) * ppp;
        let dx = (w[0] as f32 + reach - cx).max(0.0) + (w[2] as f32 - reach - cx).min(0.0);
        let dy = (w[1] as f32 + reach - cy).max(0.0) + (w[3] as f32 - reach - cy).min(0.0);
        let shift = (dx.round() as i32, dy.round() as i32);
        if shift != (0, 0) {
            win::move_to(self.hwnd, r[0] + shift.0, r[1] + shift.1);
            self.mini_shift = shift;
        }
    }
    fn set_mini(&mut self, mini: bool, ctx: &egui::Context, now: f64) {
        if self.mini == mini {
            return;
        }
        self.mini = mini;
        self.saved.mini = mini;
        if mini {
            self.close_panel();
            if self.mini_shift != (0, 0) {
                if let Some(r) = win::window_rect(self.hwnd) {
                    win::move_to(self.hwnd, r[0] - self.mini_shift.0, r[1] - self.mini_shift.1);
                }
                self.mini_shift = (0, 0);
            }
        } else {
            self.fit_wheel(ctx);
        }
        self.touch(now);
    }
    /// Panel opens on whichever side of the wheel has room on this screen.
    fn pick_side(&self, ctx: &egui::Context) -> f32 {
        let (Some(r), Some(w)) = (win::window_rect(self.hwnd), win::work_area(self.hwnd)) else { return 1.0 };
        let ppp = ctx.pixels_per_point();
        let cx = r[0] as f32 + C.x * ppp;
        let need = (R_OUT + PANEL_GAP + PANEL_W + 6.0) * ppp;
        if cx + need <= w[2] as f32 || cx - need < w[0] as f32 {
            1.0
        } else {
            -1.0
        }
    }
    fn apply_scale(&mut self, scale: f32, ctx: &egui::Context, now: f64) {
        let scale = ((scale * 10.0).round() / 10.0).clamp(0.7, 1.6);
        let old = self.saved.scale;
        if (scale - old).abs() < 0.01 {
            return;
        }
        self.saved.scale = scale;
        ctx.set_zoom_factor(scale);
        // Grow or shrink around the wheel's centre, in one native call.
        if let Some(r) = win::window_rect(self.hwnd) {
            let (w, h) = ((r[2] - r[0]) as f32, (r[3] - r[1]) as f32);
            let (nw, nh) = ((w * scale / old).round(), (h * scale / old).round());
            win::move_resize(self.hwnd, r[0] - ((nw - w) / 2.0) as i32, r[1] - ((nh - h) / 2.0) as i32, nw as i32, nh as i32);
        }
        self.touch(now);
    }
    fn track_position(&mut self, now: f64) {
        if self.hwnd == 0 {
            return;
        }
        if let Some(r) = win::window_rect(self.hwnd) {
            let pos = [r[0] - self.mini_shift.0, r[1] - self.mini_shift.1];
            if self.saved.pos != Some(pos) {
                self.saved.pos = Some(pos);
                self.touch(now);
            }
        }
    }
    // ----- actions -----

    fn copy(&mut self, ctx: &egui::Context, text: String, message: &str, now: f64) {
        ctx.copy_text(text);
        self.flash = Some((now, message.to_owned(), true));
    }
    fn apply(&mut self, action: Action, ctx: &egui::Context, now: f64) {
        match action {
            Action::ToggleMini => self.set_mini(!self.mini, ctx, now),
            Action::ToggleList => {
                if self.panel == Panel::List && self.locked {
                    self.close_panel();
                } else {
                    self.panel = Panel::Closed;
                    self.open_list(now, true);
                }
            }
            Action::ToggleSettings => {
                if self.panel == Panel::Settings {
                    self.close_panel();
                } else {
                    self.panel = Panel::Settings;
                    self.locked = true;
                    self.dwell = None;
                }
            }
            Action::ClosePanel => self.close_panel(),
            Action::MoveTo(index) => {
                if index != self.active() {
                    self.turn_to(index);
                    self.genre_changed(now);
                } else if self.panel == Panel::Closed {
                    self.open_list(now, true);
                } else {
                    self.close_panel();
                }
            }
            Action::Pick(k) => {
                self.select(k, now);
                self.locked = true;
                self.copy(ctx, self.items[k].body.clone(), "✓ コピーしました", now);
            }
            Action::Copy(k) => self.copy(ctx, self.items[k].body.clone(), "✓ コピーしました", now),
            Action::CopyEn(k) => {
                if let Some(text) = self.items[k].body_en.clone() {
                    self.copy(ctx, text, "✓ 英語原文をコピー", now);
                }
            }
            Action::ToggleFav(k) => {
                let id = self.items[k].id.clone();
                match self.saved.favorites.iter().position(|f| *f == id) {
                    Some(p) => {
                        self.saved.favorites.remove(p);
                    }
                    None => self.saved.favorites.push(id),
                }
                self.touch(now);
            }
            Action::ToggleFavOnly => {
                self.fav_only = !self.fav_only;
                self.scroll = 0.0;
                self.hl_y = -1.0;
                self.content_at = now;
            }
            Action::StartEdit(target) => {
                let cat = self.cats[self.active()].id.clone();
                self.edit = match target {
                    Some(k) => {
                        let item = &self.items[k];
                        EditBuf { target: Some(item.id.clone()), category: item.category.clone(), title: item.title.clone(), sub: item.sub.clone(), body: item.body.clone(), ..Default::default() }
                    }
                    None => EditBuf { category: cat, sub: self.current().map(|k| self.items[k].sub.clone()).unwrap_or_default(), ..Default::default() },
                };
                self.panel = Panel::Edit;
                self.locked = true;
            }
            Action::SaveEdit => self.save_edit(now),
            Action::RevertEdit => {
                if let Some(id) = self.edit.target.clone() {
                    self.saved.edits.remove(&id);
                    self.after_edit(Some(id), now);
                }
            }
            Action::DeleteEdit => {
                if let Some(id) = self.edit.target.clone() {
                    self.saved.added.retain(|p| p.id != id);
                    self.saved.favorites.retain(|f| *f != id);
                    self.after_edit(None, now);
                }
            }
            Action::Scale(scale) => self.apply_scale(scale, ctx, now),
            Action::Pin(pinned) => {
                self.saved.pinned = pinned;
                win::set_topmost(self.hwnd, pinned);
                self.touch(now);
            }
            Action::Quit => self.request.store(REQ_QUIT, Ordering::Relaxed),
        }
    }
    fn save_edit(&mut self, now: f64) {
        let (title, sub, body) = (self.edit.title.trim().to_owned(), self.edit.sub.trim().to_owned(), self.edit.body.trim().to_owned());
        if title.is_empty() || body.is_empty() {
            self.edit.message = "タイトルと本文を入れてください。".to_owned();
            return;
        }
        let id = match self.edit.target.clone() {
            None => {
                let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                let id = format!("user-{millis:x}");
                self.saved.added.push(UserPrompt { id: id.clone(), category: self.edit.category.clone(), sub, title, prompt: body });
                id
            }
            Some(id) => {
                if let Some(own) = self.saved.added.iter_mut().find(|p| p.id == id) {
                    own.title = title;
                    own.sub = sub;
                    own.prompt = body;
                } else {
                    self.saved.edits.insert(id.clone(), EditRec { title, sub, prompt: body });
                }
                id
            }
        };
        self.after_edit(Some(id), now);
    }
    fn after_edit(&mut self, focus: Option<String>, now: f64) {
        self.rebuild();
        self.panel = Panel::List;
        self.content_at = now;
        self.hl_y = -1.0;
        if let Some(k) = focus.and_then(|id| self.items.iter().position(|i| i.id == id)) {
            self.select(k, now);
        }
        self.touch(now);
    }

    // ----- input -----

    fn wheel_step(&mut self, ui: &egui::Ui, now: f64) -> (i32, bool) {
        let (dy, ctrl) = ui.input(|i| {
            let mut dy = 0.0;
            for event in &i.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = event {
                    dy += match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Point => delta.y / 40.0,
                        egui::MouseWheelUnit::Page => delta.y * 3.0,
                    };
                }
            }
            (dy, i.modifiers.ctrl)
        });
        // Notches are queued, not dropped: a flick of six notches becomes six steps in quick
        // succession, which is also how a hard spin is recognised. Reversing clears the queue.
        if dy != 0.0 && dy.signum() != self.wheel_acc.signum() {
            self.wheel_acc = 0.0;
        }
        self.wheel_acc = (self.wheel_acc + dy).clamp(-8.0, 8.0);
        if dy != 0.0 {
            self.notches.push((now, dy.abs()));
        }
        self.notches.retain(|(at, _)| now - at < TURBO_HOLD);
        if self.wheel_acc.abs() >= 1.0 && now - self.wheel_at >= 0.07 {
            let dir = -self.wheel_acc.signum() as i32;
            self.wheel_acc -= self.wheel_acc.signum();
            self.wheel_at = now;
            return (dir, ctrl);
        }
        (0, ctrl)
    }

    // ----- drawing -----

    fn draw(&mut self, ui: &mut egui::Ui, now: f64, dt: f32, hot: bool, fx: &mut Fx) {
        let ctx = ui.ctx().clone();
        let bg = ui.interact(ui.max_rect(), ui.id().with("background"), Sense::click_and_drag());
        if bg.drag_started() {
            if let (Some(cursor), Some(r)) = (win::cursor_pos(), win::window_rect(self.hwnd)) {
                self.mini_shift = (0, 0);
                self.drag = Some(Drag { cursor0: cursor, window0: (r[0], r[1]), last: cursor, heading: [0; 2], travel: [0.0; 2], flips: Vec::new() });
            }
        }
        if self.drag.is_some() {
            if !ui.input(|i| i.pointer.primary_down()) {
                self.drag = None;
            } else {
                fx.anim = true;
                if self.follow_drag(now) && self.saved.play && self.burst.is_none() && !self.mini && self.mini_t > 0.98 {
                    self.start_fall(&ctx, now);
                }
            }
        }
        // While the wheel is in pieces nothing on it can be pointed at.
        let burst_clock = match &self.burst {
            Some(burst) => {
                let clock = burst.clock((now - burst.start) as f32);
                if clock.is_none() {
                    self.snap = Some(now);
                }
                clock
            }
            None => None,
        };
        if burst_clock.is_none() {
            self.burst = None;
        } else {
            fx.anim = true;
        }
        let scattered = self.burst.is_some();
        let ptr = if hot && !scattered { ui.input(|i| i.pointer.hover_pos()) } else { None };
        let click = if bg.clicked() && !scattered { bg.interact_pointer_pos() } else { None };
        if let Some(p) = click {
            if self.saved.play && !self.mini && p.distance(C) <= R_OUT + 10.0 {
                self.taps.push(now);
                self.taps.retain(|at| now - at <= 2.0);
                if self.taps.len() >= 10 {
                    self.taps.clear();
                    self.start_explode(now);
                }
            }
        }
        let mut io = Io { ptr, click, hand: false };
        let typing = ctx.egui_wants_keyboard_input();
        if hot {
            self.last_hot = now;
        }

        // Idle translucency: solid while in use, see-through when left alone.
        let engaged = hot || typing || (self.panel != Panel::Closed && self.locked);
        fx.anim |= approach(&mut self.op, if engaged { 1.0 } else { self.saved.opacity }, dt, 9.0);
        let painter = ui.painter().clone();

        if self.panel == Panel::Closed && self.panel_t == 0.0 && self.hwnd != 0 && hot {
            self.side = self.pick_side(&ctx);
        }
        fx.anim |= approach(&mut self.mini_t, if self.mini { 0.0 } else { 1.0 }, dt, 10.0);
        // Opening unfolds at a readable pace; putting the list away is brisk, about twice as fast.
        let (panel_goal, panel_speed) = if self.panel == Panel::Closed { (0.0, 19.0) } else { (1.0, 8.5) };
        fx.anim |= approach(&mut self.panel_t, panel_goal, dt, panel_speed);
        let m = ease(self.mini_t);
        let n = self.cats.len();
        let step = 360.0 / n as f32;
        // The active genre faces the panel, so the list visibly grows out of it.
        let facing = if self.side > 0.0 { 0.0 } else { 180.0 };
        let rot_target = facing - self.turn as f32 * step - (1.0 - m) * 140.0;
        fx.anim |= approach(&mut self.rot, rot_target, dt, if self.turbo { 3.0 } else { 11.0 });
        let omega = (self.rot - self.prev_rot) / dt; // degrees per second, clockwise positive
        self.prev_rot = self.rot;
        let blur = if self.turbo { (omega.abs() / 900.0).clamp(0.0, 1.0) } else { 0.0 };
        if self.turbo {
            if (self.rot - rot_target).abs() < 1.5 {
                self.turbo = false;
            } else {
                // The list waits until the dial has come to rest.
                if self.dwell.is_some() {
                    self.dwell = Some(now);
                }
                // Sparks fly off the rim where the dial passes the pointer, thrown along the direction of travel.
                let heading = omega.signum();
                let count = ((omega.abs() / 420.0) as usize).min(7);
                for _ in 0..count {
                    let at = C + polar(R_OUT + 3.0, facing + (self.rand() - 0.5) * 10.0);
                    let tangent = polar(1.0, facing + 90.0 * heading);
                    let out = polar(1.0, facing);
                    let vel = tangent * lerp(180.0, 620.0, self.rand()) * blur.max(0.35) + out * lerp(30.0, 260.0, self.rand()) + vec2(0.0, -lerp(0.0, 120.0, self.rand()));
                    let life = lerp(0.25, 0.75, self.rand());
                    self.spark(at, vel, life);
                }
                // A second, smaller spray off the top of the dial.
                for _ in 0..count.div_ceil(3) {
                    let at = C + polar(R_OUT + 2.0, -90.0 + (self.rand() - 0.5) * 8.0);
                    let tangent = polar(1.0, -90.0 + 90.0 * heading);
                    let vel = tangent * lerp(90.0, 300.0, self.rand()) * blur.max(0.35) + vec2(0.0, -lerp(20.0, 150.0, self.rand()));
                    let life = lerp(0.18, 0.45, self.rand());
                    self.spark(at, vel, life);
                }
            }
        }

        let panel_full = panel_rect(self.side);
        let over_panel = self.panel != Panel::Closed && ptr.is_some_and(|p| panel_full.contains(p));
        let dist = ptr.map(|p| p.distance(C));

        // ---- wheel ----
        let (dir, ctrl) = self.wheel_step(ui, now);
        if self.wheel_acc.abs() >= 1.0 {
            fx.anim = true; // queued notches still to play out
        }
        if dir != 0 {
            if let Some(d) = dist {
                if ctrl {
                    fx.actions.push(Action::Scale(self.saved.scale - dir as f32 * 0.1));
                } else if self.mini {
                    fx.actions.push(Action::ToggleMini);
                } else if over_panel {
                    if self.panel == Panel::List {
                        self.step_selection(dir, now);
                        self.locked = true;
                    }
                } else if d <= R_IN {
                    self.step_selection(dir, now);
                    self.open_list(now, false);
                } else if d <= R_OUT + 10.0 {
                    // Only a spin that is kept up lets the dial run (see TURBO_HOLD).
                    if self.spin_sustained(now) && self.saved.play {
                        self.notches.clear();
                        self.wheel_acc = 0.0;
                        self.turbo_spin(dir, now);
                    } else {
                        self.spin_genre(dir, now);
                    }
                }
            }
        }

        // ---- keyboard ----
        if !typing {
            let key = |k: egui::Key| ui.input(|i| i.key_pressed(k));
            if key(egui::Key::Escape) {
                if self.panel != Panel::Closed {
                    self.close_panel();
                } else {
                    fx.actions.push(Action::ToggleMini);
                }
            }
            if !self.mini {
                if key(egui::Key::ArrowRight) {
                    self.spin_genre(1, now);
                }
                if key(egui::Key::ArrowLeft) {
                    self.spin_genre(-1, now);
                }
                if key(egui::Key::ArrowDown) {
                    self.step_selection(1, now);
                    self.open_list(now, true);
                }
                if key(egui::Key::ArrowUp) {
                    self.step_selection(-1, now);
                    self.open_list(now, true);
                }
                if key(egui::Key::Enter) {
                    if let Some(k) = self.current() {
                        fx.actions.push(Action::Copy(k));
                    }
                }
            }
        } else if self.panel == Panel::Edit && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.panel = Panel::List;
        }

        // ---- the 1.5 s rest that opens the list ----
        if let Some(started) = self.dwell {
            if !hot || self.mini {
                self.dwell = None;
            } else if now - started >= DWELL {
                self.open_list(now, false);
            } else {
                fx.anim = true;
            }
        }
        // A list that was only peeked at folds away once the pointer has gone.
        if self.panel == Panel::List && !self.locked && !typing {
            if now - self.last_hot > 0.9 {
                self.close_panel();
                fx.anim = true;
            } else if !hot {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }

        let active = self.active();
        let col = hex(&self.cats[active].color);
        let current = self.current();

        // ---- ring ----
        let hover_seg = dist.filter(|d| !self.mini && *d > R_IN && *d <= R_OUT + 6.0 && !over_panel).and_then(|_| {
            let p = ptr?;
            let angle = (p.y - C.y).atan2(p.x - C.x).to_degrees();
            Some((((angle - self.rot) / step).round() as i32).rem_euclid(n as i32) as usize)
        });
        if hover_seg.is_some() {
            io.hand = true;
        }
        if let (Some(seg), Some(p)) = (hover_seg, io.click) {
            let d = p.distance(C);
            if d > R_IN && d <= R_OUT + 6.0 {
                fx.actions.push(Action::MoveTo(seg));
            }
        }
        let (puck_hovered, puck_clicked) = if self.mini { io.circle(C, R_PUCK + 6.0) } else { (false, false) };
        if puck_clicked {
            fx.actions.push(Action::ToggleMini);
        }
        fx.anim |= approach(&mut self.puck_hover, if puck_hovered { 1.0 } else { 0.0 }, dt, 14.0);
        let r_out = lerp(R_PUCK + 3.0 * self.puck_hover, R_OUT, m);
        let r_in = lerp(R_PUCK - 11.0, R_IN, m);
        let label_alpha = ((m - 0.55) / 0.45).clamp(0.0, 1.0);
        // Part indices for the burst: genres first, then the centre, then the four corner buttons.
        let burst = self.burst.take();
        let place = |index: usize, home: Pos2, point: Pos2| -> Pos2 {
            match (&burst, burst_clock) {
                (Some(burst), Some(clock)) => burst.place(index, clock, home, point),
                _ => point,
            }
        };
        for i in 0..n {
            let c = hex(&self.cats[i].color);
            let mid = self.rot + i as f32 * step;
            let is_active = i == active;
            // A shade lighter than the centre so the dial still reads on a dark desktop.
            let dim = if hover_seg == Some(i) { Color32::from_rgb(52, 47, 39) } else { Color32::from_rgb(34, 31, 26) };
            let fill = if is_active { c } else { mix(c, dim, m) };
            let outer = r_out + if is_active { 5.0 * m } else { 0.0 };
            let gap = GAP_DEG * m;
            let (from, to) = (mid - step / 2.0 + gap / 2.0, mid + step / 2.0 - gap / 2.0);
            let home = C + polar((r_in + outer) / 2.0, mid);
            let moved = |points: Vec<Pos2>| -> Vec<Pos2> { points.into_iter().map(|point| place(i, home, point)).collect() };
            painter.add(egui::Shape::line(moved(arc_points(C, (r_in + outer) / 2.0, from, to)), Stroke::new(outer - r_in, fill)));
            if !is_active && m > 0.3 {
                painter.add(egui::Shape::line(moved(arc_points(C, outer - 2.0, from, to)), Stroke::new(4.0_f32, fade(c, (m - 0.3) / 0.7))));
            }
            // At speed the labels smear away, which reads as motion blur.
            let label_alpha = label_alpha * (1.0 - 0.9 * blur);
            if label_alpha > 0.0 {
                let at = place(i, home, home);
                let ink = fade(if is_active { DARK } else { SOFT }, label_alpha);
                painter.text(at - vec2(0.0, 8.0), Align2::CENTER_CENTER, &self.cats[i].icon, font(16.0, false), fade(if is_active { DARK } else { c }, label_alpha));
                painter.text(at + vec2(0.0, 10.0), Align2::CENTER_CENTER, &self.cats[i].short, font(10.5, true), ink);
            }
        }

        // Speed streaks around the rim while the dial is running.
        if blur > 0.05 {
            for k in 0..3 {
                let from = self.rot * 0.6 + k as f32 * 120.0;
                painter.add(egui::Shape::line(arc_points(C, R_OUT + 9.0, from, from + 50.0 * blur), Stroke::new(2.0_f32, fade(TEXT, 0.4 * blur))));
            }
        }

        // ---- core ----
        let core_at = place(n, C, C);
        let (core_hovered, core_clicked) = if self.mini { (false, false) } else { io.circle(C, R_CORE) };
        if core_clicked {
            if let Some(k) = current {
                fx.actions.push(Action::Copy(k));
            }
        }
        let r_core = lerp(R_PUCK - 12.0, R_CORE, m);
        painter.circle_filled(core_at, r_core, if core_hovered { INK2 } else { INK });
        painter.circle_stroke(core_at, r_core, Stroke::new(1.0_f32, fade(LINE, label_alpha)));
        if m < 0.6 {
            painter.text(core_at, Align2::CENTER_CENTER, &self.cats[active].icon, font(14.0, false), fade(col, 1.0 - m / 0.6));
        }
        if label_alpha > 0.0 {
            let order = self.ordered(&self.cats[active].id);
            let position = current.and_then(|k| order.iter().position(|&o| o == k)).map_or(0, |p| p + 1);
            let head = format!("{}  {}/{}", self.cats[active].short, position, order.len());
            painter.text(core_at - vec2(0.0, 41.0), Align2::CENTER_CENTER, head, num(12.0), fade(col, label_alpha));
            let title = current.map_or("プロンプトなし", |k| self.items[k].title.as_str());
            let g = galley(ui, title, display(16.0), fade(TEXT, label_alpha), 138.0, 3, Some(22.0), true);
            painter.galley(pos2(core_at.x, core_at.y - g.size().y / 2.0 + 1.0), g, TEXT);
            painter.text(core_at + vec2(0.0, 44.0), Align2::CENTER_CENTER, "クリックでコピー", font(10.0, false), fade(MUTED, label_alpha));
        }
        // Countdown sweep: shows that the list is about to open, and when.
        if let Some(started) = self.dwell {
            let progress = ((now - started) / DWELL) as f32;
            let sweep = arc_points(C, R_CORE + 6.0, -90.0, -90.0 + 360.0 * progress.clamp(0.0, 1.0));
            painter.add(egui::Shape::line(sweep, Stroke::new(2.5_f32, fade(col, 0.9))));
        }
        // Copy feedback: the centre fills with the genre colour and a ripple runs out through the ring.
        if let Some((started, message, ok)) = self.flash.clone() {
            let t = (now - started) as f32;
            if t > 1.15 {
                self.flash = None;
            } else {
                fx.anim = true;
                let alpha = (t / 0.09).min(1.0) * ((1.15 - t) / 0.25).min(1.0);
                let tint = if ok { col } else { BAD };
                painter.circle_filled(core_at, r_core, fade(tint, alpha));
                painter.text(core_at, Align2::CENTER_CENTER, message, display(15.0), fade(DARK, alpha));
                let wave = ease(t / 0.55);
                if wave < 1.0 {
                    painter.circle_stroke(C, lerp(R_CORE, R_OUT + 14.0, wave), Stroke::new(lerp(5.0, 1.0, wave), fade(tint, 0.8 * (1.0 - wave))));
                }
            }
        }

        // ---- corner buttons ----
        if label_alpha > 0.0 {
            let fav_now = current.is_some_and(|k| self.is_fav(k));
            for (index, (dx, dy)) in CORNERS.iter().enumerate() {
                let rest = C + vec2(*dx, *dy);
                let at = place(n + 1 + index, rest, rest);
                let (hovered, clicked) = if self.mini { (false, false) } else { io.circle(rest, 18.0) };
                let on = match index {
                    1 => self.panel == Panel::Settings,
                    2 => self.panel == Panel::List && self.locked,
                    3 => fav_now,
                    _ => false,
                };
                let tint = if index == 3 { GOLD } else { col };
                let ink = fade(if on { tint } else if hovered { TEXT } else { SOFT }, label_alpha);
                painter.circle_filled(at, 17.0, fade(if hovered { INK2 } else { INK }, label_alpha));
                painter.circle_stroke(at, 17.0, Stroke::new(1.0_f32, fade(if on { tint } else { LINE }, label_alpha)));
                let line = Stroke::new(1.7_f32, ink);
                match index {
                    0 => {
                        painter.line_segment([at - vec2(5.5, 0.0), at + vec2(5.5, 0.0)], line);
                    }
                    1 => {
                        for (y, knob) in [(-3.5, 2.5), (3.5, -2.5)] {
                            painter.line_segment([at + vec2(-6.0, y), at + vec2(6.0, y)], line);
                            painter.circle_filled(at + vec2(knob, y), 2.6, ink);
                        }
                    }
                    2 => {
                        for y in [-4.5, 0.0, 4.5] {
                            painter.line_segment([at + vec2(-6.0, y), at + vec2(6.0, y)], line);
                        }
                    }
                    _ => {
                        painter.text(at, Align2::CENTER_CENTER, if fav_now { "★" } else { "☆" }, font(15.0, false), ink);
                    }
                }
                if clicked {
                    match index {
                        0 => fx.actions.push(Action::ToggleMini),
                        1 => fx.actions.push(Action::ToggleSettings),
                        2 => fx.actions.push(Action::ToggleList),
                        _ => {
                            if let Some(k) = current {
                                fx.actions.push(Action::ToggleFav(k));
                            }
                        }
                    }
                }
            }
        }

        self.burst = burst;
        if self.panel_t > 0.001 {
            self.draw_panel(ui, &painter, &mut io, fx, now, dt, col, panel_full);
        }
        // The click of everything landing back in place.
        if let Some(at) = self.snap {
            let t = (now - at) as f32 / 0.4;
            if t >= 1.0 {
                self.snap = None;
            } else {
                fx.anim = true;
                painter.circle_stroke(C, lerp(R_OUT + 2.0, R_OUT + 20.0, ease(t)), Stroke::new(lerp(3.0, 0.5, t), fade(col, 0.7 * (1.0 - t))));
            }
        }
        fx.anim |= self.draw_sparks(&painter, dt);
        if io.hand {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_panel(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, io: &mut Io, fx: &mut Fx, now: f64, dt: f32, col: Color32, full: Rect) {
        let side = self.side;
        // The card unfolds from the active genre: a small pill at the ring that widens and grows tall.
        let wanted = match self.panel {
            Panel::Settings => SETTINGS_H,
            Panel::Closed => self.panel_h,
            _ => full.height(),
        };
        if self.panel_t < 0.05 {
            self.panel_h = wanted;
        }
        fx.anim |= approach(&mut self.panel_h, wanted, dt, 13.0);
        let p = ease_back(self.panel_t).max(0.0);
        let width = lerp(30.0, PANEL_W, p.min(1.04));
        let height = lerp(70.0, self.panel_h, ease(self.panel_t * 1.15));
        let edge = if side > 0.0 { full.left() } else { full.right() };
        let card = if side > 0.0 {
            Rect::from_min_max(pos2(edge, C.y - height / 2.0), pos2(edge + width, C.y + height / 2.0))
        } else {
            Rect::from_min_max(pos2(edge - width, C.y - height / 2.0), pos2(edge, C.y + height / 2.0))
        };
        let shell = (self.panel_t * 3.0).min(1.0);
        painter.rect_filled(card.expand(1.0), 13.0, fade(LINE, shell));
        painter.rect_filled(card, 12.0, fade(INK, shell));
        // A pigment notch ties the card to the active segment it grew out of.
        let tip = pos2(edge - side * 9.0, C.y);
        painter.add(egui::Shape::convex_polygon(vec![pos2(edge + side * 0.5, C.y - 8.0), tip, pos2(edge + side * 0.5, C.y + 8.0)], fade(col, shell), Stroke::NONE));

        let content = ((self.panel_t - 0.5) / 0.5).clamp(0.0, 1.0);
        if content <= 0.0 {
            return;
        }
        let inner = Rect::from_center_size(pos2(full.center().x, C.y), vec2(PANEL_W, self.panel_h)).shrink(14.0);
        match self.panel {
            Panel::Edit => self.draw_editor(ui, fx, inner, card, content),
            Panel::Settings => self.draw_settings(ui, &painter.with_clip_rect(card), fx, inner, content, col),
            _ => self.draw_list(ui, &painter.with_clip_rect(card), io, fx, now, dt, col, inner, card, content),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_list(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, io: &mut Io, fx: &mut Fx, now: f64, dt: f32, col: Color32, inner: Rect, card: Rect, alpha: f32) {
        let (left, right, top, bottom) = (inner.left(), inner.right(), inner.top(), inner.bottom());
        let side = self.side;
        let rows = self.rows();
        let count = rows.iter().filter(|r| matches!(r, Row::Item(_))).count();
        let current = self.current();
        let since = (now - self.content_at) as f32;
        if since < 1.4 {
            fx.anim = true;
        }

        // header
        let heading = if !self.search.trim().is_empty() { "検索結果".to_owned() } else if self.fav_only { "お気に入り".to_owned() } else { self.cats[self.active()].label.clone() };
        painter.circle_filled(pos2(left + 5.0, top + 12.0), 4.0, fade(col, alpha));
        painter.text(pos2(left + 16.0, top + 12.0), Align2::LEFT_CENTER, heading, display(15.0), fade(TEXT, alpha));
        painter.text(pos2(right - 62.0, top + 12.0), Align2::RIGHT_CENTER, format!("{count}"), num(13.0), fade(col, alpha));
        let fav_rect = Rect::from_min_size(pos2(right - 54.0, top), vec2(24.0, 24.0));
        let add_rect = Rect::from_min_size(pos2(right - 24.0, top), vec2(24.0, 24.0));
        for (rect, glyph, on, action) in [(fav_rect, "★", self.fav_only, 0), (add_rect, "＋", false, 1)] {
            let (hovered, clicked) = io.rect(rect);
            painter.rect_filled(rect, 8.0, fade(if on { mix(INK, GOLD, 0.16) } else if hovered { INK2 } else { DEEP }, alpha));
            painter.text(rect.center(), Align2::CENTER_CENTER, glyph, font(13.0, false), fade(if on { GOLD } else if hovered { TEXT } else { MUTED }, alpha));
            if clicked {
                fx.actions.push(if action == 0 { Action::ToggleFavOnly } else { Action::StartEdit(None) });
            }
        }

        // search
        let search_rect = Rect::from_min_max(pos2(left, top + 32.0), pos2(right, top + 58.0));
        if alpha > 0.95 {
            let before = self.search.clone();
            let field = egui::TextEdit::singleline(&mut self.search).hint_text("全ジャンルから検索…").font(font(12.0, false)).margin(vec2(9.0, 5.0));
            let response = ui.put(search_rect, field);
            if response.has_focus() {
                self.locked = true;
            }
            if before != self.search {
                self.scroll = 0.0;
                self.hl_y = -1.0;
                self.content_at = now - 0.2;
            }
        } else {
            painter.rect_filled(search_rect, 8.0, fade(DEEP, alpha));
        }

        // list
        let list = Rect::from_min_max(pos2(left, top + 66.0), pos2(right, bottom - 124.0));
        let clip = painter.with_clip_rect(list.intersect(card));
        const HEAD_H: f32 = 24.0;
        const ITEM_H: f32 = 27.0;
        let mut y = 0.0;
        let mut selected_y = None;
        let mut layout = Vec::with_capacity(rows.len());
        for row in &rows {
            let h = if matches!(row, Row::Head(_)) { HEAD_H } else { ITEM_H };
            if let Row::Item(k) = row {
                if Some(*k) == current {
                    selected_y = Some(y);
                }
            }
            layout.push(y);
            y += h;
        }
        let total = y;
        if let Some(sy) = selected_y {
            // Keep the selection comfortably inside the window, with a heading's worth of context above it.
            let want_top = (sy - HEAD_H - 4.0).max(0.0);
            let want_bottom = sy + ITEM_H + 6.0 - list.height();
            let mut target = self.scroll;
            if target > want_top {
                target = want_top;
            }
            if target < want_bottom {
                target = want_bottom;
            }
            let target = target.clamp(0.0, (total - list.height()).max(0.0));
            if self.hl_y < 0.0 {
                self.hl_y = sy;
                self.scroll = target;
            }
            fx.anim |= approach(&mut self.scroll, target, dt, 14.0);
            fx.anim |= approach(&mut self.hl_y, sy, dt, 18.0);
            let hl = Rect::from_min_size(pos2(left, list.top() + self.hl_y - self.scroll), vec2(list.width(), ITEM_H));
            clip.rect_filled(hl, 8.0, fade(INK2, alpha));
            clip.rect_filled(Rect::from_min_size(hl.min + vec2(0.0, 5.0), vec2(3.0, ITEM_H - 10.0)), 2.0, fade(col, alpha));
        }
        if rows.is_empty() {
            let message = if !self.search.trim().is_empty() { "一致するプロンプトがありません" } else if self.fav_only { "お気に入りはまだありません（☆で追加）" } else { "このジャンルは未登録です（＋で追加）" };
            clip.text(list.center(), Align2::CENTER_CENTER, message, font(11.5, false), fade(MUTED, alpha));
        }
        let pointer_in_list = io.ptr.is_some_and(|p| list.contains(p));
        for (index, row) in rows.iter().enumerate() {
            let row_top = list.top() + layout[index] - self.scroll;
            if row_top > list.bottom() || row_top + ITEM_H < list.top() {
                continue;
            }
            // Rows arrive one after another, sliding in from the wheel's side.
            let arrive = ease((since - 0.06 - index as f32 * 0.028) / 0.24);
            let a = alpha * arrive;
            let x = left - side * (1.0 - arrive) * 18.0;
            match row {
                Row::Head(name) => {
                    let g = galley(ui, name, font(10.5, true), fade(col, a), 200.0, 1, None, false);
                    let text_w = g.size().x;
                    clip.galley(pos2(x + 2.0, row_top + 8.0), g, col);
                    clip.line_segment([pos2(x + text_w + 10.0, row_top + 15.0), pos2(right - 2.0, row_top + 15.0)], Stroke::new(1.0_f32, fade(col, a * 0.28)));
                }
                Row::Item(k) => {
                    let k = *k;
                    let rect = Rect::from_min_size(pos2(left, row_top), vec2(list.width(), ITEM_H));
                    let star = Rect::from_min_size(pos2(right - 28.0, row_top), vec2(28.0, ITEM_H));
                    let is_selected = Some(k) == current;
                    let (hovered, clicked) = if pointer_in_list || io.click.is_some_and(|p| list.contains(p)) { io.rect(rect) } else { (false, false) };
                    if hovered && !is_selected {
                        clip.rect_filled(rect, 8.0, fade(Color32::from_white_alpha(14), a));
                    }
                    let item = &self.items[k];
                    let g = galley(ui, &item.title, font(12.5, is_selected), fade(if is_selected { TEXT } else { SOFT }, a), list.width() - 96.0, 1, None, false);
                    let text_w = g.size().x;
                    clip.galley(pos2(x + 13.0, row_top + 5.5), g, TEXT);
                    if item.origin != Origin::Builtin {
                        let tag = if item.origin == Origin::Added { "自作" } else { "編集済" };
                        clip.text(pos2(x + 13.0 + text_w + 7.0, row_top + ITEM_H / 2.0), Align2::LEFT_CENTER, tag, font(9.5, false), fade(col, a * 0.9));
                    }
                    let fav = self.is_fav(k);
                    if fav || hovered || is_selected {
                        let over_star = io.ptr.is_some_and(|p| star.contains(p));
                        clip.text(star.center(), Align2::CENTER_CENTER, if fav { "★" } else { "☆" }, font(13.0, false), fade(if fav { GOLD } else if over_star { TEXT } else { MUTED }, a));
                    }
                    if clicked {
                        let on_star = io.click.is_some_and(|p| star.contains(p));
                        fx.actions.push(if on_star { Action::ToggleFav(k) } else { Action::Pick(k) });
                    }
                }
            }
        }

        // preview of the selected prompt
        painter.line_segment([pos2(left, bottom - 116.0), pos2(right, bottom - 116.0)], Stroke::new(1.0_f32, fade(LINE, alpha)));
        if let Some(k) = current {
            let item = &self.items[k];
            let fresh = ease(((now - self.sel_at) as f32) / 0.22);
            if fresh < 1.0 {
                fx.anim = true;
            }
            let g = galley(ui, &item.body, font(11.5, false), fade(SOFT, alpha * fresh), inner.width(), 4, Some(18.0), false);
            painter.galley(pos2(left, bottom - 108.0 + (1.0 - fresh) * 6.0), g, SOFT);
            let mut x = left;
            let button_top = bottom - 26.0;
            let mut pill = |io: &mut Io, label: &str, w: f32, primary: bool| -> bool {
                let rect = Rect::from_min_size(pos2(x, button_top), vec2(w, 26.0));
                x += w + 6.0;
                let (hovered, clicked) = io.rect(rect);
                let fill = if primary { if hovered { mix(col, Color32::WHITE, 0.15) } else { col } } else if hovered { INK2 } else { DEEP };
                painter.rect_filled(rect, 9.0, fade(fill, alpha));
                painter.text(rect.center(), Align2::CENTER_CENTER, label, font(11.5, primary), fade(if primary { DARK } else { SOFT }, alpha));
                clicked
            };
            if pill(io, "コピー", 66.0, true) {
                fx.actions.push(Action::Copy(k));
            }
            if item.body_en.is_some() && pill(io, "EN", 40.0, false) {
                fx.actions.push(Action::CopyEn(k));
            }
            if pill(io, "編集", 48.0, false) {
                fx.actions.push(Action::StartEdit(Some(k)));
            }
            let room = right - x - 4.0;
            if room > 40.0 && !item.source.is_empty() {
                let g = galley(ui, &item.source, font(9.5, false), fade(MUTED, alpha), room, 1, None, false);
                let w = g.size().x;
                painter.galley(pos2(right - w, button_top + 7.0), g, MUTED);
            }
        }
    }

    fn child(ui: &mut egui::Ui, inner: Rect, card: Rect, alpha: f32) -> egui::Ui {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(card);
        child.multiply_opacity(alpha);
        child
    }

    fn draw_editor(&mut self, ui: &mut egui::Ui, fx: &mut Fx, inner: Rect, card: Rect, alpha: f32) {
        let mut ui = Self::child(ui, inner, card, alpha);
        let existing = self.edit.target.as_ref().and_then(|id| self.items.iter().find(|i| i.id == *id)).map(|i| i.origin);
        let genre = self.cats.iter().find(|c| c.id == self.edit.category).map_or("", |c| c.label.as_str());
        ui.label(egui::RichText::new(if existing.is_some() { "プロンプトを編集" } else { "新しいプロンプト" }).font(display(15.0)).color(TEXT));
        ui.label(egui::RichText::new(format!("ジャンル：{genre}")).font(font(10.5, false)).color(MUTED));
        ui.add_space(2.0);
        ui.add(egui::TextEdit::singleline(&mut self.edit.title).hint_text("タイトル").desired_width(f32::INFINITY));
        ui.add(egui::TextEdit::singleline(&mut self.edit.sub).hint_text("小ジャンル（一覧の見出しになります）").desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().max_height(inner.height() - 168.0).auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut self.edit.body).hint_text("プロンプト本文").desired_width(f32::INFINITY).desired_rows(11));
        });
        ui.add_space(2.0);
        if !self.edit.message.is_empty() {
            ui.label(egui::RichText::new(&self.edit.message).font(font(10.5, false)).color(BAD));
        }
        ui.horizontal(|ui| {
            if ui.button("保存").clicked() {
                fx.actions.push(Action::SaveEdit);
            }
            if ui.button("やめる").clicked() {
                fx.actions.push(Action::ToggleList);
            }
            match existing {
                Some(Origin::Edited) => {
                    if ui.button("元に戻す").clicked() {
                        fx.actions.push(Action::RevertEdit);
                    }
                }
                Some(Origin::Added) => {
                    let label = if self.edit.confirm_delete { "本当に削除" } else { "削除" };
                    if ui.button(egui::RichText::new(label).color(BAD)).clicked() {
                        if self.edit.confirm_delete {
                            fx.actions.push(Action::DeleteEdit);
                        } else {
                            self.edit.confirm_delete = true;
                        }
                    }
                }
                _ => {}
            }
        });
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui, painter: &egui::Painter, fx: &mut Fx, inner: Rect, alpha: f32, col: Color32) {
        let (left, right) = (inner.left(), inner.right());
        let width = right - left;
        let mut y = inner.top();
        painter.text(pos2(left, y + 11.0), Align2::LEFT_CENTER, "設定", display(16.0), fade(TEXT, alpha));
        y += 38.0;

        let mut opacity = (self.saved.opacity * 100.0).round();
        if slider_row(ui, painter, "opacity", pos2(left, y), width, "待機中の濃さ", &mut opacity, (30.0, 100.0, 5.0), col, alpha).0 {
            self.saved.opacity = opacity / 100.0;
            self.dirty = Some(self.last_frame);
        }
        y += 48.0;

        // The window is resized only when the handle is let go, so it never moves under the pointer mid-drag.
        let mut scale = self.scale_draft.unwrap_or((self.saved.scale * 100.0).round());
        let (changed, released) = slider_row(ui, painter, "scale", pos2(left, y), width, "大きさ", &mut scale, (70.0, 160.0, 10.0), col, alpha);
        if changed {
            self.scale_draft = Some(scale);
        }
        if released {
            if let Some(value) = self.scale_draft.take() {
                fx.actions.push(Action::Scale(value / 100.0));
            }
        }
        y += 50.0;

        if toggle_row(ui, painter, "pin", (left, right, y), "常に最前面に表示", self.saved.pinned, col, alpha) {
            fx.actions.push(Action::Pin(!self.saved.pinned));
        }
        y += 32.0;
        if toggle_row(ui, painter, "play", (left, right, y), "遊びの演出（火花・落下・爆散）", self.saved.play, col, alpha) {
            self.saved.play = !self.saved.play;
            self.dirty = Some(self.last_frame);
        }
        y += 34.0;

        painter.line_segment([pos2(left, y), pos2(right, y)], Stroke::new(1.0_f32, fade(LINE, alpha)));
        y += 12.0;
        for (key, what) in [("外輪でホイール", "ジャンルを回す"), ("中央でホイール", "プロンプトを送る"), ("1.5秒とめる", "一覧が開く"), ("ドラッグ", "移動"), ("Esc", "畳む")] {
            painter.text(pos2(left, y + 8.0), Align2::LEFT_CENTER, key, font(10.5, false), fade(MUTED, alpha));
            painter.text(pos2(left + 112.0, y + 8.0), Align2::LEFT_CENTER, what, font(10.5, false), fade(SOFT, alpha));
            y += 18.0;
        }
        y += 12.0;
        if pill_button(ui, painter, "close", Rect::from_min_size(pos2(left, y), vec2(66.0, 26.0)), "閉じる", SOFT, alpha) {
            fx.actions.push(Action::ClosePanel);
        }
        if pill_button(ui, painter, "quit", Rect::from_min_size(pos2(right - 104.0, y), vec2(104.0, 26.0)), "アプリを終了", BAD, alpha) {
            fx.actions.push(Action::Quit);
        }
    }
}

/// Label, value and a slim track with a round handle. Returns (value changed, handle released).
#[allow(clippy::too_many_arguments)]
fn slider_row(ui: &mut egui::Ui, painter: &egui::Painter, id: &str, at: Pos2, width: f32, label: &str, value: &mut f32, range: (f32, f32, f32), col: Color32, alpha: f32) -> (bool, bool) {
    let (min, max, step) = range;
    let track = Rect::from_min_size(pos2(at.x, at.y + 22.0), vec2(width, 16.0));
    let response = ui.interact(track.expand2(vec2(8.0, 4.0)), ui.id().with(id), Sense::click_and_drag());
    let mut changed = false;
    if response.dragged() || response.clicked() || response.drag_started() {
        if let Some(p) = response.interact_pointer_pos() {
            let t = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let next = ((min + t * (max - min)) / step).round() * step;
            if (next - *value).abs() > 0.01 {
                *value = next;
                changed = true;
            }
        }
    }
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let (x, y) = (track.left() + t * track.width(), track.center().y);
    painter.text(at + vec2(0.0, 8.0), Align2::LEFT_CENTER, label, font(12.0, false), fade(SOFT, alpha));
    painter.text(pos2(at.x + width, at.y + 8.0), Align2::RIGHT_CENTER, format!("{:.0}%", *value), num(14.0), fade(col, alpha));
    painter.line_segment([pos2(track.left(), y), pos2(track.right(), y)], Stroke::new(3.0_f32, fade(LINE, alpha)));
    painter.line_segment([pos2(track.left(), y), pos2(x, y)], Stroke::new(3.0_f32, fade(col, alpha)));
    let grabbed = response.hovered() || response.dragged();
    painter.circle_filled(pos2(x, y), if grabbed { 8.0 } else { 6.5 }, fade(TEXT, alpha));
    painter.circle_stroke(pos2(x, y), if grabbed { 8.0 } else { 6.5 }, Stroke::new(2.0_f32, fade(INK, alpha)));
    if grabbed {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (changed, response.drag_stopped() || response.clicked())
}

/// A label with a switch at the right edge. Returns true when it was clicked.
#[allow(clippy::too_many_arguments)]
fn toggle_row(ui: &mut egui::Ui, painter: &egui::Painter, id: &str, span: (f32, f32, f32), label: &str, on: bool, col: Color32, alpha: f32) -> bool {
    let (left, right, y) = span;
    painter.text(pos2(left, y + 10.0), Align2::LEFT_CENTER, label, font(12.0, false), fade(SOFT, alpha));
    let switch = Rect::from_min_size(pos2(right - 36.0, y), vec2(36.0, 20.0));
    let response = ui.interact(switch.expand(4.0), ui.id().with(id), Sense::click());
    painter.rect_filled(switch, 10.0, fade(if on { col } else { LINE }, alpha));
    painter.circle_filled(pos2(if on { switch.right() - 10.0 } else { switch.left() + 10.0 }, switch.center().y), 7.0, fade(if on { DARK } else { SOFT }, alpha));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

/// Outlined pill; `ink` colours both the label and, on hover, the outline.
fn pill_button(ui: &mut egui::Ui, painter: &egui::Painter, id: &str, rect: Rect, label: &str, ink: Color32, alpha: f32) -> bool {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    let hovered = response.hovered();
    painter.rect_filled(rect, 13.0, fade(if hovered { ink } else { LINE }, alpha));
    painter.rect_filled(rect.shrink(1.0), 12.0, fade(if hovered { INK2 } else { INK }, alpha));
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font(11.5, false), fade(ink, alpha));
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

impl App {
    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        // Own clock: animation speed must not depend on how the UI library estimates frame time.
        let now = self.started.elapsed().as_secs_f64();
        let dt = ((now - self.last_frame) as f32).clamp(0.001, 0.05);
        self.last_frame = now;
        if !self.placed {
            self.placed = true;
            self.restore_window(&ctx);
        }
        match self.request.swap(0, Ordering::Relaxed) {
            REQ_SHOW => {
                self.center_window(&ctx);
                self.set_mini(false, &ctx, now);
                self.window.focus_window();
            }
            REQ_QUIT => self.quit = true,
            _ => {}
        }
        let hot = ui.input(|i| i.pointer.has_pointer());
        let mut fx = Fx { anim: false, actions: Vec::new() };
        self.draw(ui, now, dt, hot, &mut fx);
        let had_actions = !fx.actions.is_empty();
        for action in std::mem::take(&mut fx.actions) {
            self.apply(action, &ctx, now);
        }
        self.track_position(now);
        if fx.anim || had_actions {
            ctx.request_repaint();
        }
        if let Some(since) = self.dirty {
            if now - since > 0.6 {
                self.saved.store();
                self.dirty = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
    }
}

// ---------- setup ----------

/// Memory-map a file for the life of the process. Font files are 10 MB+ each; mapped, only the
/// pages actually read are resident and they are shared with the system's own copy.
fn map_file(path: &str) -> Option<&'static [u8]> {
    let file = std::fs::File::open(path).ok()?;
    // Safety: system font files are not modified while the app runs.
    let map = unsafe { memmap2::Mmap::map(&file).ok()? };
    Some(&**Box::leak(Box::new(map)))
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut load = |name: &str, candidates: &[(&str, u32)]| -> bool {
        for (file, index) in candidates {
            if let Some(bytes) = map_file(&format!("C:\\Windows\\Fonts\\{file}")) {
                let mut data = egui::FontData::from_static(bytes);
                data.index = *index;
                fonts.font_data.insert(name.to_owned(), Arc::new(data));
                return true;
            }
        }
        false
    };
    // All of these ship with Windows 10/11. The index picks a face inside a .ttc; each role
    // falls back to Yu Gothic UI / Meiryo when its first choice is missing.
    let loaded = [
        ("jp", load("jp", &[("BIZ-UDGothicR.ttc", 1), ("YuGothM.ttc", 1), ("meiryo.ttc", 0)])),
        ("jp-bold", load("jp-bold", &[("BIZ-UDGothicB.ttc", 1), ("YuGothB.ttc", 1), ("meiryob.ttc", 0)])),
        ("mincho", load("mincho", &[("yumindb.ttf", 0), ("BIZ-UDMinchoM.ttc", 0)])),
        ("din", load("din", &[("bahnschrift.ttf", 0)])),
        ("symbols", load("symbols", &[("seguisym.ttf", 0)])),
    ];
    let has = |name: &str| loaded.iter().any(|(n, ok)| *n == name && *ok);
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let chain = |order: &[&str]| -> Vec<String> {
        let mut list: Vec<String> = order.iter().filter(|name| has(name)).map(|name| (*name).to_owned()).collect();
        list.extend(defaults.iter().cloned());
        list
    };
    fonts.families.insert(FontFamily::Proportional, chain(&["jp", "symbols"]));
    fonts.families.insert(FontFamily::Name("bold".into()), chain(&["jp-bold", "jp", "symbols"]));
    fonts.families.insert(FontFamily::Name("display".into()), chain(&["mincho", "jp-bold", "jp", "symbols"]));
    fonts.families.insert(FontFamily::Name("num".into()), chain(&["din", "jp-bold", "jp", "symbols"]));
    ctx.set_fonts(fonts);
}

fn install_style(ctx: &egui::Context) {
    // Only the editor's text fields and buttons use stock widgets; make them match the rest.
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.extreme_bg_color = DEEP;
    visuals.override_text_color = Some(SOFT);
    visuals.selection.bg_fill = Color32::from_rgb(92, 78, 48);
    visuals.selection.stroke = Stroke::new(1.0_f32, GOLD);
    for (state, fill, line) in [
        (&mut visuals.widgets.inactive, DEEP, LINE),
        (&mut visuals.widgets.hovered, INK2, MUTED),
        (&mut visuals.widgets.active, INK2, TEXT),
    ] {
        state.weak_bg_fill = fill;
        state.bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0_f32, line);
        state.fg_stroke = Stroke::new(1.0_f32, SOFT);
        state.corner_radius = egui::CornerRadius::same(8);
        state.expansion = 0.0;
    }
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.5));
        style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.0));
        style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(10.0));
        style.spacing.item_spacing = vec2(6.0, 7.0);
        style.spacing.button_padding = vec2(12.0, 5.0);
    });
}

fn build_tray(ctx: &egui::Context, request: Arc<AtomicU8>) -> Option<tray_icon::TrayIcon> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // 64x64 RGBA of the app icon, produced next to icon.ico (see assets/).
    let icon = Icon::from_rgba(include_bytes!("../assets/tray-64.rgba").to_vec(), 64, 64).ok()?;

    let menu = Menu::new();
    let show = MenuItem::new("ホイールを画面中央に表示", true, None);
    let quit = MenuItem::new("終了", true, None);
    menu.append_items(&[&show, &quit]).ok()?;
    let (show_id, quit_id) = (show.id().clone(), quit.id().clone());

    // Tray callbacks arrive outside the egui frame: leave a request and wake the UI.
    let (menu_request, menu_ctx) = (request.clone(), ctx.clone());
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id {
            menu_request.store(REQ_QUIT, Ordering::Relaxed);
        } else if event.id == show_id {
            menu_request.store(REQ_SHOW, Ordering::Relaxed);
        }
        menu_ctx.request_repaint();
    }));
    let click_ctx = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            request.store(REQ_SHOW, Ordering::Relaxed);
            click_ctx.request_repaint();
        }
    }));
    TrayIconBuilder::new().with_menu(Box::new(menu)).with_menu_on_left_click(false).with_tooltip(TITLE).with_icon(icon).build().ok()
}
