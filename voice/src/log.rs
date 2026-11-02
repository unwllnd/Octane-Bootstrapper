use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static FILE: Mutex<Option<File>> = Mutex::new(None);


pub fn start() {
    let local = env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA is not set");
    let dir = PathBuf::from(local).join(r"Octane\logs");
    fs::create_dir_all(&dir).ok();
    *FILE.lock().unwrap() = File::create(dir.join("voice.log")).ok();
}


pub fn write(line: &str) {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    if let Some(file) = FILE.lock().unwrap().as_mut() {
        writeln!(file, "{seconds:.3} {line}").ok();
    }
}


#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::log::write(&format!($($arg)*))
    };
}
