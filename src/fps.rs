use std::cell::RefCell;
use std::fs;
use std::iter::once;
use std::mem::{size_of, zeroed};
use std::path::PathBuf;
use std::process::Command;
use std::ptr::null;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, MF_BYCOMMAND,
    MF_SEPARATOR, MF_STRING, MSG, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

use crate::config;
use crate::fpsmem::Engine;

pub const FPS_FLAG: &str = "--fps";
const UNLIMITED: i64 = 9999;
const OPTIONS: [(&str, i64); 8] = [
    ("60", 60),
    ("75", 75),
    ("120", 120),
    ("144", 144),
    ("165", 165),
    ("240", 240),
    ("360", 360),
    ("Unlimited", UNLIMITED),
];
const WM_TRAYICON: u32 = WM_APP + 1;
const MENU_BASE: u32 = 1000;
const MENU_EXIT: u32 = 2000;

static SEEN_CLIENT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::new());
}

fn state_file() -> Result<PathBuf> {
    Ok(config::install_root()?.join("state").join("fps"))
}

pub fn saved_fps() -> i64 {
    state_file()
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(UNLIMITED)
}

fn save_fps(fps: i64) -> Result<()> {
    let file = state_file()?;
    fs::create_dir_all(file.parent().unwrap())?;
    fs::write(&file, fps.to_string()).with_context(|| format!("writing {}", file.display()))
}

fn apply(fps: i64) -> bool {
    ENGINE.with(|engine| engine.borrow_mut().apply(fps))
}

pub fn spawn_window() -> Result<()> {
    Command::new(std::env::current_exe()?).arg(FPS_FLAG).spawn()?;
    Ok(())
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(once(0)).collect()
}

pub fn run_window() -> Result<()> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = wide("OctaneFpsTray");
        let mut class: WNDCLASSW = zeroed();
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        RegisterClassW(&class);

        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            wide("Octane FPS").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            0,
            0,
            instance,
            null(),
        );

        let mut icon: NOTIFYICONDATAW = zeroed();
        icon.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        icon.hWnd = window;
        icon.uID = 1;
        icon.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        icon.uCallbackMessage = WM_TRAYICON;
        icon.hIcon = LoadIconW(instance, 1 as *const u16);
        let tip = wide("Octane FPS");
        icon.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(NIM_ADD, &icon);

        SetTimer(window, 1, 1000, None);

        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, 0, 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        Shell_NotifyIconW(NIM_DELETE, &icon);
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAYICON => {
            let event = (lparam & 0xffff) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(window);
            }
            0
        }
        WM_COMMAND => {
            handle_command(window, (wparam & 0xffff) as u32);
            0
        }
        WM_TIMER => {
            if apply(saved_fps()) {
                SEEN_CLIENT.store(true, Ordering::Relaxed);
            } else if SEEN_CLIENT.load(Ordering::Relaxed) {
                DestroyWindow(window);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn show_menu(window: HWND) {
    let menu = CreatePopupMenu();
    let current = saved_fps();
    let mut checked = MENU_BASE;
    for (index, (label, value)) in OPTIONS.iter().enumerate() {
        let id = MENU_BASE + index as u32;
        AppendMenuW(menu, MF_STRING, id as usize, wide(label).as_ptr());
        if *value == current {
            checked = id;
        }
    }
    CheckMenuRadioItem(
        menu,
        MENU_BASE,
        MENU_BASE + OPTIONS.len() as u32 - 1,
        checked,
        MF_BYCOMMAND,
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, MENU_EXIT as usize, wide("Exit").as_ptr());

    let mut point: POINT = zeroed();
    GetCursorPos(&mut point);
    SetForegroundWindow(window);
    TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, 0, window, null());
    DestroyMenu(menu);
}

unsafe fn handle_command(window: HWND, id: u32) {
    if id == MENU_EXIT {
        DestroyWindow(window);
        return;
    }
    if id >= MENU_BASE && (id - MENU_BASE) < OPTIONS.len() as u32 {
        let value = OPTIONS[(id - MENU_BASE) as usize].1;
        let _ = save_fps(value);
        apply(value);
    }
}
