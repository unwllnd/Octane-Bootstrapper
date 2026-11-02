use std::f32::consts::TAU;
use std::mem::size_of;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use eframe::egui::text::LayoutJob;
use eframe::egui::{
    pos2, vec2, Align, Align2, CentralPanel, Color32, ColorImage, Context, FontData,
    FontDefinitions, FontFamily, FontId, Frame, Galley, IconData, Id, Painter, Pos2, Rect,
    Rounding, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2, ViewportBuilder,
    ViewportCommand, Visuals,
};
use eframe::{App, CreationContext, NativeOptions};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND,
};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

const LOGO: &[u8] = include_bytes!("../logo.png");
const ICON: &[u8] = include_bytes!("../icon.png");
const INFO: &[u8] = include_bytes!("../info.png");
const FONT: &[u8] = include_bytes!("../font.ttf");
const FONT_MEDIUM: &[u8] = include_bytes!("../font-medium.ttf");

const SIZE: Vec2 = vec2(480.0, 250.0);
const WHOLE_IMAGE: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

const SURFACE: Color32 = Color32::from_rgb(0x1B, 0x1A, 0x21);
const TEXT: Color32 = Color32::from_rgb(0xEC, 0xEA, 0xF2);
const MUTED: Color32 = Color32::from_rgb(0x9F, 0x9B, 0xAE);
const ERROR: Color32 = Color32::from_rgb(0xF2, 0xB8, 0xB5);
const CHIP: Color32 = Color32::from_rgb(0x27, 0x26, 0x2F);
const CHIP_HOVER: Color32 = Color32::from_rgb(0x35, 0x33, 0x40);
const CHIP_TEXT: Color32 = Color32::from_rgb(0xD9, 0xD6, 0xE3);
const ACCENT: Color32 = Color32::from_rgb(0xA6, 0x93, 0xFF);
const SPARK: Color32 = Color32::from_rgb(0xD4, 0xCA, 0xFF);
const TRACK: Color32 = Color32::from_rgb(0x30, 0x2E, 0x3B);

const BLOB_TONES: [Color32; 2] = [
    Color32::from_rgb(0x30, 0x27, 0x66),
    Color32::from_rgb(0x4C, 0x3D, 0x98),
];
const ERROR_TONES: [Color32; 2] = [
    Color32::from_rgb(0x4A, 0x22, 0x2E),
    Color32::from_rgb(0x6E, 0x30, 0x3E),
];
const GEAR_TONES: [Color32; 2] = [
    Color32::from_rgb(0x28, 0x26, 0x31),
    Color32::from_rgb(0x36, 0x34, 0x41),
];

const BLOB_CENTER: Pos2 = pos2(468.0, 2.0);
const BLOB_RADIUS: f32 = 74.0;
const BLOB_SPIN: f32 = 0.05;
const BLOB_SHAPES: [[f32; 3]; 4] = [
    [4.0, 0.2, 1.0],
    [9.0, 0.08, 0.5],
    [3.0, 0.16, 1.0],
    [6.0, 0.12, 1.4],
];
const SHAPE_HOLD: f32 = 3.0;
const SHAPE_CHANGE: f32 = 1.6;

const GEAR_CENTER: Pos2 = pos2(-4.0, 150.0);
const GEAR_RADIUS: f32 = 38.0;
const GEAR_SPIN: f32 = 0.18;
const GEAR_SHAPE: [f32; 3] = [8.0, 0.16, 0.45];

const PIXEL: f32 = 10.0;
const PIXEL_GAP: f32 = 2.0;
const PIXEL_ROUND: f32 = 1.5;
const PIXEL_BAND: f32 = 0.75;
const PIXEL_BREATH: f32 = 0.35;
const PIXEL_FLICKER: f32 = 1.2;
const PIXEL_DETAIL: f32 = 0.6;
const WAVE_FREQUENCY: [f32; 4] = [0.36, 0.52, 0.27, 0.44];
const WAVE_DRIFT: [f32; 4] = [0.22, -0.16, 0.12, -0.09];

const LOGO_HEIGHT: f32 = 56.0;
const LOGO_TOP: f32 = 62.0;
const STATUS_GAP: f32 = 26.0;
const STATUS_FONT: f32 = 14.5;
const DETAIL_GAP: f32 = 18.0;
const DETAIL_FONT: f32 = 11.0;
const DETAIL_WIDTH: f32 = 360.0;
const DETAIL_ROWS: usize = 4;
const TYPE_RATE: f32 = 48.0;

const EDGE: f32 = 22.0;
const BUTTON: f32 = 28.0;
const INFO_ICON: f32 = 14.0;
const CLOSE_INSET: f32 = 12.0;
const CLOSE_ARM: f32 = 4.0;
const CLOSE_STROKE: f32 = 1.6;
const TIP_HEIGHT: f32 = 24.0;
const TIP_PAD: f32 = 10.0;
const TIP_GAP: f32 = 8.0;
const TIP_RISE: f32 = 4.0;
const TIP_FONT: f32 = 11.0;

const BAR_GAP: f32 = 14.0;
const BAR_HEIGHT: f32 = 6.0;
const BAR_TRACK: f32 = 2.0;
const BAR_PITCH: f32 = 8.0;
const BAR_HEAD: f32 = 48.0;
const BAR_FINISH: f32 = 0.04;

const IDLE_CEILING: f32 = 0.9;
const IDLE_SECONDS: f32 = 3.5;
const EASING: f32 = 0.16;
const SNAP: f32 = 0.002;
const INTRO_SECONDS: f32 = 0.35;
const FAIL_SECONDS: f32 = 0.45;
const HOVER_SECONDS: f32 = 0.12;


#[derive(Clone, Default)]
pub struct Status(Arc<Mutex<Screen>>);

#[derive(Clone, Default)]
struct Screen {
    text: String,
    progress: Option<f32>,
    error: Option<String>,
    done: bool,
}

struct Window {
    status: Status,
    opened: Instant,
    logo: TextureHandle,
    info: TextureHandle,
    progress: f32,
    line: String,
    line_since: f32,
}


pub fn show(title: &str, work: impl FnOnce(&Status) -> Result<()> + Send + 'static) -> Result<()> {
    unsafe { SetCurrentProcessExplicitAppUserModelID(w!("Octane.Launcher"))? };

    let status = Status::default();
    let worker = thread::spawn({
        let status = status.clone();
        move || status.finish(work(&status))
    });

    let viewport = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_icon(icon());
    let options = NativeOptions { viewport, centered: true, ..Default::default() };
    let opened = eframe::run_native(title, options, Box::new(|cc| {
        Ok(Box::new(Window::new(cc, status.clone())))
    }));
    if opened.is_ok() {
        return Ok(());
    }

    worker.join().unwrap();
    if let Some(error) = status.screen().error {
        let text = HSTRING::from(error);
        let caption = HSTRING::from(title);
        unsafe { MessageBoxW(None, &text, &caption, MB_OK | MB_ICONERROR) };
    }
    Ok(())
}


impl Status {
    pub fn text(&self, text: &str) {
        self.0.lock().unwrap().text = text.to_string();
    }


    pub fn progress(&self, progress: f32) {
        self.0.lock().unwrap().progress = Some(progress);
    }


    fn finish(&self, result: Result<()>) {
        let mut screen = self.0.lock().unwrap();
        match result {
            Ok(()) => screen.done = true,
            Err(error) => screen.error = Some(format!("{error:#}")),
        }
    }


    fn screen(&self) -> Screen {
        self.0.lock().unwrap().clone()
    }
}


impl Window {
    fn new(cc: &CreationContext, status: Status) -> Window {
        remove_rounded_frame(cc);
        install_fonts(&cc.egui_ctx);

        Window {
            status,
            opened: Instant::now(),
            logo: texture(&cc.egui_ctx, "logo", LOGO),
            info: texture(&cc.egui_ctx, "info", INFO),
            progress: 0.0,
            line: String::new(),
            line_since: 0.0,
        }
    }


    fn ease_progress(&mut self, target: f32) {
        if target <= self.progress {
            return;
        }

        self.progress += (target - self.progress) * EASING;
        if target - self.progress < SNAP {
            self.progress = target;
        }
    }


    fn type_line(&mut self, line: &str, time: f32) -> String {
        if self.line != line {
            self.line = line.to_string();
            self.line_since = time;
        }

        let typed = ((time - self.line_since) * TYPE_RATE) as usize;
        line.chars().take(typed).collect()
    }


    fn draw(&self, ui: &Ui, screen: &Screen, typed: &str, time: f32) {
        let ctx = ui.ctx();
        let window = ui.max_rect();
        let has_error = screen.error.is_some();
        let failed = ctx.animate_bool_with_time(Id::new("failed"), has_error, FAIL_SECONDS);
        let inside = ctx.input(|input| input.pointer.has_pointer());
        let close_shown = ctx.animate_bool_with_time(Id::new("close_fade"), inside, HOVER_SECONDS);

        let close = Rect::from_min_size(
            pos2(window.right() - CLOSE_INSET - BUTTON, window.top() + CLOSE_INSET),
            Vec2::splat(BUTTON),
        );
        let info = Rect::from_min_size(
            pos2(window.left() + EDGE, window.bottom() - EDGE - BUTTON),
            Vec2::splat(BUTTON),
        );
        let bar = Rect::from_min_max(
            pos2(info.right() + BAR_GAP, info.center().y - BAR_HEIGHT / 2.0),
            pos2(window.right() - EDGE, info.center().y + BAR_HEIGHT / 2.0),
        );

        draw_shapes(ui.painter(), time, failed);
        drag_window(ui, window, close, close_shown);
        self.draw_text(ui, screen, typed, failed);
        draw_bar(ui.painter(), bar, self.progress, time, 1.0 - failed);
        draw_info(ui, &self.info, info);
        draw_close(ui, close, close_shown);
        draw_intro(ui.painter(), window, time);
    }


    fn draw_text(&self, ui: &Ui, screen: &Screen, typed: &str, failed: f32) {
        let painter = ui.painter();
        let window = ui.max_rect();
        let detail = screen.error.as_deref().map(|error| detail_galley(painter, error, failed));
        let lift = detail.as_ref().map_or(0.0, |galley| (DETAIL_GAP + galley.size().y) / 2.0);
        let top = ui.ctx().animate_value_with_time(Id::new("lift"), LOGO_TOP - lift, FAIL_SECONDS);

        let size = vec2(LOGO_HEIGHT * self.logo.aspect_ratio(), LOGO_HEIGHT);
        let logo = Rect::from_min_size(pos2(window.center().x - size.x / 2.0, top), size);
        painter.image(self.logo.id(), logo, WHOLE_IMAGE, Color32::WHITE);

        let status = pos2(window.center().x, logo.bottom() + STATUS_GAP);
        let color = if screen.error.is_some() { ERROR } else { TEXT };
        painter.text(status, Align2::CENTER_CENTER, typed, medium(STATUS_FONT), color);
        if let Some(galley) = detail {
            painter.galley(status + vec2(0.0, DETAIL_GAP), galley, MUTED);
        }
    }
}


impl App for Window {
    fn clear_color(&self, _visuals: &Visuals) -> [f32; 4] {
        SURFACE.to_normalized_gamma_f32()
    }


    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(16));

        let screen = self.status.screen();
        if screen.done {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        let time = self.opened.elapsed().as_secs_f32();
        self.ease_progress(screen.progress.unwrap_or_else(|| idle_progress(time)));

        let line = match screen.error {
            Some(_) => "Launch failed",
            None => &screen.text,
        };
        let typed = self.type_line(line, time);

        let panel = CentralPanel::default().frame(Frame::none());
        panel.show(ctx, |ui| self.draw(ui, &screen, &typed, time));
    }
}


fn draw_shapes(painter: &Painter, time: f32, failed: f32) {
    let cycle = SHAPE_HOLD + SHAPE_CHANGE;
    let index = (time / cycle) as usize;
    let blend = smoothstep((time % cycle - SHAPE_HOLD) / SHAPE_CHANGE);
    let from = BLOB_SHAPES[index % BLOB_SHAPES.len()];
    let to = BLOB_SHAPES[(index + 1) % BLOB_SHAPES.len()];
    let turn = time * BLOB_SPIN;

    let blob = |angle: f32| {
        let start = lobe(angle - turn, from);
        BLOB_RADIUS * (start + (lobe(angle - turn, to) - start) * blend)
    };
    let gear = |angle: f32| GEAR_RADIUS * lobe(angle + time * GEAR_SPIN, GEAR_SHAPE);
    let blob_tones = [0, 1].map(|tone| mix(BLOB_TONES[tone], ERROR_TONES[tone], failed));

    draw_pixels(painter, BLOB_CENTER, blob, BLOB_RADIUS * (1.0 + PIXEL_BAND), time, blob_tones);
    draw_pixels(painter, GEAR_CENTER, gear, GEAR_RADIUS * (1.0 + PIXEL_BAND), time, GEAR_TONES);
}


fn draw_pixels(
    painter: &Painter,
    center: Pos2,
    radius: impl Fn(f32) -> f32,
    reach: f32,
    time: f32,
    tones: [Color32; 2],
) {
    let first = ((center - Vec2::splat(reach)).to_vec2() / PIXEL).floor();
    let last = ((center + Vec2::splat(reach)).to_vec2() / PIXEL).ceil();

    for row in first.y as i32..last.y as i32 {
        for column in first.x as i32..last.x as i32 {
            let cell = pos2(column as f32, row as f32) * PIXEL + Vec2::splat(PIXEL / 2.0);
            let offset = cell - center;
            let depth = offset.length() / radius(offset.y.atan2(offset.x));
            let side = pixel_side(depth, hash(column, row), time);
            if side == 0.0 {
                continue;
            }

            let rounding = Rounding::same(PIXEL_ROUND.min(side / 4.0));
            let color = wave_color(column, row, time, tones);
            painter.rect_filled(Rect::from_center_size(cell, Vec2::splat(side)), rounding, color);
        }
    }
}


fn pixel_side(depth: f32, seed: f32, time: f32) -> f32 {
    let flicker = 0.5 + 0.5 * (time * PIXEL_FLICKER * (0.6 + seed) + seed * TAU).sin();
    let edge = smoothstep(1.0 - (depth - 1.0) / PIXEL_BAND);
    let breath = 1.0 - PIXEL_BREATH * flicker * smoothstep((depth - 0.85) / 0.3);
    let scale = edge * breath;
    ((PIXEL - PIXEL_GAP) * scale / 2.0).round() * 2.0
}


fn wave_color(column: i32, row: i32, time: f32, [dark, light]: [Color32; 2]) -> Color32 {
    let [fx, fy, fsum, fdiff] = WAVE_FREQUENCY.map(|frequency| frequency * PIXEL_DETAIL);
    let [px, py, psum, pdiff] = WAVE_DRIFT.map(|speed| speed * time);
    let x = column as f32;
    let y = row as f32;

    let wave = (x * fx + px).sin()
        + (y * fy + py).sin()
        + ((x + y) * fsum + psum).sin()
        + ((x - y) * fdiff + pdiff).sin();
    mix(dark, light, (wave + 4.0) / 8.0)
}


fn drag_window(ui: &Ui, window: Rect, close: Rect, close_shown: f32) {
    let drag = ui.interact(window, Id::new("drag"), Sense::click_and_drag());
    let pointer = ui.ctx().pointer_interact_pos();
    let over_close = close_shown > 0.0 && pointer.is_some_and(|pos| close.contains(pos));
    if drag.drag_started() && !over_close {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}


fn detail_galley(painter: &Painter, error: &str, failed: f32) -> Arc<Galley> {
    let font = FontId::proportional(DETAIL_FONT);
    let color = MUTED.gamma_multiply(failed);
    let mut job = LayoutJob::simple(error.to_string(), font, color, DETAIL_WIDTH);
    job.wrap.max_rows = DETAIL_ROWS;
    job.halign = Align::Center;
    painter.layout_job(job)
}


fn draw_bar(painter: &Painter, track: Rect, progress: f32, time: f32, fade: f32) {
    if fade == 0.0 {
        return;
    }

    let y = track.center().y;
    let reach = track.left() + track.width() * progress.clamp(0.0, 1.0);
    let line = Rect::from_center_size(pos2(track.center().x, y), vec2(track.width(), BAR_TRACK));
    painter.rect_filled(line, Rounding::same(BAR_TRACK / 2.0), TRACK.gamma_multiply(fade));

    let head = BAR_HEAD * smoothstep((1.0 - progress) / BAR_FINISH);
    let solid = ((reach - head - track.left()) / BAR_PITCH).floor().max(0.0) as i32;
    let cap = BAR_HEIGHT / 2.0;
    if solid > 0 {
        let end = if head == 0.0 {
            reach
        } else {
            track.left() + solid as f32 * BAR_PITCH - (BAR_PITCH - BAR_HEIGHT) / 2.0
        };
        let corner = cap * (1.0 - head / BAR_HEAD);
        let rounding = Rounding { nw: cap, sw: cap, ne: corner, se: corner };
        let fill = Rect::from_min_max(pos2(track.left(), y - cap), pos2(end, y + cap));
        painter.rect_filled(fill, rounding, ACCENT.gamma_multiply(fade));
    }

    let cells = ((reach - track.left()) / BAR_PITCH).ceil() as i32;
    for cell in solid..cells {
        let x = track.left() + (cell as f32 + 0.5) * BAR_PITCH;
        let near = ((reach - x) / head).clamp(0.0, 1.0);
        let seed = hash(cell, 0);
        let flicker = 0.5 + 0.5 * (time * PIXEL_FLICKER * 2.0 * (0.6 + seed) + seed * TAU).sin();
        let grown = BAR_HEIGHT * smoothstep(near);
        let side = (grown * (1.0 - PIXEL_BREATH * flicker * (1.0 - near)) / 2.0).round() * 2.0;
        if side == 0.0 {
            continue;
        }

        let spark = Rect::from_center_size(pos2(x, y), Vec2::splat(side));
        let color = mix(ACCENT, SPARK, seed * (1.0 - near));
        painter.rect_filled(spark, Rounding::same(side / 6.0), color.gamma_multiply(fade));
    }
}


fn draw_info(ui: &Ui, icon: &TextureHandle, button: Rect) {
    let hovered = ui.interact(button, Id::new("info"), Sense::hover()).hovered();
    let shown = ui.ctx().animate_bool_with_time(Id::new("info_fade"), hovered, HOVER_SECONDS);
    let painter = ui.painter();

    let symbol = Rect::from_center_size(button.center(), Vec2::splat(INFO_ICON));
    painter.circle_filled(button.center(), BUTTON / 2.0, mix(CHIP, CHIP_HOVER, shown));
    painter.image(icon.id(), symbol, WHOLE_IMAGE, ACCENT);
    if shown == 0.0 {
        return;
    }

    let text = CHIP_TEXT.gamma_multiply(shown);
    let version = painter.layout_no_wrap(format!("v{}", crate::VERSION), medium(TIP_FONT), text);
    let width = version.size().x + 2.0 * TIP_PAD;
    let left = button.center().x - width / 2.0;
    let top = button.top() - TIP_GAP - TIP_HEIGHT + TIP_RISE * (1.0 - shown);
    let tip = Rect::from_min_size(pos2(left, top), vec2(width, TIP_HEIGHT));
    painter.rect_filled(tip, Rounding::same(TIP_HEIGHT / 2.0), CHIP.gamma_multiply(shown));
    painter.galley(tip.center() - version.size() / 2.0, version, CHIP_TEXT);
}


fn draw_close(ui: &Ui, button: Rect, shown: f32) {
    if shown == 0.0 {
        return;
    }

    let response = ui.interact(button, Id::new("close"), Sense::click());
    let hovered = response.hovered();
    let hover = ui.ctx().animate_bool_with_time(Id::new("close_hover"), hovered, HOVER_SECONDS);
    let painter = ui.painter();
    let center = button.center();
    let falling = vec2(CLOSE_ARM, CLOSE_ARM);
    let rising = vec2(CLOSE_ARM, -CLOSE_ARM);

    painter.circle_filled(center, BUTTON / 2.0, mix(CHIP, CHIP_HOVER, hover).gamma_multiply(shown));
    let stroke = Stroke::new(CLOSE_STROKE, mix(CHIP_TEXT, TEXT, hover).gamma_multiply(shown));
    painter.line_segment([center - falling, center + falling], stroke);
    painter.line_segment([center - rising, center + rising], stroke);

    if response.clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Close);
    }
}


fn draw_intro(painter: &Painter, window: Rect, time: f32) {
    let shown = (time / INTRO_SECONDS).min(1.0);
    if shown == 1.0 {
        return;
    }

    let veil = (1.0 - shown) * (1.0 - shown);
    painter.rect_filled(window, Rounding::ZERO, SURFACE.gamma_multiply(veil));
}


fn idle_progress(time: f32) -> f32 {
    IDLE_CEILING * (1.0 - (-time / IDLE_SECONDS).exp())
}


fn lobe(angle: f32, [lobes, depth, sharpness]: [f32; 3]) -> f32 {
    1.0 - depth + depth * (0.5 + 0.5 * (lobes * angle).cos()).powf(sharpness)
}


fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}


fn mix(from: Color32, to: Color32, amount: f32) -> Color32 {
    let blend = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(blend(from.r(), to.r()), blend(from.g(), to.g()), blend(from.b(), to.b()))
}


fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n ^ (n >> 16)) as f32 / u32::MAX as f32
}


fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}


fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("octane".into(), FontData::from_static(FONT));
    fonts.font_data.insert("octane-medium".into(), FontData::from_static(FONT_MEDIUM));

    let regular = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    regular.insert(0, "octane".into());
    let mut medium = regular.clone();
    medium.insert(0, "octane-medium".into());
    fonts.families.insert(FontFamily::Name("medium".into()), medium);

    ctx.set_fonts(fonts);
}


fn texture(ctx: &Context, name: &str, png: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(png).unwrap().to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixels = ColorImage::from_rgba_unmultiplied(size, &image.into_raw());
    ctx.load_texture(name, pixels, TextureOptions::LINEAR)
}


fn icon() -> IconData {
    let image = image::load_from_memory(ICON).unwrap().to_rgba8();
    IconData { width: image.width(), height: image.height(), rgba: image.into_raw() }
}


fn remove_rounded_frame(cc: &CreationContext) {
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return };
    let window = HWND(handle.hwnd.get());
    let size = size_of::<u32>() as u32;

    unsafe {
        let corners = &DWMWCP_DONOTROUND as *const _ as _;
        let border = &DWMWA_COLOR_NONE as *const _ as _;
        DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, corners, size).ok();
        DwmSetWindowAttribute(window, DWMWA_BORDER_COLOR, border, size).ok();
    }
}
