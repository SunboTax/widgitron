use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::Duration,
};
use tauri::{
    AppHandle, Manager, Monitor, PhysicalPosition, PhysicalSize, Position, Size, WebviewWindow,
};

use crate::{config_store, models::AppConfig};

const WIDGET_LAYOUTS_FILE: &str = "widget_layouts.json";
const SAVE_DEBOUNCE_MS: u64 = 300;
const MONITOR_POLL_INTERVAL_MS: u64 = 2000;
const MIN_WIDGET_WIDTH: u32 = 220;
const MIN_WIDGET_HEIGHT: u32 = 180;

pub const TRACKED_WIDGET_IDS: [&str; 4] = [
    "widget-gpu-default",
    "widget-deadlines-default",
    "widget-arxiv-default",
    "widget-quota-default",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NormalizedWidgetLayout {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WidgetLayoutStore {
    #[serde(default)]
    pub monitors: HashMap<String, HashMap<String, NormalizedWidgetLayout>>,
    #[serde(default)]
    pub active_monitor_by_widget: HashMap<String, String>,
}

#[derive(Default)]
pub struct WidgetLayoutSaveState {
    pub pending: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}

#[derive(Debug, Clone, Copy)]
struct MonitorWorkArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

pub fn is_tracked_widget(label: &str) -> bool {
    TRACKED_WIDGET_IDS.contains(&label)
}

pub fn spawn_monitor_watchdog(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_signature = monitor_signature(&app).unwrap_or_default();
        loop {
            tokio::time::sleep(Duration::from_millis(MONITOR_POLL_INTERVAL_MS)).await;
            let next_signature = match monitor_signature(&app) {
                Ok(signature) => signature,
                Err(err) => {
                    log::warn!("Monitor signature refresh failed: {}", err);
                    continue;
                }
            };
            if next_signature != last_signature {
                log::info!(
                    "Monitor topology changed from '{}' to '{}'",
                    last_signature,
                    next_signature
                );
                if let Err(err) = restore_widgets_after_monitor_change(&app).await {
                    log::warn!("Failed to restore widget layouts after monitor change: {}", err);
                }
                last_signature = next_signature;
            }
        }
    });
}

pub fn schedule_layout_persist(app: AppHandle, label: String) {
    if !is_tracked_widget(&label) {
        return;
    }

    let state = app.state::<WidgetLayoutSaveState>();
    let mut pending = match state.pending.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!("WidgetLayoutSaveState mutex poisoned, recovering");
            poisoned.into_inner()
        }
    };

    if let Some(handle) = pending.remove(&label) {
        handle.abort();
    }

    let app_clone = app.clone();
    let label_clone = label.clone();
    let handle = tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(SAVE_DEBOUNCE_MS)).await;
        if let Some(win) = app_clone.get_webview_window(&label_clone) {
            if let Err(err) = persist_layout_for_window(&app_clone, &win, &label_clone) {
                log::warn!("Failed to persist widget layout for {}: {}", label_clone, err);
            }
        }
        let state = app_clone.state::<WidgetLayoutSaveState>();
        let mut pending = match state.pending.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.remove(&label_clone);
    });

    pending.insert(label, handle);
}

pub fn persist_layout_now(app: &AppHandle, label: &str) -> Result<(), String> {
    if !is_tracked_widget(label) {
        return Ok(());
    }
    if let Some(win) = app.get_webview_window(label) {
        persist_layout_for_window(app, &win, label)?;
    }
    Ok(())
}

pub fn ensure_widget_layout(app: &AppHandle, label: &str) -> Result<(), String> {
    let Some(win) = app.get_webview_window(label) else {
        return Ok(());
    };
    ensure_widget_layout_for_window(app, &win, label)
}

pub fn ensure_widget_layout_for_window(
    app: &AppHandle,
    win: &WebviewWindow,
    label: &str,
) -> Result<(), String> {
    if !is_tracked_widget(label) {
        return Ok(());
    }

    let mut store = read_layout_store(app);
    let active_key = store.active_monitor_by_widget.get(label).cloned();
    let target_monitor = match active_key
        .as_deref()
        .and_then(|key| find_monitor_by_key(app, key).ok().flatten())
    {
        Some(monitor) => monitor,
        None => resolve_window_monitor(app, win)?
            .or_else(|| preferred_monitor(app))
            .ok_or_else(|| "No available monitor found for widget layout".to_string())?,
    };

    let target_key = monitor_key(&target_monitor);
    let source_layout = active_key
        .as_deref()
        .and_then(|key| store.monitors.get(key))
        .and_then(|widgets| widgets.get(label))
        .copied();
    let target_layout = store
        .monitors
        .get(&target_key)
        .and_then(|widgets| widgets.get(label))
        .copied();
    let layout = source_layout
        .or(target_layout)
        .or_else(|| default_layout_for_widget(label))
        .ok_or_else(|| format!("No default widget layout for {}", label))?;

    apply_layout_to_window(win, &target_monitor, layout)?;

    store
        .monitors
        .entry(target_key.clone())
        .or_default()
        .insert(label.to_string(), layout);
    store
        .active_monitor_by_widget
        .insert(label.to_string(), target_key.clone());
    write_layout_store(app, &store)?;

    log::info!("Applied widget layout for {} on monitor '{}'", label, target_key);
    Ok(())
}

async fn restore_widgets_after_monitor_change(app: &AppHandle) -> Result<(), String> {
    let store = read_layout_store(app);
    if store.active_monitor_by_widget.is_empty() {
        return Ok(());
    }

    let available_keys = available_monitor_keys(app)?;
    let app_config = config_store::read_config::<AppConfig>(app, "app_config.json");

    for label in TRACKED_WIDGET_IDS {
        let Some(source_key) = store.active_monitor_by_widget.get(label) else {
            continue;
        };
        if available_keys.contains(source_key) {
            continue;
        }
        if app.get_webview_window(label).is_none() {
            continue;
        }

        let pinned = app_config
            .always_on_top
            .as_ref()
            .and_then(|map| map.get(label))
            .copied()
            .unwrap_or(false);

        if !pinned {
            let _ = crate::desktop::set_desktop_mode(app.clone(), label.to_string(), false).await;
        }

        ensure_widget_layout(app, label)?;

        if !pinned {
            let _ = crate::desktop::set_desktop_mode(app.clone(), label.to_string(), true).await;
        }
    }

    Ok(())
}

fn persist_layout_for_window(
    app: &AppHandle,
    win: &WebviewWindow,
    label: &str,
) -> Result<(), String> {
    let Some(monitor) = resolve_window_monitor(app, win)? else {
        return Ok(());
    };

    let position = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.inner_size().map_err(|e| e.to_string())?;
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }

    let key = monitor_key(&monitor);
    let layout = physical_to_normalized(position, size, &monitor);
    let mut store = read_layout_store(app);
    store
        .monitors
        .entry(key.clone())
        .or_default()
        .insert(label.to_string(), layout);
    store
        .active_monitor_by_widget
        .insert(label.to_string(), key.clone());
    write_layout_store(app, &store)?;

    log::debug!("Persisted widget layout for {} on '{}'", label, key);
    Ok(())
}

fn apply_layout_to_window(
    win: &WebviewWindow,
    monitor: &Monitor,
    layout: NormalizedWidgetLayout,
) -> Result<(), String> {
    let (position, size) = normalized_to_physical(layout, monitor);
    win.set_size(Size::Physical(size))
        .map_err(|e| e.to_string())?;
    win.set_position(Position::Physical(position))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn default_layout_for_widget(label: &str) -> Option<NormalizedWidgetLayout> {
    // Tuned against the reference screenshot:
    // top edges align, the two middle cards share one column,
    // and horizontal/vertical gaps are kept visually consistent.
    let top = 0.024;
    let column_gap = 0.011;
    let row_gap = 0.016;

    let gpu_x = 0.453;
    let gpu_width = 0.182;

    let middle_x = gpu_x + gpu_width + column_gap;
    let middle_width = 0.164;

    let quota_x = middle_x + middle_width + column_gap;
    let quota_width = 0.168;

    let tall_height = 0.748;
    let deadline_height = 0.347;
    let arxiv_y = top + deadline_height + row_gap;
    let arxiv_height = (top + tall_height) - arxiv_y;

    match label {
        "widget-gpu-default" => Some(NormalizedWidgetLayout {
            x: gpu_x,
            y: top,
            width: gpu_width,
            height: tall_height,
        }),
        "widget-deadlines-default" => Some(NormalizedWidgetLayout {
            x: middle_x,
            y: top,
            width: middle_width,
            height: deadline_height,
        }),
        "widget-arxiv-default" => Some(NormalizedWidgetLayout {
            x: middle_x,
            y: arxiv_y,
            width: middle_width,
            height: arxiv_height,
        }),
        "widget-quota-default" => Some(NormalizedWidgetLayout {
            x: quota_x,
            y: top,
            width: quota_width,
            height: tall_height,
        }),
        _ => None,
    }
}

fn physical_to_normalized(
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    monitor: &Monitor,
) -> NormalizedWidgetLayout {
    let area = monitor_work_area(monitor);
    let width = area.width.max(1) as f64;
    let height = area.height.max(1) as f64;

    sanitize_layout(NormalizedWidgetLayout {
        x: (position.x - area.x) as f64 / width,
        y: (position.y - area.y) as f64 / height,
        width: size.width as f64 / width,
        height: size.height as f64 / height,
    })
}

fn normalized_to_physical(
    layout: NormalizedWidgetLayout,
    monitor: &Monitor,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let area = monitor_work_area(monitor);
    let layout = sanitize_layout(layout);

    let width = ((layout.width * area.width as f64).round() as u32)
        .max(MIN_WIDGET_WIDTH)
        .min(area.width.max(1));
    let height = ((layout.height * area.height as f64).round() as u32)
        .max(MIN_WIDGET_HEIGHT)
        .min(area.height.max(1));

    let desired_x = area.x + (layout.x * area.width as f64).round() as i32;
    let desired_y = area.y + (layout.y * area.height as f64).round() as i32;
    let max_x = area.x + area.width as i32 - width as i32;
    let max_y = area.y + area.height as i32 - height as i32;

    let x = desired_x.clamp(area.x, max_x.max(area.x));
    let y = desired_y.clamp(area.y, max_y.max(area.y));

    (
        PhysicalPosition::new(x, y),
        PhysicalSize::new(width, height),
    )
}

fn sanitize_layout(layout: NormalizedWidgetLayout) -> NormalizedWidgetLayout {
    let width = layout.width.clamp(0.08, 1.0);
    let height = layout.height.clamp(0.08, 1.0);
    let x = layout.x.clamp(0.0, (1.0 - width).max(0.0));
    let y = layout.y.clamp(0.0, (1.0 - height).max(0.0));

    NormalizedWidgetLayout {
        x,
        y,
        width,
        height,
    }
}

fn monitor_signature(app: &AppHandle) -> Result<String, String> {
    let mut monitors = app.available_monitors().map_err(|e| e.to_string())?;
    monitors.sort_by(|a, b| monitor_key(a).cmp(&monitor_key(b)));
    Ok(monitors
        .into_iter()
        .map(|monitor| {
            let key = monitor_key(&monitor);
            let area = monitor_work_area(&monitor);
            format!("{}@{},{}:{}x{}", key, area.x, area.y, area.width, area.height)
        })
        .collect::<Vec<_>>()
        .join("|"))
}

fn available_monitor_keys(app: &AppHandle) -> Result<HashSet<String>, String> {
    Ok(app
        .available_monitors()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|monitor| monitor_key(&monitor))
        .collect())
}

fn preferred_monitor(app: &AppHandle) -> Option<Monitor> {
    if let Some(main) = app.get_webview_window("main") {
        if let Ok(Some(monitor)) = main.current_monitor() {
            return Some(monitor);
        }
    }
    if let Ok(Some(primary)) = app.primary_monitor() {
        return Some(primary);
    }
    app.available_monitors()
        .ok()
        .and_then(|monitors| monitors.into_iter().next())
}

fn resolve_window_monitor(app: &AppHandle, win: &WebviewWindow) -> Result<Option<Monitor>, String> {
    if let Ok(Some(monitor)) = win.current_monitor() {
        return Ok(Some(monitor));
    }

    let position = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.inner_size().map_err(|e| e.to_string())?;
    let center_x = position.x + (size.width as i32 / 2);
    let center_y = position.y + (size.height as i32 / 2);

    for monitor in app.available_monitors().map_err(|e| e.to_string())? {
        let origin = monitor.position();
        let monitor_size = monitor.size();
        if center_x >= origin.x
            && center_x < origin.x + monitor_size.width as i32
            && center_y >= origin.y
            && center_y < origin.y + monitor_size.height as i32
        {
            return Ok(Some(monitor));
        }
    }

    Ok(None)
}

fn find_monitor_by_key(app: &AppHandle, key: &str) -> Result<Option<Monitor>, String> {
    for monitor in app.available_monitors().map_err(|e| e.to_string())? {
        if monitor_key(&monitor) == key {
            return Ok(Some(monitor));
        }
    }
    Ok(None)
}

fn monitor_key(monitor: &Monitor) -> String {
    if let Some(name) = monitor.name() {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let area = monitor_work_area(monitor);
    format!(
        "display@{},{}:{}x{}",
        area.x, area.y, area.width, area.height
    )
}

fn monitor_work_area(monitor: &Monitor) -> MonitorWorkArea {
    let area = monitor.work_area();
    MonitorWorkArea {
        x: area.position.x,
        y: area.position.y,
        width: area.size.width.max(1),
        height: area.size.height.max(1),
    }
}

fn read_layout_store(app: &AppHandle) -> WidgetLayoutStore {
    config_store::read_config::<WidgetLayoutStore>(app, WIDGET_LAYOUTS_FILE)
}

fn write_layout_store(app: &AppHandle, store: &WidgetLayoutStore) -> Result<(), String> {
    config_store::write_config(app, WIDGET_LAYOUTS_FILE, store)
}
