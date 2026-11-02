use std::env;
use std::error::Error;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
use image::imageops::{self, FilterType};

const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];


fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=icon.png");
    println!("cargo:rerun-if-changed=app.manifest");

    let out = PathBuf::from(env::var("OUT_DIR")?);
    let icon = out.join("icon.ico");
    let script = out.join("app.rc");

    write_icon(Path::new("icon.png"), &icon)?;
    fs::write(&script, resource_script(&icon))?;
    embed_resource::compile(&script, embed_resource::NONE).manifest_required()?;
    Ok(())
}


fn write_icon(png: &Path, icon: &Path) -> Result<(), Box<dyn Error>> {
    let image = image::open(png)?.to_rgba8();
    let mut sizes = IconDir::new(ResourceType::Icon);

    for size in ICON_SIZES {
        let resized = imageops::resize(&image, size, size, FilterType::Lanczos3);
        let entry = IconImage::from_rgba_data(size, size, resized.into_raw());
        sizes.add_entry(IconDirEntry::encode(&entry)?);
    }

    sizes.write(File::create(icon)?)?;
    Ok(())
}


fn resource_script(icon: &Path) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("app.manifest");
    let icon = icon.display().to_string().replace('\\', "/");
    let manifest = manifest.display().to_string().replace('\\', "/");

    let version = env!("CARGO_PKG_VERSION");
    let major = env!("CARGO_PKG_VERSION_MAJOR");
    let minor = env!("CARGO_PKG_VERSION_MINOR");
    let patch = env!("CARGO_PKG_VERSION_PATCH");

    format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Octane"
            VALUE "FileDescription", "Octane Voice"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "OctaneVoice"
            VALUE "LegalCopyright", "Copyright (C) 2026 Octane"
            VALUE "OriginalFilename", "OctaneVoice.exe"
            VALUE "ProductName", "Octane"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}
