use std::path::{Path, PathBuf};

use anyhow::Result;
use windows::core::{ComInterface, GUID, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetKnownFolderPath, ShellLink, FOLDERID_Desktop, FOLDERID_Programs,
    KF_FLAG_DEFAULT,
};
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;


pub fn register_links(launcher: &Path) -> Result<()> {
    let (classes, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(r"Software\Classes")?;
    let launcher = launcher.display();

    for (scheme, name) in [("octane-player", "Octane"), ("octane-studio", "Octane Studio")] {
        let (link, _) = classes.create_subkey(scheme)?;
        link.set_value("", &format!("URL:{name} Protocol"))?;
        link.set_value("URL Protocol", &"")?;

        let (icon, _) = link.create_subkey("DefaultIcon")?;
        icon.set_value("", &format!("\"{launcher}\",0"))?;

        let (command, _) = link.create_subkey(r"shell\open\command")?;
        command.set_value("", &format!("\"{launcher}\" \"%1\""))?;
    }
    Ok(())
}


pub fn save_shortcut(name: &str, launcher: &Path, args: &str, icon: &Path) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)?;
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(launcher))?;
        link.SetArguments(&HSTRING::from(args))?;
        link.SetWorkingDirectory(&HSTRING::from(crate::root().as_path()))?;
        link.SetIconLocation(&HSTRING::from(icon), 0)?;

        let file: IPersistFile = link.cast()?;
        for folder in [known_folder(&FOLDERID_Programs)?, known_folder(&FOLDERID_Desktop)?] {
            file.Save(&HSTRING::from(folder.join(name).as_path()), true)?;
        }
    }
    Ok(())
}


unsafe fn known_folder(id: &GUID) -> Result<PathBuf> {
    let path = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
    let folder = PathBuf::from(path.to_string()?);
    CoTaskMemFree(Some(path.0 as _));
    Ok(folder)
}
