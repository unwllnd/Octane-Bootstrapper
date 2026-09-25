use std::fs;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Process32FirstW, Process32NextW, MODULEENTRY32W,
    PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};

const CLIENT_EXE: [&str; 2] = ["octaneplayer.exe", "robloxplayerbeta.exe"];
const PRIMARY: [u8; 17] = [
    0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x10, 0x56, 0xE8, 0, 0, 0, 0, 0x8B, 0xF0, 0x8D, 0x45, 0xF0,
];
const PRIMARY_MASK: [bool; 17] = [
    true, true, true, true, true, true, true, true, false, false, false, false, true, true, true,
    true, true,
];
const SECONDARY: [u8; 8] = [0xA1, 0, 0, 0, 0, 0x8B, 0x4D, 0xF4];
const SECONDARY_MASK: [bool; 8] = [true, false, false, false, false, true, true, true];
const SEARCH_OFFSET: u32 = 0x100;
const SEARCH_LEN: usize = 0x100;

fn frame_delay_bytes(fps: i64) -> [u8; 8] {
    let delay = if fps >= 1000 { 1.0 / 10000.0 } else { 1.0 / fps as f64 };
    delay.to_le_bytes()
}

fn masked_find(hay: &[u8], pat: &[u8], mask: &[bool]) -> Option<usize> {
    if hay.len() < pat.len() {
        return None;
    }
    'scan: for i in 0..=hay.len() - pat.len() {
        for j in 0..pat.len() {
            if mask[j] && hay[i + j] != pat[j] {
                continue 'scan;
            }
        }
        return Some(i);
    }
    None
}

fn wide_to_string(value: &[u16]) -> String {
    let end = value.iter().position(|&c| c == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn find_client() -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        let mut found = None;
        while ok != 0 {
            let name = wide_to_string(&entry.szExeFile).to_ascii_lowercase();
            if CLIENT_EXE.contains(&name.as_str()) {
                found = Some(entry.th32ProcessID);
                break;
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        found
    }
}

fn module_base(pid: u32) -> Option<(u32, String)> {
    unsafe {
        let mut snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        let mut attempt = 0;
        while snapshot == INVALID_HANDLE_VALUE && attempt < 4 {
            attempt += 1;
            snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        }
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: MODULEENTRY32W = zeroed();
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let ok = Module32FirstW(snapshot, &mut entry);
        CloseHandle(snapshot);
        if ok == 0 {
            return None;
        }
        Some((
            entry.modBaseAddr as usize as u32,
            wide_to_string(&entry.szExePath),
        ))
    }
}

fn primary_rva(exe: &[u8]) -> Option<u32> {
    let pe = u32::from_le_bytes([
        *exe.get(0x3C)?,
        *exe.get(0x3D)?,
        *exe.get(0x3E)?,
        *exe.get(0x3F)?,
    ]) as usize;
    let sections = u16::from_le_bytes([*exe.get(pe + 6)?, *exe.get(pe + 7)?]) as usize;
    let optional = u16::from_le_bytes([*exe.get(pe + 20)?, *exe.get(pe + 21)?]) as usize;
    let table = pe + 24 + optional;
    for index in 0..sections {
        let header = table + index * 40;
        let name = exe.get(header..header + 8)?;
        if !name.starts_with(b".text") {
            continue;
        }
        let virtual_address =
            u32::from_le_bytes([exe[header + 12], exe[header + 13], exe[header + 14], exe[header + 15]]);
        let raw_size =
            u32::from_le_bytes([exe[header + 16], exe[header + 17], exe[header + 18], exe[header + 19]])
                as usize;
        let raw_pointer =
            u32::from_le_bytes([exe[header + 20], exe[header + 21], exe[header + 22], exe[header + 23]])
                as usize;
        let end = (raw_pointer + raw_size).min(exe.len());
        let section = exe.get(raw_pointer..end)?;
        let offset = masked_find(section, &PRIMARY, &PRIMARY_MASK)?;
        return Some(virtual_address + offset as u32);
    }
    None
}

pub struct Engine {
    session: Option<Session>,
}

struct Session {
    handle: HANDLE,
    pid: u32,
    slot: u32,
    scheduler: u32,
    frame_delay: u32,
}

impl Engine {
    pub fn new() -> Self {
        Engine { session: None }
    }

    pub fn apply(&mut self, fps: i64) -> bool {
        let Some(pid) = find_client() else {
            self.close();
            return false;
        };
        if self.session.as_ref().map(|session| session.pid) != Some(pid) {
            self.close();
            self.session = Session::open(pid);
        }
        if let Some(session) = self.session.as_mut() {
            if !session.apply(fps) {
                self.close();
            }
        }
        true
    }

    fn close(&mut self) {
        if let Some(session) = self.session.take() {
            unsafe {
                CloseHandle(session.handle);
            }
        }
    }
}

impl Session {
    fn open(pid: u32) -> Option<Session> {
        let (base, path) = module_base(pid)?;
        let exe = fs::read(&path).ok()?;
        let rva = primary_rva(&exe)?;
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION,
                0,
                pid,
            )
        };
        if handle == 0 {
            return None;
        }
        let mut session = Session {
            handle,
            pid,
            slot: 0,
            scheduler: 0,
            frame_delay: 0,
        };
        match session.resolve_slot(base, rva) {
            Some(slot) => {
                session.slot = slot;
                Some(session)
            }
            None => {
                unsafe {
                    CloseHandle(handle);
                }
                None
            }
        }
    }

    fn read(&self, address: u32, len: usize) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; len];
        let mut read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                address as usize as *const _,
                buffer.as_mut_ptr() as *mut _,
                len,
                &mut read,
            )
        };
        if ok != 0 && read == len {
            Some(buffer)
        } else {
            None
        }
    }

    fn read_u32(&self, address: u32) -> Option<u32> {
        let bytes = self.read(address, 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn write(&self, address: u32, data: &[u8]) -> bool {
        let mut written = 0usize;
        let ok = unsafe {
            WriteProcessMemory(
                self.handle,
                address as usize as *const _,
                data.as_ptr() as *const _,
                data.len(),
                &mut written,
            )
        };
        ok != 0 && written == data.len()
    }

    fn resolve_slot(&self, base: u32, rva: u32) -> Option<u32> {
        let primary = base.wrapping_add(rva);
        let head = self.read(primary, PRIMARY.len())?;
        if masked_find(&head, &PRIMARY, &PRIMARY_MASK) != Some(0) {
            return None;
        }
        let displacement = i32::from_le_bytes([head[8], head[9], head[10], head[11]]);
        let function = primary
            .wrapping_add(12)
            .wrapping_add(displacement as u32);
        let body = self.read(function, 0x100)?;
        let offset = masked_find(&body, &SECONDARY, &SECONDARY_MASK)?;
        Some(u32::from_le_bytes([
            body[offset + 1],
            body[offset + 2],
            body[offset + 3],
            body[offset + 4],
        ]))
    }

    fn find_frame_delay(&self, scheduler: u32) -> Option<u32> {
        let search = scheduler.wrapping_add(SEARCH_OFFSET);
        let data = self.read(search, SEARCH_LEN)?;
        let target = (1.0f64 / 60.0).to_le_bytes();
        let mut found = None;
        let mut count = 0;
        let mut offset = 0;
        while offset + 8 <= data.len() {
            if data[offset..offset + 8] == target {
                found = Some(search.wrapping_add(offset as u32));
                count += 1;
            }
            offset += 4;
        }
        if count == 1 {
            found
        } else {
            None
        }
    }

    fn apply(&mut self, fps: i64) -> bool {
        let Some(scheduler) = self.read_u32(self.slot) else {
            return false;
        };
        if scheduler == 0 {
            self.frame_delay = 0;
            return true;
        }
        if scheduler != self.scheduler {
            self.scheduler = scheduler;
            self.frame_delay = 0;
        }
        if self.frame_delay == 0 {
            match self.find_frame_delay(scheduler) {
                Some(address) => self.frame_delay = address,
                None => return true,
            }
        }
        let desired = frame_delay_bytes(fps);
        match self.read(self.frame_delay, 8) {
            Some(current) => {
                if current != desired {
                    let _ = self.write(self.frame_delay, &desired);
                }
                true
            }
            None => {
                self.frame_delay = 0;
                true
            }
        }
    }
}
