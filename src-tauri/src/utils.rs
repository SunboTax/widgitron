use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

fn is_dir_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let test = dir.join(".write_test");
    if fs::write(&test, b"").is_err() {
        return false;
    }
    let _ = fs::remove_file(test);
    true
}

pub fn is_program_files_install() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().to_lowercase()))
        .map(|s| s.contains("program files"))
        .unwrap_or(false)
}

fn portable_config_path(filename: &str) -> Option<PathBuf> {
    let mut exe_dir = std::env::current_exe().ok()?;
    exe_dir.pop();
    Some(exe_dir.join("configs").join(filename))
}

fn app_config_dir(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_default()
            .join("configs")
    })
}

fn should_use_portable_config(portable_path: &Path, app_data_path: &Path) -> bool {
    if is_program_files_install() {
        return false;
    }

    let portable_dir = match portable_path.parent() {
        Some(dir) => dir,
        None => return false,
    };

    if !is_dir_writable(portable_dir) {
        return false;
    }

    // Writable portable/dev layout: prefer exe-adjacent configs when present or AppData is empty.
    portable_path.exists() || !app_data_path.exists()
}

fn seed_config_from_resources(app: &AppHandle, filename: &str, dest: &Path) -> bool {
    if dest.exists() {
        return false;
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let resource_path = resource_dir.join("configs").join(filename);
        if resource_path.exists() {
            if fs::copy(&resource_path, dest).is_ok() {
                log::info!("Seeded config '{}' from bundled resources", filename);
                return true;
            }
            log::warn!("Failed to copy bundled config '{}' to {}", filename, dest.display());
        } else {
            log::debug!("Bundled config '{}' not found in resources", filename);
        }
    }
    false
}

const SEED_CONFIG_FILES: &[&str] = &[
    "app_config.json",
    "gpu_monitor.json",
    "quota_config.json",
    "paper_deadline.json",
    "antigravity_oauth.json",
];

/// Seed bundled default configs into the user config directory on first install.
pub fn ensure_default_configs(app: &AppHandle) {
    let config_dir = get_config_dir(app);
    let mut seeded = 0usize;
    for filename in SEED_CONFIG_FILES {
        let dest = config_dir.join(filename);
        if seed_config_from_resources(app, filename, &dest) {
            seeded += 1;
        }
    }
    if seeded > 0 {
        log::info!("Seeded {} default config file(s) into {}", seeded, config_dir.display());
    }
    if is_program_files_install() {
        log::info!(
            "Program Files install detected — user configs live in AppData: {}",
            config_dir.display()
        );
    }
}

/// Resolve the directory where user config files are stored.
pub fn get_config_dir(app: &AppHandle) -> PathBuf {
    if let Some(portable_path) = portable_config_path("app_config.json") {
        let app_data_path = app_config_dir(app).join("app_config.json");
        if should_use_portable_config(&portable_path, &app_data_path) {
            if let Some(dir) = portable_path.parent() {
                let _ = fs::create_dir_all(dir);
                return dir.to_path_buf();
            }
        }
    }

    let dir = app_config_dir(app);
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir
}

/// Resolve path for a single config file.
/// Installed apps under Program Files always use AppData so upgrades keep user settings.
pub fn get_config_path(app: &AppHandle, filename: &str) -> PathBuf {
    let app_data_dir = app_config_dir(app);
    let app_data_path = app_data_dir.join(filename);

    if let Some(portable_path) = portable_config_path(filename) {
        if should_use_portable_config(&portable_path, &app_data_path) {
            if let Some(dir) = portable_path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            return portable_path;
        }
    }

    if !app_data_dir.exists() {
        let _ = fs::create_dir_all(&app_data_dir);
    }

    seed_config_from_resources(app, filename, &app_data_path);
    app_data_path
}
