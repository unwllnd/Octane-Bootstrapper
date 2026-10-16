use std::collections::{HashMap, HashSet};
use std::fs;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::NetworkManagement::IpHelper::{GetExtendedUdpTable, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, UDP_TABLE_OWNER_PID};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows_sys::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Module32FirstW, MODULEENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE};

use crate::fps::OPTIONS;
use crate::launcher;

const PRIMARY: [u8; 17] = [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x10, 0x56, 0xE8, 0, 0, 0, 0, 0x8B, 0xF0, 0x8D, 0x45, 0xF0];
const PRIMARY_MASK: [bool; 17] = [true, true, true, true, true, true, true, true, false, false, false, false, true, true, true, true, true];
const SECONDARY: [u8; 8] = [0xA1, 0, 0, 0, 0, 0x8B, 0x4D, 0xF4];
const SECONDARY_MASK: [bool; 8] = [true, false, false, false, false, true, true, true];
const CALL_DISPLACEMENT: usize = 8;
const CALL_END: u32 = 12;
const FUNCTION_SCAN_LEN: usize = 0x100;
const SEARCH_OFFSET: u32 = 0x100;
const SEARCH_LEN: usize = 0x100;
const SEARCH_STEP: usize = 4;
const DELAY_SIZE: usize = 8;
const MENU_FPS: i64 = 60;
const UNLOCKED_FPS: i64 = 1000;
const UNLOCKED_DELAY: f64 = 1.0 / 10000.0;
const SNAPSHOT_ATTEMPTS: u32 = 5;
const PE_HEADER_POINTER: usize = 0x3C;
const SECTION_HEADER_SIZE: usize = 40;

fn le_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn le_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn frame_delay_bytes(fps: i64) -> [u8; DELAY_SIZE] {
    let delay = if fps >= UNLOCKED_FPS { UNLOCKED_DELAY } else { 1.0 / fps as f64 };
    delay.to_le_bytes()
}

fn unique_offset(data: &[u8], values: &[[u8; DELAY_SIZE]]) -> Option<usize> {
    let mut matches = (0..=data.len() - DELAY_SIZE).step_by(SEARCH_STEP).filter(|&offset| values.iter().any(|value| data[offset..offset + DELAY_SIZE] == *value));
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

fn masked_find(hay: &[u8], pattern: &[u8], mask: &[bool]) -> Option<usize> {
    let last = hay.len().checked_sub(pattern.len())?;
    (0..=last).find(|&start| (0..pattern.len()).all(|index| !mask[index] || hay[start + index] == pattern[index]))
}

fn udp_owners() -> HashSet<u32> {
    let mut owners = HashSet::new();
    collect_udp_owners::<MIB_UDPROW_OWNER_PID>(AF_INET, |row| row.dwOwningPid, &mut owners);
    collect_udp_owners::<MIB_UDP6ROW_OWNER_PID>(AF_INET6, |row| row.dwOwningPid, &mut owners);
    owners
}

fn collect_udp_owners<Row>(family: u16, owner: fn(&Row) -> u32, owners: &mut HashSet<u32>) {
    let mut size = 0u32;
    let mut buffer: Vec<u32> = Vec::new();
    unsafe {
        let mut status = GetExtendedUdpTable(null_mut(), &mut size, 0, family as u32, UDP_TABLE_OWNER_PID, 0);
        while status == ERROR_INSUFFICIENT_BUFFER {
            buffer.resize((size as usize).div_ceil(size_of::<u32>()), 0);
            status = GetExtendedUdpTable(buffer.as_mut_ptr() as *mut _, &mut size, 0, family as u32, UDP_TABLE_OWNER_PID, 0);
        }
        if status != 0 || buffer.is_empty() {
            return;
        }
        let rows = buffer.as_ptr().add(1) as *const Row;
        for index in 0..buffer[0] as usize {
            owners.insert(owner(&*rows.add(index)));
        }
    }
}

fn module_base(pid: u32) -> Option<(u32, String)> {
    unsafe {
        let flags = TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32;
        let snapshot = (0..SNAPSHOT_ATTEMPTS).map(|_| CreateToolhelp32Snapshot(flags, pid)).find(|&snapshot| snapshot != INVALID_HANDLE_VALUE)?;
        let mut entry: MODULEENTRY32W = zeroed();
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        let found = Module32FirstW(snapshot, &mut entry) != 0;
        CloseHandle(snapshot);
        found.then(|| (entry.modBaseAddr as usize as u32, crate::from_wide(&entry.szExePath)))
    }
}

fn primary_rva(exe: &[u8]) -> Option<u32> {
    let pe = le_u32(exe, PE_HEADER_POINTER)? as usize;
    let sections = le_u16(exe, pe + 6)? as usize;
    let optional = le_u16(exe, pe + 20)? as usize;
    let table = pe + 24 + optional;
    for index in 0..sections {
        let header = table + index * SECTION_HEADER_SIZE;
        if !exe.get(header..header + 8)?.starts_with(b".text") {
            continue;
        }
        let virtual_address = le_u32(exe, header + 12)?;
        let raw_size = le_u32(exe, header + 16)? as usize;
        let raw_pointer = le_u32(exe, header + 20)? as usize;
        let section = exe.get(raw_pointer..(raw_pointer + raw_size).min(exe.len()))?;
        let offset = masked_find(section, &PRIMARY, &PRIMARY_MASK)?;
        return Some(virtual_address + offset as u32);
    }
    None
}

pub struct Engine {
    sessions: HashMap<u32, Session>,
    scanned: Option<(String, Option<u32>)>,
}

struct Session {
    handle: HANDLE,
    slot: u32,
    scheduler: u32,
    frame_delay: u32,
}

impl Engine {
    pub fn new() -> Self {
        Engine { sessions: HashMap::new(), scanned: None }
    }

    pub fn apply(&mut self, fps: i64) -> bool {
        let clients = launcher::client_pids().unwrap_or_default();
        let gone: Vec<u32> = self.sessions.keys().copied().filter(|pid| !clients.contains(pid)).collect();
        for pid in gone {
            self.close(pid);
        }
        if clients.is_empty() {
            return false;
        }
        let in_game = udp_owners();
        for pid in clients {
            if !self.sessions.contains_key(&pid) {
                if let Some(session) = self.open(pid) {
                    self.sessions.insert(pid, session);
                }
            }
            let Some(session) = self.sessions.get_mut(&pid) else {
                continue;
            };
            let target = if in_game.contains(&pid) { fps } else { MENU_FPS };
            if !session.apply(target) {
                self.close(pid);
            }
        }
        true
    }

    fn open(&mut self, pid: u32) -> Option<Session> {
        let (base, path) = module_base(pid)?;
        if self.scanned.as_ref().map(|(scanned, _)| scanned) != Some(&path) {
            let rva = fs::read(&path).ok().and_then(|exe| primary_rva(&exe));
            self.scanned = Some((path, rva));
        }
        Session::open(pid, base, self.scanned.as_ref()?.1?)
    }

    fn close(&mut self, pid: u32) {
        if let Some(session) = self.sessions.remove(&pid) {
            unsafe { CloseHandle(session.handle) };
        }
    }
}

impl Session {
    fn open(pid: u32, base: u32, rva: u32) -> Option<Session> {
        let access = PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION | PROCESS_QUERY_LIMITED_INFORMATION;
        let handle = unsafe { OpenProcess(access, 0, pid) };
        if handle == 0 {
            return None;
        }
        let mut session = Session { handle, slot: 0, scheduler: 0, frame_delay: 0 };
        let Some(slot) = session.resolve_slot(base, rva) else {
            unsafe { CloseHandle(handle) };
            return None;
        };
        session.slot = slot;
        Some(session)
    }

    fn read(&self, address: u32, len: usize) -> Option<Vec<u8>> {
        let mut buffer = vec![0u8; len];
        let mut read = 0usize;
        let ok = unsafe { ReadProcessMemory(self.handle, address as usize as *const _, buffer.as_mut_ptr() as *mut _, len, &mut read) };
        (ok != 0 && read == len).then_some(buffer)
    }

    fn read_u32(&self, address: u32) -> Option<u32> {
        le_u32(&self.read(address, size_of::<u32>())?, 0)
    }

    fn write(&self, address: u32, data: &[u8]) -> bool {
        let mut written = 0usize;
        let ok = unsafe { WriteProcessMemory(self.handle, address as usize as *const _, data.as_ptr() as *const _, data.len(), &mut written) };
        ok != 0 && written == data.len()
    }

    fn resolve_slot(&self, base: u32, rva: u32) -> Option<u32> {
        let primary = base.wrapping_add(rva);
        let head = self.read(primary, PRIMARY.len())?;
        if masked_find(&head, &PRIMARY, &PRIMARY_MASK) != Some(0) {
            return None;
        }
        let displacement = le_u32(&head, CALL_DISPLACEMENT)?;
        let function = primary.wrapping_add(CALL_END).wrapping_add(displacement);
        let body = self.read(function, FUNCTION_SCAN_LEN)?;
        let offset = masked_find(&body, &SECONDARY, &SECONDARY_MASK)?;
        le_u32(&body, offset + 1)
    }

    fn find_frame_delay(&self, scheduler: u32) -> Option<u32> {
        let search = scheduler.wrapping_add(SEARCH_OFFSET);
        let data = self.read(search, SEARCH_LEN)?;
        let engine_default = frame_delay_bytes(MENU_FPS);
        let unlocked: Vec<[u8; DELAY_SIZE]> = OPTIONS.iter().map(|(_, value)| frame_delay_bytes(*value)).filter(|delay| *delay != engine_default).collect();
        let offset = unique_offset(&data, &[engine_default]).or_else(|| unique_offset(&data, &unlocked))?;
        Some(search.wrapping_add(offset as u32))
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
            let Some(address) = self.find_frame_delay(scheduler) else {
                return true;
            };
            self.frame_delay = address;
        }
        let desired = frame_delay_bytes(fps);
        match self.read(self.frame_delay, DELAY_SIZE) {
            Some(current) if current != desired => {
                let _ = self.write(self.frame_delay, &desired);
            }
            Some(_) => {}
            None => self.frame_delay = 0,
        }
        true
    }
}
