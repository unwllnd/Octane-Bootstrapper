use std::fs;
use std::mem::size_of;
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetTimer, TrackPopupMenu, MF_BYCOMMAND, MF_SEPARATOR,
    MF_STRING, MSG, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};

const OPTIONS: [(&str, u32); 8] = [
    ("60", 60),
    ("75", 75),
    ("120", 120),
    ("144", 144),
    ("165", 165),
    ("240", 240),
    ("360", 360),
    ("Unlimited", UNLIMITED),
];
const UNLIMITED: u32 = 9999;
const DEFAULT_FPS: u32 = 60;
const EXIT: u32 = 1;
const TRAY_CLICK: u32 = WM_APP + 1;

const SCHEDULER_CALL: &str = "55 8B EC 83 EC 10 56 E8 ?? ?? ?? ?? 8B F0 8D 45 F0";
const SCHEDULER_LOAD: &str = "A1 ?? ?? ?? ?? 8B 4D F4";

struct Client {
    process: HANDLE,
    slot: u32,
    scheduler: u32,
    delay: Option<u32>,
}


pub fn show_tray(exe: &str, pid: u32) -> JoinHandle<Result<()>> {
    let exe = exe.to_string();
    thread::spawn(move || run_tray(&exe, pid))
}


fn run_tray(exe: &str, pid: u32) -> Result<()> {
    let mut client = Client::open(exe, pid)?;
    let window = unsafe { create_window()? };
    let icon = unsafe { add_icon(window)? };
    unsafe { SetTimer(window, 1, 1000, None) };

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
        if message.message == WM_TIMER {
            if client.closed() {
                break;
            }
            client.set_fps(chosen_fps());
        }
        unsafe { DispatchMessageW(&message) };
    }

    client.set_fps(DEFAULT_FPS);
    unsafe { Shell_NotifyIconW(NIM_DELETE, &icon) };
    Ok(())
}


fn chosen_fps() -> u32 {
    match crate::load_state("fps") {
        Some(saved) => saved.parse().unwrap_or(UNLIMITED),
        None => UNLIMITED,
    }
}


unsafe fn create_window() -> Result<HWND> {
    let instance = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(tray_events),
        hInstance: instance.into(),
        lpszClassName: w!("OctaneFpsTray"),
        ..Default::default()
    };
    RegisterClassW(&class);

    let window = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        w!("OctaneFpsTray"),
        w!("Octane FPS"),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        None,
        None,
        instance,
        None,
    );
    Ok(window)
}


unsafe fn add_icon(window: HWND) -> Result<NOTIFYICONDATAW> {
    let mut icon = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: TRAY_CLICK,
        hIcon: LoadIconW(GetModuleHandleW(None)?, PCWSTR(1 as _))?,
        ..Default::default()
    };

    let tip: Vec<u16> = "Octane FPS".encode_utf16().collect();
    icon.szTip[..tip.len()].copy_from_slice(&tip);
    Shell_NotifyIconW(NIM_ADD, &icon).ok()?;
    Ok(icon)
}


unsafe extern "system" fn tray_events(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CLICK if matches!(lparam.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) => {
            show_menu(window).ok();
        }
        WM_DESTROY => PostQuitMessage(0),
        _ => {}
    }
    DefWindowProcW(window, message, wparam, lparam)
}


unsafe fn show_menu(window: HWND) -> Result<()> {
    let menu = CreatePopupMenu()?;
    for (label, fps) in OPTIONS {
        AppendMenuW(menu, MF_STRING, fps as usize, &HSTRING::from(label))?;
    }
    CheckMenuRadioItem(menu, OPTIONS[0].1, UNLIMITED, chosen_fps(), MF_BYCOMMAND.0).ok();
    AppendMenuW(menu, MF_SEPARATOR, 0, None)?;
    AppendMenuW(menu, MF_STRING, EXIT as usize, w!("Exit"))?;

    let mut cursor = POINT::default();
    GetCursorPos(&mut cursor)?;
    SetForegroundWindow(window);
    let flags = TPM_RETURNCMD | TPM_RIGHTBUTTON;
    let picked = TrackPopupMenu(menu, flags, cursor.x, cursor.y, 0, window, None);
    DestroyMenu(menu)?;

    match picked.0 as u32 {
        0 => {}
        EXIT => DestroyWindow(window)?,
        fps => crate::save_state("fps", &fps.to_string())?,
    }
    Ok(())
}


impl Client {
    fn open(exe: &str, pid: u32) -> Result<Client> {
        let slot = scheduler_slot(&fs::read(exe)?).context("the client has no task scheduler")?;
        let memory = PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION;
        let process = unsafe { OpenProcess(memory | PROCESS_SYNCHRONIZE, false, pid)? };
        Ok(Client { process, slot, scheduler: 0, delay: None })
    }


    fn closed(&self) -> bool {
        unsafe { WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 }
    }


    fn set_fps(&mut self, fps: u32) {
        let Some(scheduler) = self.read_u32(self.slot) else { return };
        if scheduler != self.scheduler {
            self.scheduler = scheduler;
            self.delay = None;
        }
        if self.delay.is_none() {
            self.delay = self.find_delay();
        }
        if let Some(delay) = self.delay {
            self.write(delay, &frame_delay(fps));
        }
    }


    fn find_delay(&self) -> Option<u32> {
        let start = self.scheduler + 0x100;
        let bytes = self.read(start, 0x100)?;
        let default = frame_delay(DEFAULT_FPS);
        let found: Vec<usize> = (0..=bytes.len() - 8)
            .step_by(4)
            .filter(|&i| bytes[i..i + 8] == default)
            .collect();

        match found[..] {
            [offset] => Some(start + offset as u32),
            _ => None,
        }
    }


    fn read(&self, address: u32, len: usize) -> Option<Vec<u8>> {
        let mut bytes = vec![0; len];
        let from = address as usize as _;
        let to = bytes.as_mut_ptr() as _;
        unsafe { ReadProcessMemory(self.process, from, to, len, None) }.ok()?;
        Some(bytes)
    }


    fn read_u32(&self, address: u32) -> Option<u32> {
        let bytes = self.read(address, 4)?;
        Some(u32::from_le_bytes(bytes.try_into().unwrap()))
    }


    fn write(&self, address: u32, bytes: &[u8]) {
        let to = address as usize as _;
        let from = bytes.as_ptr() as _;
        unsafe { WriteProcessMemory(self.process, to, from, bytes.len(), None) }.ok();
    }
}


fn frame_delay(fps: u32) -> [u8; 8] {
    (1.0 / fps as f64).to_le_bytes()
}


fn scheduler_slot(exe: &[u8]) -> Option<u32> {
    let call = find(exe, SCHEDULER_CALL)?;
    let jump = i32::from_le_bytes(exe[call + 8..call + 12].try_into().unwrap());
    let function = (call + 12).checked_add_signed(jump as isize)?;
    let body = exe.get(function..function + 0x100)?;
    let load = find(body, SCHEDULER_LOAD)?;
    Some(u32::from_le_bytes(body[load + 1..load + 5].try_into().unwrap()))
}


fn find(bytes: &[u8], pattern: &str) -> Option<usize> {
    let pattern: Vec<Option<u8>> = pattern
        .split(' ')
        .map(|byte| u8::from_str_radix(byte, 16).ok())
        .collect();

    bytes.windows(pattern.len()).position(|window| {
        window.iter().zip(&pattern).all(|(&byte, &wanted)| wanted.is_none() || wanted == Some(byte))
    })
}
