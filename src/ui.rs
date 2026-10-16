use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui::text::LayoutJob;
use eframe::egui::{self, Align, Align2, Color32, ColorImage, FontId, IconData, Painter, Pos2, Rect, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK};

use crate::config;

const LOGO_PNG: &[u8] = include_bytes!("../logo.png");
const ICON_PNG: &[u8] = include_bytes!("../icon.png");
const INFO_PNG: &[u8] = include_bytes!("../info.png");

const FULL_UV: Rect = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
const REPAINT_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_CEILING: f32 = 0.9;
const IDLE_SECONDS: f32 = 3.5;
const EASING: f32 = 0.16;
const SNAP: f32 = 0.002;

const CUBE: f32 = 30.0;
const TONES: [Color32; 8] = [
    Color32::from_rgb(0x17, 0x10, 0x36),
    Color32::from_rgb(0x1B, 0x13, 0x40),
    Color32::from_rgb(0x1F, 0x16, 0x4B),
    Color32::from_rgb(0x23, 0x19, 0x55),
    Color32::from_rgb(0x27, 0x1D, 0x5F),
    Color32::from_rgb(0x2B, 0x20, 0x69),
    Color32::from_rgb(0x30, 0x25, 0x73),
    Color32::from_rgb(0x35, 0x29, 0x7E),
];
const WAVE_FREQUENCY: [f32; 4] = [0.36, 0.52, 0.27, 0.44];
const WAVE_DRIFT: [f32; 4] = [0.22, -0.16, 0.12, -0.09];
const SHADE: Color32 = Color32::from_rgb(0x0E, 0x09, 0x22);
const VIGNETTE: f32 = 0.55;
const EDGE_WIDTH: f32 = 1.0;
const EDGE_LINE: Color32 = Color32::from_rgba_premultiplied(14, 14, 14, 14);
const INTRO_SECONDS: f32 = 0.35;
const ACCENT: Color32 = Color32::from_rgb(0x7C, 0x4D, 0xFF);
const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF2, 0xEE, 0xFF);
const TEXT_MUTED: Color32 = Color32::from_rgb(0x9C, 0x90, 0xCE);
const ERROR_TEXT: Color32 = Color32::from_rgb(0xFF, 0x8F, 0x8F);

const LOGO_TOP: f32 = 46.0;
const LOGO_HEIGHT: f32 = 70.0;
const LOGO_MARGIN: f32 = 44.0;
const STATUS_Y: f32 = 156.0;
const STATUS_FONT: f32 = 15.0;
const EDGE: f32 = 22.0;
const TRACK_HEIGHT: f32 = 14.0;
const TRACK_MARGIN: f32 = 42.0;
const DETAIL_TOP: f32 = STATUS_Y + 18.0;
const DETAIL_FONT: f32 = 11.0;
const DETAIL_ROWS: usize = 4;
const INFO_SIZE: f32 = 22.0;
const INFO_GAP: f32 = 12.0;
const TIP_FONT: f32 = 11.0;
const TIP_GAP: f32 = 6.0;
const CLOSE_SIZE: f32 = 30.0;
const CLOSE_INSET: f32 = 10.0;
const CLOSE_ARM: f32 = 5.0;
const CLOSE_STROKE: f32 = 1.6;
const CLOSE_STROKE_HOVER: f32 = 2.0;
const HOVER_FADE: f32 = 0.12;

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

pub struct BootstrapApp {
    state: BootstrapState,
    started: Instant,
    logo: TextureHandle,
    info: TextureHandle,
    displayed: f32,
    border_stripped: bool,
}

impl BootstrapApp {
    pub fn new(cc: &eframe::CreationContext<'_>, state: BootstrapState) -> Self {
        Self {
            state,
            started: Instant::now(),
            logo: load_texture(&cc.egui_ctx, "octane_logo", LOGO_PNG),
            info: load_texture(&cc.egui_ctx, "octane_info", INFO_PNG),
            displayed: 0.0,
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

    fn draw_logo(&self, painter: &Painter, card: Rect) {
        let aspect = self.logo.aspect_ratio();
        let width = (LOGO_HEIGHT * aspect).min(card.width() - 2.0 * LOGO_MARGIN);
        let corner = Pos2::new(card.center().x - width / 2.0, card.top() + LOGO_TOP);
        let rect = Rect::from_min_size(corner, Vec2::new(width, width / aspect));
        painter.image(self.logo.id(), rect, FULL_UV, Color32::WHITE);
    }
}

impl eframe::App for BootstrapApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        TONES[0].to_normalized_gamma_f32()
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

        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let card = ui.max_rect();
            let close = close_button_rect(card);
            let (info, track) = bottom_row(card);
            let pointer_inside = ctx.input(|input| input.pointer.has_pointer());
            let close_shown = ctx.animate_bool_with_time(egui::Id::new("close_fade"), pointer_inside, HOVER_FADE);
            draw_grid(ui.painter(), card, elapsed);

            let drag = ui.interact(card, egui::Id::new("winmove"), Sense::click_and_drag());
            let over_close = close_shown > 0.0 && ctx.pointer_interact_pos().is_some_and(|pos| close.contains(pos));
            if drag.drag_started() && !over_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            self.draw_logo(ui.painter(), card);
            match &snapshot.error {
                Some(error) => {
                    draw_status(ui.painter(), card, "Launch failed", ERROR_TEXT);
                    draw_error_detail(ui.painter(), card, error);
                }
                None => {
                    draw_status(ui.painter(), card, &snapshot.status, TEXT_PRIMARY);
                    draw_progress(ui.painter(), track, self.displayed);
                }
            }
            draw_info(ui, &self.info, info);
            draw_close(ui, close, close_shown);
            draw_intro(ui.painter(), card, elapsed);
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

fn tone(level: f32) -> Color32 {
    let position = level.clamp(0.0, 1.0) * (TONES.len() - 1) as f32;
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(TONES.len() - 1);
    mix(TONES[lower], TONES[upper], position - lower as f32)
}

fn draw_grid(painter: &Painter, card: Rect, time: f32) {
    let columns = (card.width() / CUBE).ceil() as i32;
    let rows = (card.height() / CUBE).ceil() as i32;
    let reach = card.size().length() / 2.0;
    let [fx, fy, fsum, fdiff] = WAVE_FREQUENCY;
    let [px, py, psum, pdiff] = WAVE_DRIFT.map(|speed| speed * time);
    for row in 0..rows {
        for column in 0..columns {
            let (x, y) = (column as f32, row as f32);
            let wave = (x * fx + px).sin() + (y * fy + py).sin() + ((x + y) * fsum + psum).sin() + ((x - y) * fdiff + pdiff).sin();
            let cell = Rect::from_min_size(card.min + Vec2::new(x, y) * CUBE, Vec2::splat(CUBE));
            let distance = cell.center().distance(card.center()) / reach;
            let color = mix(tone((wave + 4.0) / 8.0), SHADE, VIGNETTE * distance * distance);
            painter.rect_filled(cell, Rounding::ZERO, color);
        }
    }
    painter.rect_stroke(card.shrink(EDGE_WIDTH / 2.0), Rounding::ZERO, Stroke::new(EDGE_WIDTH, EDGE_LINE));
}

fn draw_intro(painter: &Painter, card: Rect, elapsed: f32) {
    let shown = (elapsed / INTRO_SECONDS).min(1.0);
    if shown == 1.0 {
        return;
    }
    let veil = (1.0 - shown) * (1.0 - shown);
    painter.rect_filled(card, Rounding::ZERO, TONES[0].gamma_multiply(veil));
}

fn draw_status(painter: &Painter, card: Rect, status: &str, color: Color32) {
    let center = Pos2::new(card.center().x, card.top() + STATUS_Y);
    painter.text(center, Align2::CENTER_CENTER, status, FontId::proportional(STATUS_FONT), color);
}

fn draw_error_detail(painter: &Painter, card: Rect, error: &str) {
    let wrap_width = card.width() - 2.0 * TRACK_MARGIN;
    let mut detail = LayoutJob::simple(error.to_string(), FontId::proportional(DETAIL_FONT), TEXT_MUTED, wrap_width);
    detail.wrap.max_rows = DETAIL_ROWS;
    detail.halign = Align::Center;
    let galley = painter.layout_job(detail);
    painter.galley(Pos2::new(card.center().x, card.top() + DETAIL_TOP), galley, TEXT_MUTED);
}

fn bottom_row(card: Rect) -> (Rect, Rect) {
    let row_y = card.bottom() - EDGE - TRACK_HEIGHT / 2.0;
    let info = Rect::from_center_size(Pos2::new(card.left() + EDGE + INFO_SIZE / 2.0, row_y), Vec2::splat(INFO_SIZE));
    let track_inset = EDGE + INFO_SIZE + INFO_GAP;
    let track_min = Pos2::new(card.left() + track_inset, row_y - TRACK_HEIGHT / 2.0);
    let track_max = Pos2::new(card.right() - track_inset, row_y + TRACK_HEIGHT / 2.0);
    (info, Rect::from_min_max(track_min, track_max))
}

fn draw_info(ui: &mut egui::Ui, icon: &TextureHandle, button: Rect) {
    let hovered = ui.interact(button, egui::Id::new("info_btn"), Sense::hover()).hovered();
    let shown = ui.ctx().animate_bool_with_time(egui::Id::new("info_fade"), hovered, HOVER_FADE);
    let color = if hovered { TEXT_PRIMARY } else { TEXT_MUTED };
    let painter = ui.painter();
    painter.image(icon.id(), button, FULL_UV, color);
    if shown == 0.0 {
        return;
    }
    let version = format!("v{}", config::LAUNCHER_VERSION);
    let anchor = button.center_top() - Vec2::new(0.0, TIP_GAP);
    painter.text(anchor, Align2::CENTER_BOTTOM, version, FontId::proportional(TIP_FONT), TEXT_PRIMARY.gamma_multiply(shown));
}

fn draw_progress(painter: &Painter, track: Rect, progress: f32) {
    let rounding = Rounding::same(TRACK_HEIGHT / 2.0);
    painter.rect_filled(track, rounding, Color32::from_black_alpha(80));
    let fill = Vec2::new((track.width() * progress).max(TRACK_HEIGHT), TRACK_HEIGHT);
    painter.rect_filled(Rect::from_min_size(track.min, fill), rounding, ACCENT);
}

fn close_button_rect(card: Rect) -> Rect {
    let corner = Pos2::new(card.right() - CLOSE_SIZE - CLOSE_INSET, card.top() + CLOSE_INSET);
    Rect::from_min_size(corner, Vec2::splat(CLOSE_SIZE))
}

fn draw_close(ui: &mut egui::Ui, button: Rect, shown: f32) {
    if shown == 0.0 {
        return;
    }
    let response = ui.interact(button, egui::Id::new("close_btn"), Sense::click());
    let (color, width) = if response.hovered() {
        (TEXT_PRIMARY, CLOSE_STROKE_HOVER)
    } else {
        (TEXT_MUTED, CLOSE_STROKE)
    };
    let stroke = Stroke::new(width, color.gamma_multiply(shown));
    let center = button.center();
    let falling = Vec2::splat(CLOSE_ARM);
    let rising = Vec2::new(CLOSE_ARM, -CLOSE_ARM);
    ui.painter().line_segment([center - falling, center + falling], stroke);
    ui.painter().line_segment([center - rising, center + rising], stroke);
    if response.clicked() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

pub fn message_box(title: &str, text: &str, failed: bool) {
    let icon = if failed { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe { MessageBoxW(0, crate::wide(text).as_ptr(), crate::wide(title).as_ptr(), MB_OK | icon) };
}
