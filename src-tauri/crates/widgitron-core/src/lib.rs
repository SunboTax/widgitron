use chrono::{DateTime, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

const RELEASE_DOWNLOAD_PATH_PREFIX: &str = "/starkmomo/widgitron/releases/download/";

fn localize_deadline(naive: NaiveDateTime, timezone: &str) -> Result<DateTime<Utc>, String> {
    let timezone = timezone.trim();
    if timezone.eq_ignore_ascii_case("AoE") {
        let offset =
            FixedOffset::west_opt(12 * 60 * 60).ok_or_else(|| "Invalid AoE offset".to_string())?;
        return offset
            .from_local_datetime(&naive)
            .single()
            .map(|date| date.with_timezone(&Utc))
            .ok_or_else(|| "Invalid AoE deadline".to_string());
    }
    if timezone.eq_ignore_ascii_case("UTC") || timezone.eq_ignore_ascii_case("GMT") {
        return Ok(Utc.from_utc_datetime(&naive));
    }

    let parsed_timezone = timezone
        .parse::<Tz>()
        .map_err(|_| format!("Unsupported deadline timezone '{}'", timezone))?;
    match parsed_timezone.from_local_datetime(&naive) {
        LocalResult::Single(date) => Ok(date.with_timezone(&Utc)),
        LocalResult::Ambiguous(_, latest) => Ok(latest.with_timezone(&Utc)),
        LocalResult::None => Err(format!(
            "Deadline time '{}' does not exist in timezone '{}'",
            naive, timezone
        )),
    }
}

pub fn parse_deadline_utc(deadline: &str, timezone: &str) -> Result<DateTime<Utc>, String> {
    let normalized = deadline.trim().replace(' ', "T");
    if let Ok(parsed) = DateTime::parse_from_rfc3339(&normalized) {
        return Ok(parsed.with_timezone(&Utc));
    }

    let naive = if normalized.len() == 10 {
        NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
            .map_err(|e| format!("Invalid deadline date '{}': {}", deadline, e))?
            .and_hms_opt(23, 59, 59)
            .ok_or_else(|| format!("Invalid deadline date '{}'", deadline))?
    } else {
        ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"]
            .iter()
            .find_map(|format| NaiveDateTime::parse_from_str(&normalized, format).ok())
            .ok_or_else(|| format!("Invalid deadline timestamp '{}'", deadline))?
    };
    localize_deadline(naive, timezone)
}

pub fn truncate_to_limit<T>(items: &mut Vec<T>, max_items: Option<usize>) {
    if let Some(max_items) = max_items {
        items.truncate(max_items);
    }
}

pub fn clamp_poll_interval_secs(value: u64, minimum: u64, maximum: u64) -> u64 {
    value.max(minimum).min(maximum.max(minimum))
}

pub fn validate_external_link(link: &str) -> Result<(), String> {
    let url = url::Url::parse(link).map_err(|e| format!("Invalid external URL: {}", e))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("External links must use HTTP or HTTPS and include a host".to_string());
    }
    Ok(())
}

pub fn validate_installer_asset_name(asset_name: &str) -> Result<(), String> {
    let lower = asset_name.to_ascii_lowercase();
    let unsafe_name = asset_name.is_empty()
        || asset_name.trim() != asset_name
        || asset_name == "."
        || asset_name == ".."
        || asset_name.contains(['/', '\\', ':']);
    if unsafe_name || !(lower.ends_with(".exe") || lower.ends_with(".msi")) {
        return Err("Release asset name is not a supported installer basename".to_string());
    }
    Ok(())
}

pub fn validate_official_download_url(download_url: &str) -> Result<(), String> {
    let url = url::Url::parse(download_url)
        .map_err(|e| format!("Invalid release download URL: {}", e))?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || url.host_str() != Some("github.com")
        || !url.path().starts_with(RELEASE_DOWNLOAD_PATH_PREFIX)
    {
        return Err("Release download URL is not an official Widgitron GitHub asset".to_string());
    }
    Ok(())
}

pub fn validate_github_response_url(response_url: &str) -> Result<(), String> {
    let url = url::Url::parse(response_url)
        .map_err(|e| format!("Invalid installer response URL: {}", e))?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !(host == "github.com" || host.ends_with(".githubusercontent.com"))
    {
        return Err("Installer download redirected outside GitHub".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aoe_deadline_as_utc_minus_twelve() {
        let parsed = parse_deadline_utc("2026-01-01 23:59:59", "AoE").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-01-02T11:59:59+00:00");
    }

    #[test]
    fn preserves_explicit_rfc3339_offset() {
        let parsed = parse_deadline_utc("2026-01-01T23:59:59+08:00", "AoE").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-01-01T15:59:59+00:00");
    }

    #[test]
    fn parses_iana_timezone_with_daylight_saving() {
        let parsed = parse_deadline_utc("2026-07-01 12:00:00", "America/New_York").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-07-01T16:00:00+00:00");
    }

    #[test]
    fn date_only_deadline_uses_end_of_local_day() {
        let parsed = parse_deadline_utc("2026-01-01", "Asia/Shanghai").unwrap();
        assert_eq!(parsed.to_rfc3339(), "2026-01-01T15:59:59+00:00");
    }

    #[test]
    fn truncates_only_when_limit_is_present() {
        let mut limited = vec![1, 2, 3];
        truncate_to_limit(&mut limited, Some(2));
        assert_eq!(limited, vec![1, 2]);

        let mut unlimited = vec![1, 2, 3];
        truncate_to_limit(&mut unlimited, None);
        assert_eq!(unlimited, vec![1, 2, 3]);
    }

    #[test]
    fn clamps_poll_intervals_to_safe_bounds() {
        assert_eq!(clamp_poll_interval_secs(0, 1, 86_400), 1);
        assert_eq!(clamp_poll_interval_secs(5, 1, 86_400), 5);
        assert_eq!(clamp_poll_interval_secs(u64::MAX, 1, 86_400), 86_400);
        assert_eq!(clamp_poll_interval_secs(0, 60, 10), 60);
    }

    #[test]
    fn accepts_only_hosted_http_external_links() {
        assert!(validate_external_link("https://arxiv.org/abs/2601.00001").is_ok());
        assert!(validate_external_link("http://example.test/path").is_ok());
        assert!(validate_external_link("javascript:alert(1)").is_err());
        assert!(validate_external_link("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(validate_external_link("https://").is_err());
        assert!(validate_external_link("not a url").is_err());
    }

    #[test]
    fn accepts_only_installer_basenames() {
        assert!(validate_installer_asset_name("widgitron_x64-setup.exe").is_ok());
        assert!(validate_installer_asset_name("widgitron.msi").is_ok());
        assert!(validate_installer_asset_name("../payload.exe").is_err());
        assert!(validate_installer_asset_name("..\\payload.exe").is_err());
        assert!(validate_installer_asset_name("C:\\payload.exe").is_err());
        assert!(validate_installer_asset_name("payload.exe ").is_err());
        assert!(validate_installer_asset_name("notes.txt").is_err());
    }

    #[test]
    fn accepts_only_official_release_downloads() {
        assert!(validate_official_download_url(
            "https://github.com/starkmomo/widgitron/releases/download/v0.3.0/widgitron.exe"
        )
        .is_ok());
        assert!(validate_official_download_url(
            "http://github.com/starkmomo/widgitron/releases/download/v0.3.0/widgitron.exe"
        )
        .is_err());
        assert!(validate_official_download_url(
            "https://github.com/attacker/widgitron/releases/download/v0.3.0/widgitron.exe"
        )
        .is_err());
        assert!(validate_official_download_url(
            "https://github.com:444/starkmomo/widgitron/releases/download/v0.3.0/widgitron.exe"
        )
        .is_err());
    }

    #[test]
    fn accepts_only_github_redirect_hosts() {
        assert!(validate_github_response_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/file"
        )
        .is_ok());
        assert!(
            validate_github_response_url("https://githubusercontent.com.evil.test/file").is_err()
        );
        assert!(validate_github_response_url("https://example.com/file").is_err());
        assert!(validate_github_response_url("https://github.com:444/file").is_err());
    }
}
