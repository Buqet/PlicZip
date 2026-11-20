//! Проверка и загрузка новых версий PlicZip через GitHub Releases.

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

const RELEASES_URL: &str = "https://api.github.com/repos/Buqet/PlicZip/releases?per_page=20";

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    name: Option<String>,
    prerelease: bool,
    draft: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Информация о файле, который можно скачать для текущей платформы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    /// Имя файла в GitHub Release.
    pub name: String,
    /// URL скачивания файла.
    pub url: String,
    /// Размер файла в байтах.
    pub size: u64,
    /// URL соседнего manifest-файла SHA-256.
    pub sha256_url: String,
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
    /// Подходящий asset и его manifest, если они опубликованы.
    pub asset: Option<ReleaseAsset>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(30))
        .build()
}

#[cfg(windows)]
fn is_windows_asset(name: &str) -> bool {
    name.to_ascii_lowercase().contains("windows") && name.ends_with(".zip")
}

fn select_asset(release: &GithubRelease) -> Option<ReleaseAsset> {
    #[cfg(windows)]
    let preferred = release
        .assets
        .iter()
        .find(|asset| {
            asset.name
                == format!(
                    "PlicZip-Setup-v{}.exe",
                    release.tag_name.trim_start_matches('v')
                )
        })
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|asset| is_windows_asset(&asset.name))
        });
    #[cfg(not(windows))]
    let preferred = release
        .assets
        .iter()
        .find(|asset| !asset.name.ends_with(".sha256"));

    let asset = preferred?;
    let sha_name = format!("{}.sha256", asset.name);
    let sha256_url = release
        .assets
        .iter()
        .find(|candidate| candidate.name == sha_name)
        .map(|candidate| candidate.browser_download_url.clone())?;
    Some(ReleaseAsset {
        name: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256_url,
    })
}

/// Запрашивает последний опубликованный релиз с коротким сетевым таймаутом.
pub fn check_latest() -> Result<ReleaseInfo> {
    let releases: Vec<GithubRelease> = agent()
        .get(RELEASES_URL)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "PlicZip-update-check")
        .call()
        .context("не удалось связаться с GitHub для проверки обновлений")?
        .into_json()
        .context("GitHub вернул неожиданный ответ при проверке обновлений")?;
    let release = releases
        .into_iter()
        .find(|release| !release.draft)
        .ok_or_else(|| anyhow::anyhow!("в GitHub пока нет опубликованных релизов"))?;
    Ok(ReleaseInfo {
        current: option_env!("PLICZIP_RELEASE_TAG")
            .map(str::to_owned)
            .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION"))),
        latest: release.tag_name.clone(),
        url: release.html_url.clone(),
        name: release.name.clone(),
        prerelease: release.prerelease,
        asset: select_asset(&release),
    })
}

fn read_asset(url: &str) -> Result<Vec<u8>> {
    let mut reader = agent()
        .get(url)
        .set("Accept", "application/octet-stream")
        .set("User-Agent", "PlicZip-updater")
        .call()
        .context("не удалось скачать release asset")?
        .into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .context("ошибка чтения release asset")?;
    Ok(bytes)
}

/// Скачивает asset и его manifest, проверяет SHA-256 и атомарно сохраняет файл.
pub fn download_verified(asset: &ReleaseAsset, destination: &Path) -> Result<()> {
    let bytes = read_asset(&asset.url)?;
    let manifest = String::from_utf8(read_asset(&asset.sha256_url)?)
        .context("manifest SHA-256 имеет неверную кодировку")?;
    let expected = manifest
        .split_whitespace()
        .next()
        .filter(|hash| hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| anyhow::anyhow!("manifest SHA-256 имеет неверный формат"))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        anyhow::bail!("проверка SHA-256 не пройдена: ожидался {expected}, получен {actual}");
    }
    let temporary = destination.with_extension("download");
    std::fs::write(&temporary, bytes).context("не удалось сохранить скачанный файл")?;
    std::fs::rename(&temporary, destination).or_else(|_| {
        std::fs::copy(&temporary, destination)?;
        std::fs::remove_file(&temporary)
    })?;
    Ok(())
}
