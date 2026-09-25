#![windows_subsystem = "windows"]

mod config;
mod fps;
mod fpsmem;
mod installer;
mod launcher;
mod rpc;
mod ui;

use std::thread;

use anyhow::{anyhow, Result};
use eframe::egui::ViewportBuilder;

use config::Kind;

const WINDOW_SIZE: [f32; 2] = [480.0, 250.0];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == rpc::WATCH_FLAG) {
        return rpc::watch_and_play(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == rpc::STUDIO_WATCH_FLAG) {
        return rpc::watch_studio(&args[1..]);
    }
    if args.first().is_some_and(|arg| arg == fps::FPS_FLAG) {
        return fps::run_window();
    }

    let launch = launcher::Launch::parse(args);
    if launch.kind == Kind::Player && launch.url.is_some() && launcher::client_already_running()? {
        return Ok(());
    }

    let title = launch.kind.label();
    let state = ui::BootstrapState::new();
    let pipeline_state = state.clone();
    thread::spawn(move || {
        if let Err(err) = launcher::run_pipeline(launch, &pipeline_state) {
            pipeline_state.fail(format!("{err:#}"));
        }
    });

    let viewport = ViewportBuilder::default()
        .with_inner_size(WINDOW_SIZE)
        .with_resizable(false)
        .with_decorations(false)
        .with_title(title)
        .with_icon(ui::app_icon());
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        title,
        options,
        Box::new(move |cc| Box::new(ui::BootstrapApp::new(cc, state))),
    )
    .map_err(|err| anyhow!("{err}"))
}
