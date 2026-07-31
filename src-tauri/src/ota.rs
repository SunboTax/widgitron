use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use widgitron_core::{
    validate_github_response_url, validate_installer_asset_name, validate_official_download_url,
};

static UPDATE_CHECK_CACHE: std::sync::OnceLock<Mutex<Option<(Instant, UpdateInfo)>>> =
    std::sync::OnceLock::new();

const UPDATE_CHECK_COOLDOWN: Duration = Duration::from_secs(120);
const RELEASES_API_URL: &str = "https://api.github.com/repos/starkmomo/widgitron/releases/latest";
const MAX_INSTALLER_BYTES: u64 = 1024 * 1024 * 1024;
static UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Serialize, Clone)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_notes: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct ProgressPayload {
    state: String, // "downloading" | "completed" | "error"
    progress: u8,
    error: Option<String>,
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Result<UpdateInfo, String> {
    let cache = UPDATE_CHECK_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| {
            log::warn!("Update cache mutex poisoned, recovering");
            e.into_inner()
        });
        if let Some((checked_at, info)) = guard.as_ref() {
            if checked_at.elapsed() < UPDATE_CHECK_COOLDOWN {
                log::info!(
                    "Returning cached update check ({}s ago)",
                    checked_at.elapsed().as_secs()
                );
                return Ok(info.clone());
            }
        }
    }

    let current_version = app.package_info().version.to_string();
    log::info!("Checking for updates. Current version: {}", current_version);

    let client = reqwest::Client::builder()
        .user_agent("widgitron-updater")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {:?}", e))?;

    let response = client
        .get(RELEASES_API_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch release info: {:?}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "GitHub API returned status code: {}",
            response.status()
        ));
    }

    let release: GithubRelease = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse release JSON: {:?}", e))?;

    let latest_tag = release.tag_name.clone();
    let cleaned_latest = latest_tag.strip_prefix('v').unwrap_or(&latest_tag);
    let cleaned_current = current_version
        .strip_prefix('v')
        .unwrap_or(&current_version);

    let has_update = match (
        semver::Version::parse(cleaned_latest),
        semver::Version::parse(cleaned_current),
    ) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => cleaned_latest != cleaned_current,
    };

    log::info!("Latest version: {}, has_update: {}", latest_tag, has_update);

    let mut download_url = None;
    let mut asset_name = None;

    if has_update {
        // Find Windows asset: prioritize _x64-setup.exe, then -setup.exe, then .exe, then .msi
        let windows_asset = release
            .assets
            .iter()
            .filter(|asset| validate_release_asset(asset).is_ok())
            .find(|a| a.name.ends_with("_x64-setup.exe"))
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .filter(|asset| validate_release_asset(asset).is_ok())
                    .find(|a| a.name.ends_with("-setup.exe"))
            })
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .filter(|asset| validate_release_asset(asset).is_ok())
                    .find(|a| a.name.ends_with(".exe"))
            })
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .filter(|asset| validate_release_asset(asset).is_ok())
                    .find(|a| a.name.ends_with(".msi"))
            });

        if let Some(asset) = windows_asset {
            download_url = Some(asset.browser_download_url.clone());
            asset_name = Some(asset.name.clone());
            log::info!(
                "Found Windows update asset: {} ({})",
                asset.name,
                asset.browser_download_url
            );
        } else {
            log::warn!("No suitable Windows installation asset found in release");
        }
    }

    let result = UpdateInfo {
        has_update,
        current_version,
        latest_version: latest_tag,
        release_notes: release.body,
        download_url,
        asset_name,
    };

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), result.clone()));
    }

    Ok(result)
}

#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<(), String> {
    if UPDATE_IN_PROGRESS
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("An update download is already in progress".to_string());
    }

    let update = match check_for_updates(app.clone()).await {
        Ok(update) => update,
        Err(error) => {
            UPDATE_IN_PROGRESS.store(false, Ordering::Release);
            return Err(error);
        }
    };
    if !update.has_update {
        UPDATE_IN_PROGRESS.store(false, Ordering::Release);
        return Err("No newer release is available".to_string());
    }
    let download_url = update.download_url.ok_or_else(|| {
        UPDATE_IN_PROGRESS.store(false, Ordering::Release);
        "The official release has no supported installer".to_string()
    })?;
    let asset_name = update.asset_name.ok_or_else(|| {
        UPDATE_IN_PROGRESS.store(false, Ordering::Release);
        "The official release has no supported installer name".to_string()
    })?;
    if let Err(error) = validate_official_download_url(&download_url)
        .and_then(|_| validate_installer_asset_name(&asset_name))
    {
        UPDATE_IN_PROGRESS.store(false, Ordering::Release);
        return Err(error);
    }

    log::info!(
        "Starting download of update: {} from {}",
        asset_name,
        download_url
    );
    tauri::async_runtime::spawn(async move {
        let _guard = UpdateInProgressGuard;
        if let Err(e) = perform_download_and_install(&app, &download_url, &asset_name).await {
            log::error!("OTA update download or installation failed: {}", e);
            let _ = app.emit(
                "ota_download_progress",
                ProgressPayload {
                    state: "error".into(),
                    progress: 0,
                    error: Some(e),
                },
            );
        }
    });
    Ok(())
}

struct UpdateInProgressGuard;

impl Drop for UpdateInProgressGuard {
    fn drop(&mut self) {
        UPDATE_IN_PROGRESS.store(false, Ordering::Release);
    }
}

fn validate_release_asset(asset: &GithubAsset) -> Result<(), String> {
    validate_installer_asset_name(&asset.name)?;
    validate_official_download_url(&asset.browser_download_url)
}

fn installer_path(asset_name: &str) -> Result<PathBuf, String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let dir = std::env::temp_dir().join("widgitron-updates").join(format!(
        "{}-{}",
        std::process::id(),
        nonce
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create update directory: {}", e))?;
    Ok(dir.join(asset_name))
}

async fn perform_download_and_install(
    app: &AppHandle,
    download_url: &str,
    asset_name: &str,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("widgitron-updater")
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600)) // 10 minutes total timeout for download
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {:?}", e))?;

    let mut response = client
        .get(download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download update file: {:?}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Download request failed with status: {}",
            response.status()
        ));
    }
    validate_github_response_url(response.url().as_str())?;

    let total_size = response.content_length().unwrap_or(0);
    if total_size > MAX_INSTALLER_BYTES {
        return Err(format!("Installer is too large: {} bytes", total_size));
    }
    log::info!("Download size: {} bytes", total_size);

    let installer_path = installer_path(asset_name)?;
    log::info!("Saving installer to: {:?}", installer_path);

    let mut file = std::fs::File::create(&installer_path)
        .map_err(|e| format!("Failed to create destination file: {:?}", e))?;

    // Emit initial progress
    let _ = app.emit(
        "ota_download_progress",
        ProgressPayload {
            state: "downloading".into(),
            progress: 0,
            error: None,
        },
    );

    let mut downloaded: u64 = 0;
    let mut last_emitted_percentage = 0u8;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Error receiving chunk: {:?}", e))?
    {
        use std::io::Write;
        file.write_all(&chunk)
            .map_err(|e| format!("Error writing chunk to file: {:?}", e))?;

        downloaded += chunk.len() as u64;
        if downloaded > MAX_INSTALLER_BYTES {
            return Err("Installer exceeded the maximum allowed size".to_string());
        }

        if total_size > 0 {
            let percentage = ((downloaded as f64 / total_size as f64) * 100.0) as u8;
            if percentage > last_emitted_percentage {
                last_emitted_percentage = percentage;
                let _ = app.emit(
                    "ota_download_progress",
                    ProgressPayload {
                        state: "downloading".into(),
                        progress: percentage,
                        error: None,
                    },
                );
            }
        }
    }

    // Flush and close the file
    drop(file);
    log::info!("Download completed successfully. Launching installer...");

    run_installer(&installer_path)?;

    let _ = app.emit(
        "ota_download_progress",
        ProgressPayload {
            state: "completed".into(),
            progress: 100,
            error: None,
        },
    );

    Ok(())
}

fn run_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let mut command = if extension.eq_ignore_ascii_case("msi") {
            let mut command = std::process::Command::new("msiexec.exe");
            command.arg("/i").arg(path);
            command
        } else {
            std::process::Command::new(path)
        };
        command
            .spawn()
            .map_err(|e| format!("Failed to start installer process: {}", e))?;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        return Err("Automatic installation is only supported on Windows".to_string());
    }

    Ok(())
}
