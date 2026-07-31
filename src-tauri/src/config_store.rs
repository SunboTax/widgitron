use crate::models::{AppConfig, WidgetThemeConfig, WidgetVisibilityPayload};
use crate::utils::get_config_path;
use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

static CONFIG_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

fn config_lock() -> std::sync::MutexGuard<'static, ()> {
    match CONFIG_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("Config mutex poisoned, recovering");
            poisoned.into_inner()
        }
    }
}

fn backup_corrupt_config(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("No parent directory for corrupt config '{}'", path.display()))?;
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("config");

    for counter in 0..u32::MAX {
        let backup = parent.join(format!(
            "{}.{}.{}.corrupt.json",
            stem,
            std::process::id(),
            counter
        ));
        if backup.exists() {
            continue;
        }
        return fs::rename(path, backup)
            .map_err(|err| format!("Failed to back up corrupt config '{}': {}", path.display(), err));
    }

    Err(format!("No available backup filename for corrupt config '{}'", path.display()))
}
/// Read configuration of type T. If the file doesn't exist, returns default value.
/// If parsing fails, logs error, renames file to <filename>.corrupt.json, and returns default value.
pub fn read_config<T: DeserializeOwned + Default>(app: &AppHandle, filename: &str) -> T {
    let _guard = config_lock();
    read_config_unlocked(app, filename)
}

fn read_config_unlocked<T: DeserializeOwned + Default>(app: &AppHandle, filename: &str) -> T {
    let path = get_config_path(app, filename);
    if !path.exists() {
        return T::default();
    }

    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<T>(&content) {
            Ok(config) => config,
            Err(e) => {
                log::error!(
                    "Failed to parse config file '{}': {}. Backing up and returning defaults.",
                    filename,
                    e
                );
                if let Err(err) = backup_corrupt_config(&path) {
                    log::warn!("Failed to back up corrupt config '{}': {}", path.display(), err);
                }
                T::default()
            }
        },
        Err(e) => {
            log::error!(
                "Failed to read config file '{}': {}. Returning defaults.",
                filename,
                e
            );
            T::default()
        }
    }
}

/// Write configuration of type T atomically.
pub fn write_config<T: Serialize>(
    app: &AppHandle,
    filename: &str,
    config: &T,
) -> Result<(), String> {
    let _guard = config_lock();
    write_config_unlocked(app, filename, config)
}

fn write_config_unlocked<T: Serialize>(
    app: &AppHandle,
    filename: &str,
    config: &T,
) -> Result<(), String> {
    let path = get_config_path(app, filename);
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let parent = path
        .parent()
        .ok_or_else(|| "No parent directory for config path".to_string())?;
    let temp_filename = format!("{}.{}.tmp", filename, std::process::id());
    let temp_path = parent.join(temp_filename);

    let mut temp_file = fs::File::create(&temp_path)
        .map_err(|e| format!("Failed to create temp config file: {}", e))?;
    temp_file
        .write_all(content.as_bytes())
        .and_then(|_| temp_file.sync_all())
        .map_err(|e| format!("Failed to persist temp config file: {}", e))?;
    drop(temp_file);

    atomic_replace_file(&temp_path, &path)?;
    Ok(())
}

#[cfg(windows)]
fn atomic_replace_file(temp_path: &Path, path: &Path) -> Result<(), String> {
    if !path.exists() {
        return fs::rename(temp_path, path)
            .map_err(|e| format!("Failed to install config file: {}", e));
    }

    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACE_FILE_FLAGS};

    let replaced: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replacement: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        ReplaceFileW(
            PCWSTR(replaced.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS(0),
            None,
            None,
        )
        .map_err(|e| format!("Failed to atomically replace config file: {}", e))?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn atomic_replace_file(temp_path: &Path, path: &Path) -> Result<(), String> {
    fs::rename(temp_path, path).map_err(|e| format!("Failed to replace config file: {}", e))
}

pub fn update_config<T, F>(app: &AppHandle, filename: &str, update: F) -> Result<T, String>
where
    T: DeserializeOwned + Default + Serialize,
    F: FnOnce(&mut T),
{
    let _guard = config_lock();
    let mut config = read_config_unlocked::<T>(app, filename);
    update(&mut config);
    write_config_unlocked(app, filename, &config)?;
    Ok(config)
}

pub fn update_config_if_changed<T, F>(
    app: &AppHandle,
    filename: &str,
    update: F,
) -> Result<Option<T>, String>
where
    T: DeserializeOwned + Default + Serialize,
    F: FnOnce(&mut T) -> bool,
{
    let _guard = config_lock();
    let mut config = read_config_unlocked::<T>(app, filename);
    if !update(&mut config) {
        return Ok(None);
    }
    write_config_unlocked(app, filename, &config)?;
    Ok(Some(config))
}

/// Specialized theme configuration loader that handles legacy format migration.
pub fn read_theme_config(app: &AppHandle) -> WidgetThemeConfig {
    let _guard = config_lock();
    let path = get_config_path(app, "widget_themes.json");
    if !path.exists() {
        return WidgetThemeConfig::default();
    }

    let config_str = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            log::error!(
                "Failed to read widget_themes.json: {}. Returning default theme config.",
                e
            );
            return WidgetThemeConfig::default();
        }
    };

    // Try normal deserialization
    match serde_json::from_str::<WidgetThemeConfig>(&config_str) {
        Ok(mut config) => {
            // Sync default themes and assignments if missing
            let defaults = WidgetThemeConfig::default();
            config.themes.retain(|t| !t.id.ends_with("-transparent"));

            for default_theme in defaults.themes {
                if !config.themes.iter().any(|t| t.id == default_theme.id) {
                    config.themes.push(default_theme);
                }
            }

            // Sync missing default assignments
            for (widget_id, default_theme_id) in defaults.assignments {
                if !config.assignments.contains_key(&widget_id)
                    || config
                        .assignments
                        .get(&widget_id)
                        .map_or(true, |s| s.is_empty())
                {
                    config.assignments.insert(widget_id, default_theme_id);
                }
            }
            config
        }
        Err(_) => {
            // Migration from old format (text_color: String)
            match serde_json::from_str::<serde_json::Value>(&config_str) {
                Ok(mut val) => {
                    if let Some(themes) = val.get_mut("themes").and_then(|t| t.as_array_mut()) {
                        for theme in themes {
                            let old_color = theme.get("text_color").cloned();
                            if let Some(color_val) = old_color {
                                if color_val.is_string() {
                                    if let Some(obj) = theme.as_object_mut() {
                                        obj.insert(
                                            "text_colors".into(),
                                            serde_json::json!([
                                                { "name": "Main Text", "value": color_val, "opacity": 1.0 },
                                                { "name": "Sub Text", "value": "#94a3b8", "opacity": 0.6 }
                                            ]),
                                        );
                                        obj.remove("text_color");
                                    }
                                }
                            }
                        }
                    }
                    match serde_json::from_value::<WidgetThemeConfig>(val) {
                        Ok(mut migrated) => {
                            let defaults = WidgetThemeConfig::default();
                            migrated.themes.retain(|t| !t.id.ends_with("-transparent"));

                            for default_theme in defaults.themes {
                                if !migrated.themes.iter().any(|t| t.id == default_theme.id) {
                                    migrated.themes.push(default_theme);
                                }
                            }

                            for (widget_id, default_theme_id) in defaults.assignments {
                                if !migrated.assignments.contains_key(&widget_id)
                                    || migrated
                                        .assignments
                                        .get(&widget_id)
                                        .map_or(true, |s| s.is_empty())
                                {
                                    migrated.assignments.insert(widget_id, default_theme_id);
                                }
                            }

                            if let Err(e) =
                                write_config_unlocked(app, "widget_themes.json", &migrated)
                            {
                                log::warn!("Failed to persist migrated widget themes: {}", e);
                            }
                            migrated
                        }
                        Err(e) => {
                            log::error!("Failed to migrate widget_themes.json: {}. Backing up and returning defaults.", e);
                            if let Err(err) = backup_corrupt_config(&path) {
                                log::warn!("Failed to back up corrupt config '{}': {}", path.display(), err);
                            }
                            WidgetThemeConfig::default()
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to parse widget_themes.json as JSON: {}. Backing up and returning defaults.", e);
                    if let Err(err) = backup_corrupt_config(&path) {
                        log::warn!("Failed to back up corrupt config '{}': {}", path.display(), err);
                    }
                    WidgetThemeConfig::default()
                }
            }
        }
    }
}

/// Specialized theme configuration writer.
pub fn write_theme_config(app: &AppHandle, config: &WidgetThemeConfig) -> Result<(), String> {
    write_config(app, "widget_themes.json", config)
}

/// Seed default widget themes on first install when no bundled copy exists.
pub fn seed_default_theme_config_if_missing(app: &AppHandle) {
    let path = get_config_path(app, "widget_themes.json");
    if path.exists() {
        return;
    }
    let default = WidgetThemeConfig::default();
    if let Err(e) = write_theme_config(app, &default) {
        log::warn!("Failed to seed widget_themes.json: {}", e);
    } else {
        log::info!("Seeded default widget_themes.json");
    }
}

/// List backup files created when a config JSON failed to parse.
pub fn list_corrupt_config_files(app: &AppHandle) -> Vec<String> {
    let dir = crate::utils::get_config_dir(app);
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".corrupt.json") {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    files
}

/// Atomic helper to update widget visibility inside AppConfig.
pub async fn update_widget_visibility_config(
    app: &AppHandle,
    id: &str,
    visible: bool,
) -> Result<(), String> {
    update_config::<AppConfig, _>(app, "app_config.json", |config| {
        let mut active = config.active_widgets.take().unwrap_or_default();
        active.insert(id.to_string(), visible);
        config.active_widgets = Some(active);
    })?;
    let _ = app.emit(
        "widget_visibility_changed",
        WidgetVisibilityPayload {
            id: id.to_string(),
            visible,
        },
    );
    Ok(())
}
