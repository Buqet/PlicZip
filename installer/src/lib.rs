use anyhow::{Context, Result};
use installrs::{source, Installer, OptionKind};

const INSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\PlicZip";
const CLASSES_ROOT: &str = r"Software\Classes";
const EXTENSIONS: [&str; 5] = [".plz", ".zip", ".7z", ".rar", ".tar"];

fn default_install_dir() -> String {
    std::env::var_os("LOCALAPPDATA")
        .map(|root| {
            std::path::PathBuf::from(root)
                .join("PlicZip")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| r"C:\Users\Public\PlicZip".to_string())
}

#[cfg(windows)]
fn register_shell(i: &mut Installer, install_dir: &str) -> Result<()> {
    use installrs::RegistryHive::CurrentUser;

    let exe = format!(r#""{install_dir}\PlicZip.exe""#);
    let open_command = format!(r#"{exe} --context-open "%1""#);
    let add_command = format!(r#"{exe} --context-add "%1""#);

    for extension in EXTENSIONS {
        let key = format!(r"{CLASSES_ROOT}\SystemFileAssociations\{extension}\shell\PlicZip");
        i.registry()
            .set_value(CurrentUser, &key, "MUIVerb", "Открыть в PlicZip")
            .install()?;
        i.registry()
            .set_value(CurrentUser, &key, "Icon", exe.as_str())
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                &format!(r"{key}\command"),
                "",
                open_command.as_str(),
            )
            .install()?;
    }

    for key in [
        format!(r"{CLASSES_ROOT}\*\shell\PlicZipAdd"),
        format!(r"{CLASSES_ROOT}\Directory\shell\PlicZipAdd"),
    ] {
        i.registry()
            .set_value(CurrentUser, &key, "MUIVerb", "Добавить в архив PlicZip")
            .install()?;
        i.registry()
            .set_value(CurrentUser, &key, "Icon", exe.as_str())
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                &format!(r"{key}\command"),
                "",
                add_command.as_str(),
            )
            .install()?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn register_shell(_i: &mut Installer, _install_dir: &str) -> Result<()> {
    Ok(())
}

#[cfg(windows)]
fn unregister_shell(i: &mut Installer) -> Result<()> {
    use installrs::RegistryHive::CurrentUser;

    for extension in EXTENSIONS {
        let key = format!(r"{CLASSES_ROOT}\SystemFileAssociations\{extension}\shell\PlicZip");
        i.registry()
            .delete_key(CurrentUser, key)
            .recursive()
            .install()?;
    }
    for key in [
        format!(r"{CLASSES_ROOT}\*\shell\PlicZipAdd"),
        format!(r"{CLASSES_ROOT}\Directory\shell\PlicZipAdd"),
    ] {
        i.registry()
            .delete_key(CurrentUser, key)
            .recursive()
            .install()?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn unregister_shell(_i: &mut Installer) -> Result<()> {
    Ok(())
}

pub fn install(i: &mut Installer) -> Result<()> {
    use installrs::gui::*;

    i.add_option("install-dir", OptionKind::String, "Installation directory");
    i.process_commandline()?;
    i.set_option_if_unset("install-dir", default_install_dir());

    if i.is_headless() {
        install_files(i)
    } else {
        let mut wizard = InstallerGui::new("PlicZip Setup");
        wizard.welcome(
            "Install PlicZip",
            "Install PlicZip for the current Windows user.",
        );
        wizard.custom_page(
            "Installation folder",
            "Choose where PlicZip will be installed:",
            |p| {
                p.dir_picker("install-dir", "Installation directory:");
            },
        );
        wizard.install_page(install_files);
        wizard.finish_page("PlicZip installed", "PlicZip is ready to use.");
        wizard.error_page("Installation failed", "PlicZip could not be installed.");
        wizard.run(i)
    }
}

fn install_files(i: &mut Installer) -> Result<()> {
    let install_dir: String = i
        .option("install-dir")
        .context("installation directory is missing")?;
    i.set_out_dir(&install_dir);

    i.file(source!("assets/PlicZip.exe"), "PlicZip.exe")
        .status("Installing PlicZip GUI")
        .install()?;
    i.file(source!("assets/pliczip-cli.exe"), "pliczip-cli.exe")
        .status("Installing PlicZip CLI")
        .install()?;
    i.file(source!("assets/LICENSE"), "LICENSE").install()?;
    i.uninstaller("uninstall.exe").install()?;

    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| install_dir.clone());
        let start_menu = format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs\PlicZip.lnk");
        let target = format!(r"{install_dir}\PlicZip.exe");
        i.shortcut(&start_menu, &target)
            .description("PlicZip")
            .working_dir(&install_dir)
            .install()?;

        use installrs::RegistryHive::CurrentUser;
        i.registry()
            .set_value(CurrentUser, INSTALL_KEY, "DisplayName", "PlicZip")
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                INSTALL_KEY,
                "DisplayVersion",
                env!("CARGO_PKG_VERSION"),
            )
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                INSTALL_KEY,
                "Publisher",
                "PlicZip contributors",
            )
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                INSTALL_KEY,
                "InstallLocation",
                install_dir.as_str(),
            )
            .install()?;
        i.registry()
            .set_value(
                CurrentUser,
                INSTALL_KEY,
                "UninstallString",
                format!(r#""{install_dir}\uninstall.exe""#),
            )
            .install()?;
        register_shell(i, &install_dir)?;
    }
    Ok(())
}

pub fn uninstall(i: &mut Installer) -> Result<()> {
    i.process_commandline()?;
    #[cfg(windows)]
    i.enable_self_delete();

    #[cfg(windows)]
    let install_dir: String = {
        use installrs::RegistryHive::CurrentUser;
        i.registry()
            .get_value(CurrentUser, INSTALL_KEY, "InstallLocation")
            .or_else(|_| {
                std::env::current_exe()?
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .context("cannot determine installation directory")
            })?
    };
    #[cfg(not(windows))]
    let install_dir = std::env::current_exe()?
        .parent()
        .context("cannot determine installation directory")?
        .to_string_lossy()
        .into_owned();

    unregister_shell(i)?;
    #[cfg(windows)]
    {
        use installrs::RegistryHive::CurrentUser;
        i.registry()
            .delete_key(CurrentUser, INSTALL_KEY)
            .recursive()
            .install()?;
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        let start_menu = format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs\PlicZip.lnk");
        i.remove(start_menu).install()?;
    }
    i.remove(install_dir).status("Removing PlicZip").install()?;
    Ok(())
}
