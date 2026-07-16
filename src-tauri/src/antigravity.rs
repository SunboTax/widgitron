use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::AppHandle;

use crate::config_store;

const OAUTH_TOKEN_KEY: &str = "antigravityUnifiedStateSync.oauthToken";
const OAUTH_TOKEN_SENTINEL: &str = "oauthTokenInfoSentinelKey";
const DEFAULT_GOOGLE_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLOUD_CODE_URLS: &[&str] = &[
    "https://daily-cloudcode-pa.googleapis.com",
    "https://cloudcode-pa.googleapis.com",
];
const FETCH_MODELS_PATH: &str = "/v1internal:fetchAvailableModels";

const MODEL_BLACKLIST: &[&str] = &[
    "MODEL_CHAT_20706",
    "MODEL_CHAT_23310",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH_THINKING",
    "MODEL_GOOGLE_GEMINI_2_5_FLASH_LITE",
    "MODEL_GOOGLE_GEMINI_2_5_PRO",
    "MODEL_PLACEHOLDER_M19",
    "MODEL_PLACEHOLDER_M9",
    "MODEL_PLACEHOLDER_M12",
];

#[derive(Debug, Clone)]
pub struct ModelQuota {
    pub label: String,
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
}

#[derive(Debug, Clone)]
pub struct QuotaSnapshot {
    pub email: Option<String>,
    pub models: Vec<ModelQuota>,
    pub tier_name: Option<String>,
}

#[derive(Debug, Clone)]
struct OAuthTokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expiry_seconds: Option<i64>,
}

enum ProtobufField {
    Varint(u64),
    Bytes(Vec<u8>),
}

fn antigravity_db_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for app_name in ["Antigravity IDE", "Antigravity"] {
        if let Some(path) = antigravity_db_path(app_name) {
            if path.exists() {
                paths.push(path);
            }
        }
    }
    paths
}

fn antigravity_db_path(app_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").ok()?;
        Some(
            PathBuf::from(appdata)
                .join(app_name)
                .join("User")
                .join("globalStorage")
                .join("state.vscdb"),
        )
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(app_name)
                .join("User")
                .join("globalStorage")
                .join("state.vscdb"),
        )
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".config")
                .join(app_name)
                .join("User")
                .join("globalStorage")
                .join("state.vscdb"),
        )
    }
}

fn read_vscdb_key(db_path: &Path, target_key: &str) -> Result<Option<String>, String> {
    crate::sqlite_state::read_text_key(db_path, target_key)
}

fn read_varint(bytes: &[u8], mut pos: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while pos < bytes.len() {
        let byte = bytes[pos];
        pos += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

fn read_protobuf_fields(bytes: &[u8]) -> HashMap<u32, ProtobufField> {
    let mut fields = HashMap::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let (tag, next_pos) = match read_varint(bytes, pos) {
            Some(v) => v,
            None => break,
        };
        pos = next_pos;
        let field_num = (tag >> 3) as u32;
        let wire_type = tag & 0x07;
        match wire_type {
            0 => {
                let (value, next_pos) = match read_varint(bytes, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = next_pos;
                fields.insert(field_num, ProtobufField::Varint(value));
            }
            2 => {
                let (len, next_pos) = match read_varint(bytes, pos) {
                    Some(v) => v,
                    None => break,
                };
                pos = next_pos;
                let len = len as usize;
                if pos + len > bytes.len() {
                    break;
                }
                let data = bytes[pos..pos + len].to_vec();
                pos += len;
                fields.insert(field_num, ProtobufField::Bytes(data));
            }
            1 => pos = pos.saturating_add(8),
            5 => pos = pos.saturating_add(4),
            _ => break,
        }
    }
    fields
}

fn field_bytes<'a>(fields: &'a HashMap<u32, ProtobufField>, num: u32) -> Option<&'a [u8]> {
    match fields.get(&num) {
        Some(ProtobufField::Bytes(data)) => Some(data.as_slice()),
        _ => None,
    }
}

fn field_string(fields: &HashMap<u32, ProtobufField>, num: u32) -> Option<String> {
    field_bytes(fields, num)
        .and_then(|data| String::from_utf8(data.to_vec()).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn unwrap_oauth_token_blob(base64_text: &str) -> Option<Vec<u8>> {
    let outer = base64::engine::general_purpose::STANDARD
        .decode(base64_text.trim())
        .ok()?;
    let outer_fields = read_protobuf_fields(&outer);
    let wrapper_bytes = field_bytes(&outer_fields, 1)?;
    let wrapper_fields = read_protobuf_fields(wrapper_bytes);
    let sentinel = field_string(&wrapper_fields, 1)?;
    if sentinel != OAUTH_TOKEN_SENTINEL {
        return None;
    }
    let payload_bytes = field_bytes(&wrapper_fields, 2)?;
    let payload_fields = read_protobuf_fields(payload_bytes);
    let inner_b64 = field_string(&payload_fields, 1)?;
    base64::engine::general_purpose::STANDARD
        .decode(inner_b64)
        .ok()
}

fn parse_oauth_tokens(inner: &[u8]) -> OAuthTokens {
    let fields = read_protobuf_fields(inner);
    let expiry_seconds = field_bytes(&fields, 4).and_then(|ts_bytes| {
        let ts_fields = read_protobuf_fields(ts_bytes);
        match ts_fields.get(&1) {
            Some(ProtobufField::Varint(v)) => Some(*v as i64),
            _ => None,
        }
    });

    OAuthTokens {
        access_token: field_string(&fields, 1),
        refresh_token: field_string(&fields, 3),
        expiry_seconds,
    }
}

fn load_oauth_token_candidates() -> Vec<OAuthTokens> {
    let mut candidates = Vec::new();
    for db_path in antigravity_db_paths() {
        let Ok(Some(raw)) = read_vscdb_key(&db_path, OAUTH_TOKEN_KEY) else {
            continue;
        };
        let Some(inner) = unwrap_oauth_token_blob(&raw) else {
            continue;
        };
        let tokens = parse_oauth_tokens(&inner);
        if tokens.access_token.is_some() || tokens.refresh_token.is_some() {
            candidates.push(tokens);
        }
    }
    candidates
}

fn now_epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct AntigravityOAuthConfig {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    client_secret: String,
}

fn extract_gocspx_secret(content: &str) -> Option<String> {
    let marker = "GOCSPX-";
    let start = content.find(marker)?;
    let rest = &content[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
        .unwrap_or(rest.len());
    let secret = rest[..end].trim();
    if secret.len() > 8 {
        Some(secret.to_string())
    } else {
        None
    }
}

fn extract_google_client_id(content: &str) -> Option<String> {
    let marker = ".apps.googleusercontent.com";
    let end = content.find(marker)? + marker.len();
    let before = &content[..end];
    let start = before.rfind('"').or_else(|| before.rfind('\''))? + 1;
    let id = before[start..end].trim();
    if id.contains(".apps.googleusercontent.com") && id.len() > 20 {
        Some(id.to_string())
    } else {
        None
    }
}

fn read_scan_file(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn antigravity_install_scan_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in antigravity_install_roots() {
        paths.push(root.join("resources/app/product.json"));
        paths.push(root.join("resources/app/out/main.js"));
        paths.push(root.join("resources/app/out/vs/code/electron-main/main.js"));
        paths.push(root.join("resources/app/out/vs/workbench/workbench.desktop.main.js"));
    }
    paths
}

fn antigravity_install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            for name in [
                "Antigravity",
                "Antigravity IDE",
                "Google Antigravity",
                "antigravity",
            ] {
                roots.push(PathBuf::from(&local).join("Programs").join(name));
            }
            for name in ["Antigravity", "Antigravity IDE", "Google Antigravity"] {
                roots.push(PathBuf::from(&local).join(name));
            }
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            for name in ["Antigravity", "Antigravity IDE"] {
                roots.push(PathBuf::from(&pf).join(name));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            for name in ["Antigravity.app", "Antigravity IDE.app"] {
                roots.push(
                    PathBuf::from(&home)
                        .join("Applications")
                        .join(name)
                        .join("Contents/Resources/app"),
                );
            }
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(home) = std::env::var("HOME") {
            for name in ["antigravity", "Antigravity", "antigravity-ide"] {
                roots.push(PathBuf::from(&home).join(".local/share").join(name));
                roots.push(PathBuf::from(&home).join(name));
            }
        }
    }
    roots.retain(|path| path.exists());
    roots
}

fn scan_antigravity_ide_client_secret() -> Option<String> {
    for path in antigravity_install_scan_paths() {
        if let Some(content) = read_scan_file(&path) {
            if let Some(secret) = extract_gocspx_secret(&content) {
                return Some(secret);
            }
        }
    }
    None
}

fn scan_antigravity_ide_client_id() -> Option<String> {
    for path in antigravity_install_scan_paths() {
        if let Some(content) = read_scan_file(&path) {
            if let Some(client_id) = extract_google_client_id(&content) {
                return Some(client_id);
            }
        }
    }
    None
}

fn resolve_google_oauth_credentials(app: &AppHandle) -> Result<(String, String), String> {
    let cfg: AntigravityOAuthConfig = config_store::read_config(app, "antigravity_oauth.json");

    let client_id = std::env::var("ANTIGRAVITY_GOOGLE_CLIENT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let value = cfg.client_id.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .or_else(scan_antigravity_ide_client_id)
        .unwrap_or_else(|| DEFAULT_GOOGLE_CLIENT_ID.to_string());

    let client_secret = std::env::var("ANTIGRAVITY_GOOGLE_CLIENT_SECRET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let value = cfg.client_secret.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        })
        .or_else(scan_antigravity_ide_client_secret);

    match client_secret {
        Some(secret) if !secret.is_empty() => Ok((client_id, secret)),
        _ => Err(
            "Google OAuth client_secret not configured. Set ANTIGRAVITY_GOOGLE_CLIENT_SECRET, add client_secret to antigravity_oauth.json, or install Antigravity IDE."
                .to_string(),
        ),
    }
}

async fn refresh_google_access_token(
    app: &AppHandle,
    refresh_token: &str,
) -> Result<String, String> {
    let (client_id, client_secret) = resolve_google_oauth_credentials(app)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!(
            "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
            client_id, client_secret, refresh_token
        ))
        .send()
        .await
        .map_err(|e| format!("Google OAuth refresh failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Google OAuth refresh returned HTTP {}",
            resp.status()
        ));
    }

    let json: Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse OAuth refresh response: {}", e))?;
    json.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "OAuth refresh response missing access_token".to_string())
}

async fn fetch_cloud_models(access_token: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let mut last_error = String::from("Cloud Code API unavailable");

    for base in CLOUD_CODE_URLS {
        let url = format!("{}{}", base, FETCH_MODELS_PATH);
        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("User-Agent", "antigravity")
            .body("{}")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                return resp
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse Cloud Code response: {}", e));
            }
            Ok(resp)
                if resp.status() == reqwest::StatusCode::UNAUTHORIZED
                    || resp.status() == reqwest::StatusCode::FORBIDDEN =>
            {
                return Err("Google OAuth token expired or invalid".to_string());
            }
            Ok(resp) => {
                last_error = format!("Cloud Code API returned HTTP {}", resp.status());
            }
            Err(e) => {
                last_error = format!("Cloud Code request failed: {}", e);
            }
        }
    }

    Err(last_error)
}

fn parse_cloud_model_quotas(data: &Value) -> Vec<ModelQuota> {
    let Some(models) = data.get("models").and_then(|v| v.as_object()) else {
        return Vec::new();
    };

    let mut by_label: HashMap<String, ModelQuota> = HashMap::new();

    for (fallback_id, model) in models {
        if model.get("isInternal").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }

        let model_id = model
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_id);
        if MODEL_BLACKLIST.contains(&model_id) {
            continue;
        }

        let label = model
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if label.is_empty() {
            continue;
        }

        let quota_info = match model.get("quotaInfo") {
            Some(v) if v.is_object() => v,
            _ => continue,
        };

        let remaining_fraction = quota_info.get("remainingFraction").and_then(|v| v.as_f64());
        let reset_time = quota_info
            .get("resetTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let entry = ModelQuota {
            label: label.clone(),
            remaining_fraction,
            reset_time,
        };

        match by_label.get(&label) {
            Some(existing) => {
                let existing_frac = existing.remaining_fraction.unwrap_or(0.0);
                let new_frac = remaining_fraction.unwrap_or(0.0);
                if new_frac < existing_frac {
                    by_label.insert(label, entry);
                }
            }
            None => {
                by_label.insert(label, entry);
            }
        }
    }

    let mut models: Vec<ModelQuota> = by_label.into_values().collect();
    models.sort_by(|a, b| a.label.cmp(&b.label));
    models
}

async fn resolve_access_token(
    app: &AppHandle,
    candidates: &[OAuthTokens],
) -> Result<String, String> {
    let now = now_epoch_seconds();
    let mut access_tokens = Vec::new();

    for tokens in candidates {
        if let Some(access) = tokens.access_token.as_ref() {
            let expired = tokens
                .expiry_seconds
                .map(|expiry| expiry <= now)
                .unwrap_or(false);
            if !expired {
                access_tokens.push(access.clone());
            }
        }
    }

    for token in &access_tokens {
        match fetch_cloud_models(token).await {
            Ok(data) if !parse_cloud_model_quotas(&data).is_empty() => return Ok(token.clone()),
            Ok(_) => {}
            Err(e) if e.contains("expired or invalid") => {}
            Err(_) => {}
        }
    }

    let mut refresh_tokens = Vec::new();
    for tokens in candidates {
        if let Some(refresh) = tokens.refresh_token.as_ref() {
            if !refresh_tokens.iter().any(|existing| existing == refresh) {
                refresh_tokens.push(refresh.clone());
            }
        }
    }

    for refresh_token in refresh_tokens {
        let access = refresh_google_access_token(app, &refresh_token).await?;
        match fetch_cloud_models(&access).await {
            Ok(data) if !parse_cloud_model_quotas(&data).is_empty() => return Ok(access),
            Ok(_) => continue,
            Err(e) => return Err(e),
        }
    }

    Err(
        "No valid Antigravity Google OAuth credentials found. Sign in via Antigravity IDE."
            .to_string(),
    )
}

/// Fetch Antigravity quota via Google Cloud Code API using stored OAuth credentials.
pub async fn fetch_antigravity_via_cloud(app: &AppHandle) -> Result<QuotaSnapshot, String> {
    let candidates = load_oauth_token_candidates();
    if candidates.is_empty() {
        return Err(
            "Antigravity OAuth credentials not found. Sign in via Antigravity IDE once."
                .to_string(),
        );
    }

    let access_token = resolve_access_token(app, &candidates).await?;
    let data = fetch_cloud_models(&access_token).await?;
    let models = parse_cloud_model_quotas(&data);

    let tier_name = data
        .get("userTier")
        .or_else(|| data.get("userStatus").and_then(|us| us.get("userTier")))
        .and_then(|ut| ut.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());

    if models.is_empty() {
        return Err("No quota-tracked models returned from Antigravity Cloud API.".to_string());
    }

    Ok(QuotaSnapshot {
        email: None,
        models,
        tier_name,
    })
}

#[derive(serde::Serialize)]
pub struct AntigravitySetupStatus {
    pub has_oauth_tokens: bool,
    pub cloud_auth_ready: bool,
    pub language_server_running: bool,
    pub oauth_config_path: String,
    pub config_dir: String,
    pub program_files_install: bool,
}

pub fn get_setup_status(app: &AppHandle) -> AntigravitySetupStatus {
    let config_dir = crate::utils::get_config_dir(app);
    AntigravitySetupStatus {
        has_oauth_tokens: !load_oauth_token_candidates().is_empty(),
        cloud_auth_ready: resolve_google_oauth_credentials(app).is_ok(),
        language_server_running: crate::quota::is_antigravity_language_server_running(),
        oauth_config_path: config_dir
            .join("antigravity_oauth.json")
            .display()
            .to_string(),
        config_dir: config_dir.display().to_string(),
        program_files_install: crate::utils::is_program_files_install(),
    }
}
