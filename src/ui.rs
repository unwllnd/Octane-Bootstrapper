use std::f32::consts::TAU;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Align, Align2, Color32, ColorImage, FontData, FontDefinitions, FontFamily, FontId, Galley, IconData, Painter, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

use crate::config;

const LOGO_PNG: &[u8] = include_bytes!("../logo.png");
const ICON_PNG: &[u8] = include_bytes!("../icon.png");
const INFO_PNG: &[u8] = include_bytes!("../info.png");
const FONT_TTF: &[u8] = include_bytes!("../font.ttf");
const FONT_MEDIUM_TTF: &[u8] = include_bytes!("../font-medium.ttf");
const MEDIUM: &str = "medium";

const FULL_UV: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
const REPAINT_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_CEILING: f32 = 0.9;
const IDLE_SECONDS: f32 = 3.5;
const EASING: f32 = 0.16;
const SNAP: f32 = 0.002;

const SURFACE: Color32 = Color32::from_rgb(0x1B, 0x1A, 0x21);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xEC, 0xEA, 0xF2);
const TEXT_MUTED: Color32 = Color32::from_rgb(0x9F, 0x9B, 0xAE);
const ERROR_TEXT: Color32 = Color32::from_rgb(0xF2, 0xB8, 0xB5);
const CHIP_FILL: Color32 = Color32::from_rgb(0x27, 0x26, 0x2F);
const CHIP_HOVER: Color32 = Color32::from_rgb(0x35, 0x33, 0x40);
const CHIP_TEXT: Color32 = Color32::from_rgb(0xD9, 0xD6, 0xE3);
const ACCENT: Color32 = Color32::from_rgb(0xA6, 0x93, 0xFF);
const TRACK: Color32 = Color32::from_rgb(0x30, 0x2E, 0x3B);

const BLOB_CENTER: Pos2 = Pos2::new(468.0, 2.0);
const BLOB_RADIUS: f32 = 74.0;
const GEAR_CENTER: Pos2 = Pos2::new(-4.0, 150.0);
const GEAR_RADIUS: f32 = 38.0;
const GEAR_SHAPE: [f32; 3] = [8.0, 0.16, 0.45];
const GEAR_SPIN: f32 = 0.18;
const MORPHS: [[f32; 3]; 4] = [[4.0, 0.2, 1.0], [9.0, 0.08, 0.5], [3.0, 0.16, 1.0], [6.0, 0.12, 1.4]];
const MORPH_HOLD: f32 = 3.0;
const MORPH_SECONDS: f32 = 1.6;
const BLOB_SPIN: f32 = 0.05;
const INTRO_SECONDS: f32 = 0.35;

const PIXEL: f32 = 10.0;
const PIXEL_GAP: f32 = 2.0;
const PIXEL_ROUND: f32 = 1.5;
const PIXEL_BAND: f32 = 0.75;
const PIXEL_BREATH: f32 = 0.35;
const PIXEL_FLICKER: f32 = 1.2;
const PIXEL_DETAIL: f32 = 0.6;
const BLOB_TONES: [Color32; 2] = [Color32::from_rgb(0x30, 0x27, 0x66), Color32::from_rgb(0x4C, 0x3D, 0x98)];
const ERROR_TONES: [Color32; 2] = [Color32::from_rgb(0x4A, 0x22, 0x2E), Color32::from_rgb(0x6E, 0x30, 0x3E)];
const GEAR_TONES: [Color32; 2] = [Color32::from_rgb(0x28, 0x26, 0x31), Color32::from_rgb(0x36, 0x34, 0x41)];
const WAVE_FREQUENCY: [f32; 4] = [0.36, 0.52, 0.27, 0.44];
const WAVE_DRIFT: [f32; 4] = [0.22, -0.16, 0.12, -0.09];

const LOGO_HEIGHT: f32 = 56.0;
const LOGO_TOP: f32 = 62.0;
const STATUS_GAP: f32 = 26.0;
const STATUS_FONT: f32 = 14.5;
const DETAIL_GAP: f32 = 18.0;
const DETAIL_FONT: f32 = 11.0;
const DETAIL_ROWS: usize = 4;
const DETAIL_WIDTH: f32 = 360.0;
const TYPE_RATE: f32 = 48.0;

const EDGE: f32 = 22.0;
const BUTTON_SIZE: f32 = 28.0;
const INFO_ICON: f32 = 14.0;
const TIP_HEIGHT: f32 = 24.0;
const TIP_PAD: f32 = 10.0;
const TIP_GAP: f32 = 8.0;
const TIP_RISE: f32 = 4.0;
const TIP_FONT: f32 = 11.0;
const BAR_GAP_LEFT: f32 = 14.0;
const BAR_HEIGHT: f32 = 6.0;
const BAR_TRACK: f32 = 2.0;
const BAR_PITCH: f32 = 8.0;
const BAR_HEAD: f32 = 48.0;
const BAR_FINISH: f32 = 0.04;
const BAR_SPARK: Color32 = Color32::from_rgb(0xD4, 0xCA, 0xFF);

const CLOSE_INSET: f32 = 12.0;
const CLOSE_ARM: f32 = 4.0;
const CLOSE_STROKE: f32 = 1.6;
const HOVER_FADE: f32 = 0.12;
const FAIL_SECONDS: f32 = 0.45;

const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_BORDER_COLOR: u32 = 34;
const DWMWCP_DONOTROUND: u32 = 1;
const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;

#[derive(Clone)]
struct Snapshot {
    status: String,
    progress: Option<f32>,
    error: Option<String>,
    close: bool,
}

#[derive(Clone)]
pub struct BootstrapState(Arc<Mutex<Snapshot>>);

impl BootstrapState {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Snapshot {
            status: String::new(),
            progress: None,
            error: None,
            close: false,
        })))
    }

    pub fn status(&self, status: &str) {
        self.lock().status = status.to_string();
    }

    pub fn progress(&self, progress: f32) {
        self.lock().progress = Some(progress);
    }

    pub fn fail(&self, error: String) {
        self.lock().error = Some(error);
    }

    pub fn close(&self) {
        self.lock().close = true;
    }

    fn snapshot(&self) -> Snapshot {
        self.lock().clone()
    }

    pub fn final_message(&self) -> Option<(String, bool)> {
        let snapshot = self.snapshot();
        match snapshot.error {
            Some(error) => Some((error, true)),
            None if !snapshot.close => Some((snapshot.status, false)),
            None => None,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Snapshot> {
        self.0.lock().unwrap()
    }
}

pub fn app_icon() -> IconData {
    let icon = image::load_from_memory(ICON_PNG).unwrap().to_rgba8();
    IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    }
}

fn load_texture(ctx: &egui::Context, name: &str, png: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(png).unwrap().to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixels = ColorImage::from_rgba_unmultiplied(size, &image.into_raw());
    ctx.load_texture(name, pixels, TextureOptions::LINEAR)
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("octane".into(), FontData::from_static(FONT_TTF));
    fonts.font_data.insert("octane_medium".into(), FontData::from_static(FONT_MEDIUM_TTF));
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "octane".into());
    let medium = std::iter::once("octane_medium".to_string()).chain(proportional.iter().cloned()).collect();
    fonts.families.insert(FontFamily::Name(MEDIUM.into()), medium);
    ctx.set_fonts(fonts);
}

fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MEDIUM.into()))
}

pub struct BootstrapApp {
    state: BootstrapState,
    started: Instant,
    logo: TextureHandle,
    info: TextureHandle,
    displayed: f32,
    typed: String,
    typed_at: f32,
    border_stripped: bool,
}

impl BootstrapApp {
    pub fn new(cc: &eframe::CreationContext<'_>, state: BootstrapState) -> Self {
        install_fonts(&cc.egui_ctx);
        Self {
            state,
            started: Instant::now(),
            logo: load_texture(&cc.egui_ctx, "octane_logo", LOGO_PNG),
            info: load_texture(&cc.egui_ctx, "octane_info", INFO_PNG),
            displayed: 0.0,
            typed: String::new(),
            typed_at: 0.0,
            border_stripped: false,
        }
    }

    fn idle_progress(elapsed: f32) -> f32 {
        IDLE_CEILING * (1.0 - (-elapsed / IDLE_SECONDS).exp())
    }

    fn ease_progress(&mut self, target: f32) {
        if target <= self.displayed {
            return;
        }
        self.displayed += (target - self.displayed) * EASING;
        if target - self.displayed < SNAP {
            self.displayed = target;
        }
    }

    fn type_line(&mut self, line: &str, elapsed: f32) -> String {
        if self.typed != line {
            self.typed = line.to_string();
            self.typed_at = elapsed;
        }
        line.chars().take(((elapsed - self.typed_at) * TYPE_RATE) as usize).collect()
    }

    fn draw_logo(&self, painter: &Painter, window: Rect, top: f32) -> f32 {
        let size = Vec2::new(LOGO_HEIGHT * self.logo.aspect_ratio(), LOGO_HEIGHT);
        let rect = Rect::from_min_size(Pos2::new(window.center().x - size.x / 2.0, top), size);
        painter.image(self.logo.id(), rect, FULL_UV, Color32::WHITE);
        rect.bottom()
    }
}

impl eframe::App for BootstrapApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        SURFACE.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(REPAINT_INTERVAL);
        if !self.border_stripped {
            strip_window_border();
            self.border_stripped = true;
        }

        let snapshot = self.state.snapshot();
        if snapshot.close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f32();
        let target = snapshot.progress.unwrap_or_else(|| Self::idle_progress(elapsed));
        self.ease_progress(target);
        let failed = ctx.animate_bool_with_time(egui::Id::new("failed"), snapshot.error.is_some(), FAIL_SECONDS);
        let line = if snapshot.error.is_some() { "Launch failed" } else { snapshot.status.as_str() };
        let typed = self.type_line(line, elapsed);

        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let window = ui.max_rect();
            let close = Rect::from_min_size(Pos2::new(window.right() - CLOSE_INSET - BUTTON_SIZE, window.top() + CLOSE_INSET), Vec2::splat(BUTTON_SIZE));
            let info = Rect::from_min_size(Pos2::new(window.left() + EDGE, window.bottom() - EDGE - BUTTON_SIZE), Vec2::splat(BUTTON_SIZE));
            let pointer_inside = ctx.input(|input| input.pointer.has_pointer());
            let close_shown = ctx.animate_bool_with_time(egui::Id::new("close_fade"), pointer_inside, HOVER_FADE);
            draw_shapes(ui.painter(), elapsed, failed);

            let drag = ui.interact(window, egui::Id::new("winmove"), Sense::click_and_drag());
            let over_close = close_shown > 0.0 && ctx.pointer_interact_pos().is_some_and(|pos| close.contains(pos));
            if drag.drag_started() && !over_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            let detail = snapshot.error.as_deref().map(|error| detail_galley(ui.painter(), error, TEXT_MUTED.gamma_multiply(failed)));
            let lift = detail.as_ref().map_or(0.0, |galley| (DETAIL_GAP + galley.size().y) / 2.0);
            let top = ctx.animate_value_with_time(egui::Id::new("lift"), LOGO_TOP - lift, FAIL_SECONDS);
            let status_y = self.draw_logo(ui.painter(), window, top) + STATUS_GAP;
            let color = if snapshot.error.is_some() { ERROR_TEXT } else { TEXT_PRIMARY };
            draw_status(ui.painter(), window, status_y, &typed, color);
            if let Some(galley) = detail {
                let corner = Pos2::new(window.center().x, status_y + DETAIL_GAP);
                ui.painter().galley(corner, galley, TEXT_MUTED);
            }

            let track = Rect::from_min_max(Pos2::new(info.right() + BAR_GAP_LEFT, info.center().y - BAR_HEIGHT / 2.0), Pos2::new(window.right() - EDGE, info.center().y + BAR_HEIGHT / 2.0));
            draw_bar(ui.painter(), track, self.displayed, elapsed, 1.0 - failed);
            draw_info(ui, &self.info, info);
            draw_close(ui, close, close_shown);
            draw_intro(ui.painter(), window, elapsed);
        });
    }
}

fn strip_window_border() {
    unsafe extern "system" fn strip(hwnd: HWND, _: LPARAM) -> BOOL {
        let corner = DWMWCP_DONOTROUND;
        let border = DWMWA_COLOR_NONE;
        let size = std::mem::size_of::<u32>() as u32;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const u32 as _, size);
        DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const u32 as _, size);
        TRUE
    }

    unsafe {
        EnumThreadWindows(GetCurrentThreadId(), Some(strip), 0);
    }
}

fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(blend(from.r(), to.r()), blend(from.g(), to.g()), blend(from.b(), to.b()))
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lobe(angle: f32, [lobes, depth, sharpness]: [f32; 3]) -> f32 {
    1.0 - depth + depth * (0.5 + 0.5 * (lobes * angle).cos()).powf(sharpness)
}

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}

fn draw_pixels(painter: &Painter, center: Pos2, radius: impl Fn(f32) -> f32, reach: f32, time: f32, [dark, light]: [Color32; 2]) {
    let [fx, fy, fsum, fdiff] = WAVE_FREQUENCY.map(|frequency| frequency * PIXEL_DETAIL);
    let [px, py, psum, pdiff] = WAVE_DRIFT.map(|speed| speed * time);
    let low = ((center - Vec2::splat(reach)).to_vec2() / PIXEL).floor();
    let high = ((center + Vec2::splat(reach)).to_vec2() / PIXEL).ceil();
    for row in low.y as i32..high.y as i32 {
        for column in low.x as i32..high.x as i32 {
            let cell = Rect::from_min_size(Pos2::new(column as f32, row as f32) * PIXEL, Vec2::splat(PIXEL));
            let offset = cell.center() - center;
            let depth = offset.length() / radius(offset.y.atan2(offset.x));
            let seed = hash(column, row);
            let flicker = 0.5 + 0.5 * (time * PIXEL_FLICKER * (0.6 + seed) + seed * TAU).sin();
            let scale = smoothstep(1.0 - (depth - 1.0) / PIXEL_BAND) * (1.0 - PIXEL_BREATH * flicker * smoothstep((depth - 0.85) / 0.3));
            let side = ((PIXEL - PIXEL_GAP) * scale / 2.0).round() * 2.0;
            if side == 0.0 {
                continue;
            }
            let (x, y) = (column as f32, row as f32);
            let wave = (x * fx + px).sin() + (y * fy + py).sin() + ((x + y) * fsum + psum).sin() + ((x - y) * fdiff + pdiff).sin();
            let color = mix(dark, light, (wave + 4.0) / 8.0);
            painter.rect_filled(Rect::from_center_size(cell.center(), Vec2::splat(side)), Rounding::same(PIXEL_ROUND.min(side / 4.0)), color);
        }
    }
}

fn draw_shapes(painter: &Painter, time: f32, failed: f32) {
    let cycle = MORPH_HOLD + MORPH_SECONDS;
    let index = (time / cycle) as usize;
    let blend = smoothstep((time % cycle - MORPH_HOLD) / MORPH_SECONDS);
    let (from, to) = (MORPHS[index % MORPHS.len()], MORPHS[(index + 1) % MORPHS.len()]);
    let turn = time * BLOB_SPIN;
    let blob = |angle: f32| BLOB_RADIUS * (lobe(angle - turn, from) + (lobe(angle - turn, to) - lobe(angle - turn, from)) * blend);
    let gear = |angle: f32| GEAR_RADIUS * lobe(angle + time * GEAR_SPIN, GEAR_SHAPE);
    let blob_tones = [0, 1].map(|tone| mix(BLOB_TONES[tone], ERROR_TONES[tone], failed));
    draw_pixels(painter, BLOB_CENTER, blob, BLOB_RADIUS * (1.0 + PIXEL_BAND), time, blob_tones);
    draw_pixels(painter, GEAR_CENTER, gear, GEAR_RADIUS * (1.0 + PIXEL_BAND), time, GEAR_TONES);
}

fn draw_intro(painter: &Painter, window: Rect, elapsed: f32) {
    let shown = (elapsed / INTRO_SECONDS).min(1.0);
    if shown == 1.0 {
        return;
    }
    let veil = (1.0 - shown) * (1.0 - shown);
    painter.rect_filled(window, Rounding::ZERO, SURFACE.gamma_multiply(veil));
}

fn draw_status(painter: &Painter, window: Rect, y: f32, typed: &str, color: Color32) {
    painter.text(Pos2::new(window.center().x, y), Align2::CENTER_CENTER, typed, medium(STATUS_FONT), color);
}

fn detail_galley(painter: &Painter, error: &str, color: Color32) -> Arc<Galley> {
    let mut detail = LayoutJob::simple(error.to_string(), FontId::proportional(DETAIL_FONT), color, DETAIL_WIDTH);
    detail.wrap.max_rows = DETAIL_ROWS;
    detail.halign = Align::Center;
    painter.layout_job(detail)
}

fn draw_info(ui: &mut egui::Ui, icon: &TextureHandle, button: Rect) {
    let hovered = ui.interact(button, egui::Id::new("info_btn"), Sense::hover()).hovered();
    let shown = ui.ctx().animate_bool_with_time(egui::Id::new("info_fade"), hovered, HOVER_FADE);
    let painter = ui.painter();
    painter.circle_filled(button.center(), BUTTON_SIZE / 2.0, mix(CHIP_FILL, CHIP_HOVER, shown));
    painter.image(icon.id(), Rect::from_center_size(button.center(), Vec2::splat(INFO_ICON)), FULL_UV, ACCENT);
    if shown == 0.0 {
        return;
    }
    let version = painter.layout_no_wrap(format!("v{}", config::LAUNCHER_VERSION), medium(TIP_FONT), CHIP_TEXT.gamma_multiply(shown));
    let corner = Pos2::new(button.left(), button.top() - TIP_GAP - TIP_HEIGHT + TIP_RISE * (1.0 - shown));
    let tip = Rect::from_min_size(corner, Vec2::new(version.size().x + 2.0 * TIP_PAD, TIP_HEIGHT));
    painter.rect_filled(tip, Rounding::same(TIP_HEIGHT / 2.0), CHIP_FILL.gamma_multiply(shown));
    painter.galley(tip.center() - version.size() / 2.0, version, CHIP_TEXT);
}

fn draw_bar(painter: &Painter, track: Rect, progress: f32, time: f32, fade: f32) {
    if fade == 0.0 {
        return;
    }
    let y = track.center().y;
    let reach = track.left() + track.width() * progress.clamp(0.0, 1.0);
    painter.rect_filled(Rect::from_center_size(Pos2::new(track.center().x, y), Vec2::new(track.width(), BAR_TRACK)), Rounding::same(BAR_TRACK / 2.0), TRACK.gamma_multiply(fade));
    let head = BAR_HEAD * smoothstep((1.0 - progress) / BAR_FINISH);
    let solid = ((reach - head - track.left()) / BAR_PITCH).floor().max(0.0) as i32;
    let cap = BAR_HEIGHT / 2.0;
    if solid > 0 {
        let end = if head == 0.0 { reach } else { track.left() + solid as f32 * BAR_PITCH - (BAR_PITCH - BAR_HEIGHT) / 2.0 };
        let corner = cap * (1.0 - head / BAR_HEAD);
        let rounding = Rounding { nw: cap, sw: cap, ne: corner, se: corner };
        painter.rect_filled(Rect::from_min_max(Pos2::new(track.left(), y - cap), Pos2::new(end, y + cap)), rounding, ACCENT.gamma_multiply(fade));
    }
    let cells = ((reach - track.left()) / BAR_PITCH).ceil() as i32;
    for cell in solid..cells {
        let center = track.left() + (cell as f32 + 0.5) * BAR_PITCH;
        let near = ((reach - center) / head).clamp(0.0, 1.0);
        let seed = hash(cell, 0);
        let flicker = 0.5 + 0.5 * (time * PIXEL_FLICKER * 2.0 * (0.6 + seed) + seed * TAU).sin();
        let side = (BAR_HEIGHT * smoothstep(near) * (1.0 - PIXEL_BREATH * flicker * (1.0 - near)) / 2.0).round() * 2.0;
        if side == 0.0 {
            continue;
        }
        let color = mix(ACCENT, BAR_SPARK, seed * (1.0 - near));
        painter.rect_filled(Rect::from_center_size(Pos2::new(center, y), Vec2::splat(side)), Rounding::same(side / 6.0), color.gamma_multiply(fade));
    }
}

fn draw_close(ui: &mut egui::Ui, button: Rect, shown: f32) {
    if shown == 0.0 {
        return;
    }
    let response = ui.interact(button, egui::Id::new("close_btn"), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(egui::Id::new("close_hover"), response.hovered(), HOVER_FADE);
    let painter = ui.painter();
    let center = button.center();
    painter.circle_filled(center, BUTTON_SIZE / 2.0, mix(CHIP_FILL, CHIP_HOVER, hover).gamma_multiply(shown));
    let stroke = Stroke::new(CLOSE_STROKE, mix(CHIP_TEXT, TEXT_PRIMARY, hover).gamma_multiply(shown));
    let falling = Vec2::splat(CLOSE_ARM);
    let rising = Vec2::new(CLOSE_ARM, -CLOSE_ARM);
    painter.line_segment([center - falling, center + falling], stroke);
    painter.line_segment([center - rising, center + rising], stroke);
    if response.clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

pub fn message_box(title: &str, text: &str, failed: bool) {
    let icon = if failed { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe { MessageBoxW(0, crate::wide(text).as_ptr(), crate::wide(title).as_ptr(), MB_OK | icon) };
}
