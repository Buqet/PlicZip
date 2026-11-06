//! Проверка новых версий PlicZip через GitHub Releases.

use anyhow::{Context, Result};
use serde::Deserialize;

const RELEASES_URL: &str = "https://api.github.com/repos/Buqet/PlicZip/releases?per_page=20";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    prerelease: bool,
    draft: bool,
}

/// Результат проверки обновлений.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Версия, встроенная в текущий бинарник.
    pub current: String,
    /// Тег последнего опубликованного релиза.
    pub latest: String,
    /// Ссылка на страницу релиза.
    pub url: String,
    /// Название релиза.
    pub name: Option<String>,
    /// Является ли релиз предварительным.
    pub prerelease: bool,
}

/// Запрашивает последний опубликованный релиз с коротким сетевым таймаутом.
///
/// Функция только читает GitHub API: обновление и запуск файлов автоматически
/// не выполняются.
pub fn check_latest() -> Result<ReleaseInfo> {
    let output = std::process::Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--connect-timeout",
            "5",
            "--max-time",
            "8",
            "--header",
            "Accept: application/vnd.github+json",
            "--user-agent",
            "PlicZip-update-check",
            RELEASES_URL,
        ])
        .output()
        .context("не удалось запустить curl для проверки обновлений")?;
    anyhow::ensure!(
        output.status.success(),
        "GitHub не ответил при проверке обновлений: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let releases: Vec<GithubRelease> = serde_json::from_slice(&output.stdout)
        .context("GitHub вернул неожиданный ответ при проверке обновлений")?;
    let release = releases
        .into_iter()
        .find(|release| !release.draft)
        .ok_or_else(|| anyhow::anyhow!("в GitHub пока нет опубликованных релизов"))?;
    Ok(ReleaseInfo {
        current: option_env!("PLICZIP_RELEASE_TAG")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
        latest: release.tag_name,
        url: release.html_url,
        name: release.name,
        prerelease: release.prerelease,
    })
}
