#![windows_subsystem = "windows"]

mod config;
mod fps;
mod fpsmem;
mod installer;
mod launcher;
mod rpc;
mod tls;
mod ui;

use std::thread;

use anyhow::Result;
use eframe::egui::ViewportBuilder;
use windows::core::w;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

use config::Kind;

const WINDOW_SIZE: [f32; 2] = [480.0, 250.0];

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

pub fn from_wide(text: &[u16]) -> String {
    let end = text.iter().position(|&unit| unit == 0).unwrap_or(text.len());
    String::from_utf16_lossy(&text[..end])
}

fn main() -> Result<()> {
    unsafe { SetCurrentProcessExplicitAppUserModelID(w!("Octane.Launcher"))? };
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(rpc::WATCH_FLAG) => return rpc::watch_and_play(&args[1..]),
        Some(rpc::STUDIO_WATCH_FLAG) => return rpc::watch_studio(&args[1..]),
        Some(fps::FPS_FLAG) => return fps::run_window(),
        _ => {}
    }

    let launch = launcher::Launch::parse(args);
    if launch.kind == Kind::Player && launch.url.is_some() && !launcher::client_pids()?.is_empty() {
        return Ok(());
    }

    let title = launch.kind.label();
    let state = ui::BootstrapState::new();
    let pipeline_state = state.clone();
    let fallback_state = state.clone();
    let pipeline = thread::spawn(move || {
        if let Err(err) = launcher::run_pipeline(launch, &pipeline_state) {
            pipeline_state.fail(format!("{err:#}"));
        }
    });

    let viewport = ViewportBuilder::default().with_inner_size(WINDOW_SIZE).with_resizable(false).with_decorations(false).with_title(title).with_icon(ui::app_icon());
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    let shown = eframe::run_native(title, options, Box::new(move |cc| Box::new(ui::BootstrapApp::new(cc, state))));
    if shown.is_ok() {
        return Ok(());
    }
    let _ = pipeline.join();
    if let Some((text, failed)) = fallback_state.final_message() {
        ui::message_box(title, &text, failed);
    }
    Ok(())
}
