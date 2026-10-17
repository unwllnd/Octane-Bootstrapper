use std::error::Error;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=icon.png");
    println!("cargo:rerun-if-changed=app.manifest");

    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    let ico = out.join("icon.ico");
    write_ico(Path::new("icon.png"), &ico)?;

    let manifest = std::fs::canonicalize("app.manifest")?;
    let rc = out.join("app.rc");
    std::fs::write(&rc, resource_script(&ico, &manifest)?)?;
    embed_resource::compile(&rc, embed_resource::NONE).manifest_required()?;
    Ok(())
}

fn rc_path(path: &Path) -> String {
    path.display().to_string().trim_start_matches(r"\\?\").replace('\\', "/")
}

fn resource_script(ico: &Path, manifest: &Path) -> Result<String, Box<dyn Error>> {
    let major = std::env::var("CARGO_PKG_VERSION_MAJOR")?;
    let minor = std::env::var("CARGO_PKG_VERSION_MINOR")?;
    let patch = std::env::var("CARGO_PKG_VERSION_PATCH")?;
    let version = std::env::var("CARGO_PKG_VERSION")?;
    Ok(format!(
        r#"1 ICON "{ico}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Octane"
            VALUE "FileDescription", "Octane Launcher"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "OctanePlayerLauncher"
            VALUE "LegalCopyright", "Copyright (C) 2026 Octane"
            VALUE "OriginalFilename", "OctanePlayerLauncher.exe"
            VALUE "ProductName", "Octane"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        ico = rc_path(ico),
        manifest = rc_path(manifest),
    ))
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
