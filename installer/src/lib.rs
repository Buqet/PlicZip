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

fn installed_location() -> Option<String> {
    #[cfg(windows)]
    {
        use installrs::RegistryHive::CurrentUser;
        let mut probe = Installer::new(&[], &[], "none");
        return probe
            .registry()
            .get_value(CurrentUser, INSTALL_KEY, "InstallLocation")
            .ok();
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn installed_version() -> Option<String> {
    #[cfg(windows)]
    {
        use installrs::RegistryHive::CurrentUser;
        let mut probe = Installer::new(&[], &[], "none");
        return probe
            .registry()
            .get_value(CurrentUser, INSTALL_KEY, "DisplayVersion")
            .ok();
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn is_russian() -> bool {
    let locale = sys_locale::get_locale()
        .or_else(|| std::env::var("LANG").ok())
        .or_else(|| std::env::var("LC_ALL").ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    locale.starts_with("ru")
}

fn text<'a>(ru: bool, russian: &'a str, english: &'a str) -> &'a str {
    if ru {
        russian
    } else {
        english
    }
}

#[cfg(windows)]
fn close_running_gui() {
    let _ = std::process::Command::new("taskkill")
        .args(["/IM", "PlicZip.exe", "/T", "/F"])
        .status();
}

#[cfg(not(windows))]
fn close_running_gui() {}

#[cfg(windows)]
fn launch_gui(install_dir: &str) {
    let _ = std::process::Command::new(format!(r"{install_dir}\PlicZip.exe")).spawn();
}

#[cfg(not(windows))]
fn launch_gui(_install_dir: &str) {}

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

    let ru = is_russian();
    i.add_component(
        "core",
        text(ru, "PlicZip GUI", "PlicZip GUI"),
        text(
            ru,
            "Основное приложение (обязательно)",
            "Main application (required)",
        ),
        4,
    )
    .required();
    i.add_component(
        "cli",
        "PlicZip CLI",
        text(ru, "Командная строка", "Command-line tools"),
        1,
    );
    i.add_component(
        "explorer",
        text(ru, "Интеграция с Explorer", "Explorer integration"),
        text(
            ru,
            "Команды PlicZip в контекстном меню",
            "PlicZip commands in the context menu",
        ),
        1,
    );
    i.add_component(
        "start-menu",
        text(ru, "Ярлык в меню Пуск", "Start Menu shortcut"),
        text(ru, "Добавить ярлык PlicZip", "Add a PlicZip shortcut"),
        1,
    );
    i.add_component(
        "desktop",
        text(ru, "Ярлык на рабочем столе", "Desktop shortcut"),
        text(
            ru,
            "Добавить ярлык на рабочий стол",
            "Add a desktop shortcut",
        ),
        1,
    );
    i.set_component_selected("desktop", false);

    i.add_option("install-dir", OptionKind::String, "Installation directory");
    i.add_option(
        "launch-app",
        OptionKind::Bool,
        "Launch PlicZip after installation",
    );
    i.add_option("yes", OptionKind::Flag, "Skip confirmation prompts");
    i.process_commandline()?;
    if let Some(previous) = installed_location() {
        i.set_option_if_unset("install-dir", previous);
    } else {
        i.set_option_if_unset("install-dir", default_install_dir());
    }
    i.set_option_if_unset("launch-app", true);

    if i.is_headless() {
        install_files(i)
    } else {
        let installed = installed_version();
        let welcome_message = match installed {
            Some(version) => format!(
                "{}\n\n{}: {version}",
                text(
                    ru,
                    "Установщик обновит PlicZip, сохранив ваши настройки.",
                    "Setup will update PlicZip while keeping your settings."
                ),
                text(ru, "Текущая версия", "Current version")
            ),
            None => text(
                ru,
                "Установите PlicZip для текущего пользователя без прав администратора.",
                "Install PlicZip for the current user without administrator privileges.",
            )
            .to_string(),
        };

        let mut wizard = InstallerGui::new(text(ru, "Установка PlicZip", "PlicZip Setup"));
        wizard.buttons(ButtonLabels {
            back: text(ru, "Назад", "Back").into(),
            next: text(ru, "Далее", "Next").into(),
            install: text(ru, "Установить", "Install").into(),
            uninstall: text(ru, "Удалить", "Uninstall").into(),
            finish: text(ru, "Готово", "Finish").into(),
            cancel: text(ru, "Отмена", "Cancel").into(),
        });
        wizard.welcome(
            text(ru, "Добро пожаловать в PlicZip", "Welcome to PlicZip"),
            welcome_message,
        );
        wizard.components_page(
            text(ru, "Компоненты", "Components"),
            text(
                ru,
                "Выберите компоненты для установки:",
                "Choose the components to install:",
            ),
        );
        wizard.custom_page(
            text(ru, "Папка установки", "Installation folder"),
            text(
                ru,
                "Выберите папку для PlicZip:",
                "Choose where PlicZip will be installed:",
            ),
            |p| {
                p.dir_picker(
                    "install-dir",
                    text(ru, "Папка установки:", "Installation directory:"),
                );
            },
        );
        wizard.install_page(install_files).hide_log();
        wizard
            .finish_page(
                text(ru, "Установка завершена", "Installation complete"),
                text(ru, "PlicZip готов к работе.", "PlicZip is ready to use."),
            )
            .with_widgets(|p| {
                p.checkbox(
                    "launch-app",
                    text(ru, "Запустить PlicZip", "Launch PlicZip"),
                );
            });
        wizard.error_page(
            text(ru, "Ошибка установки", "Installation failed"),
            text(
                ru,
                "Не удалось установить PlicZip.",
                "PlicZip could not be installed.",
            ),
        );
        let result = wizard.run(i);
        if result.is_ok() && i.option::<bool>("launch-app").unwrap_or(false) {
            let dir: String = i.option("install-dir").unwrap_or_default();
            launch_gui(&dir);
        }
        result
    }
}

fn install_files(i: &mut Installer) -> Result<()> {
    let install_dir: String = i
        .option("install-dir")
        .context("installation directory is missing")?;
    i.set_out_dir(&install_dir);
    close_running_gui();

    i.file(source!("assets/PlicZip.exe"), "PlicZip.exe")
        .status("Installing PlicZip GUI")
        .install()?;
    if i.is_component_selected("cli") {
        i.file(source!("assets/pliczip-cli.exe"), "pliczip-cli.exe")
            .status("Installing PlicZip CLI")
            .install()?;
    }
    i.file(source!("assets/LICENSE"), "LICENSE").install()?;
    i.uninstaller("uninstall.exe").install()?;

    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| install_dir.clone());
        let target = format!(r"{install_dir}\PlicZip.exe");
        if i.is_component_selected("start-menu") {
            let start_menu =
                format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs\PlicZip.lnk");
            i.shortcut(&start_menu, &target)
                .description("PlicZip")
                .working_dir(&install_dir)
                .install()?;
        }
        if i.is_component_selected("desktop") {
            let desktop = std::env::var("USERPROFILE").unwrap_or_else(|_| appdata.clone());
            let desktop_link = format!(r"{desktop}\Desktop\PlicZip.lnk");
            i.shortcut(&desktop_link, &target)
                .description("PlicZip")
                .working_dir(&install_dir)
                .install()?;
        }

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
        if i.is_component_selected("explorer") {
            register_shell(i, &install_dir)?;
        }
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
        let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
        i.remove(format!(
            r"{appdata}\Microsoft\Windows\Start Menu\Programs\PlicZip.lnk"
        ))
        .install()?;
        i.remove(format!(r"{userprofile}\Desktop\PlicZip.lnk"))
            .install()?;
    }
    i.remove(install_dir).status("Removing PlicZip").install()?;
    Ok(())
}
