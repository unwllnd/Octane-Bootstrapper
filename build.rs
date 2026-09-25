use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=icon.png");

    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    let ico = out.join("icon.ico");
    write_ico(Path::new("icon.png"), &ico)?;

    let rc = out.join("icon.rc");
    std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "/")))?;

    let windres = if std::env::var("HOST")?.contains("windows") {
        "windres"
    } else {
        "x86_64-w64-mingw32-windres"
    };
    let obj = out.join("icon_res.o");
    let status = Command::new(windres)
        .arg(&rc)
        .args(["-O", "coff", "-o"])
        .arg(&obj)
        .status()?;
    if !status.success() {
        return Err(format!("{windres} exited with {status}").into());
    }
    println!("cargo:rustc-link-arg={}", obj.display());
    Ok(())
}

fn write_ico(png: &Path, ico: &Path) -> Result<(), Box<dyn Error>> {
    let source = image::open(png)?.to_rgba8();
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in ICON_SIZES {
        let resized = image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3);
        let entry = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
        dir.add_entry(ico::IconDirEntry::encode(&entry)?);
    }
    dir.write(BufWriter::new(File::create(ico)?))?;
    Ok(())
}
