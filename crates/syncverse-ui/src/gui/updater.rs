//! Asynchronous GitHub Releases auto-update checker and release launcher.
//!
//! Provides non-blocking background update checks querying the GitHub Releases API,
//! SemVer 2.0.0 normalization and comparison against the running version,
//! rate-limit / timeout / offline fault tolerance, and cross-platform browser launching.

use std::sync::mpsc::Sender;
use std::time::Duration;
use thiserror::Error;

/// Target GitHub releases endpoint for the syncplay desktop application.
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/Gauthampro7/syncverse/releases/latest";

/// Default network timeout in seconds for GitHub Releases API queries.
pub const DEFAULT_UPDATE_TIMEOUT_SECS: u64 = 5;

/// Information describing an available remote application update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Currently running application version string.
    pub current_version: String,
    /// Latest remote version tag string without 'v' prefix.
    pub latest_version: String,
    /// Web URL to the GitHub release page.
    pub release_url: String,
    /// Direct download URL for the target platform asset, if found.
    pub download_url: Option<String>,
    /// Release notes body text, if available.
    pub release_notes: Option<String>,
}

/// Strongly typed errors encountered during update checking.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum UpdateError {
    /// Network connection is offline or unreachable.
    #[error("Network connection offline or unreachable: {0}")]
    NetworkOffline(String),
    /// Request timed out after the configured timeout duration.
    #[error("Update check timed out after {0} seconds")]
    Timeout(u64),
    /// GitHub API rate limit was exceeded.
    #[error("GitHub API rate limit exceeded (reset epoch: {reset_time})")]
    RateLimited {
        /// Formatted or epoch string describing when the rate limit resets.
        reset_time: String,
    },
    /// Failed to parse the JSON release response.
    #[error("Failed to parse GitHub release response: {0}")]
    ParseError(String),
    /// HTTP error response returned by the server.
    #[error("GitHub API returned HTTP status {status}: {message}")]
    HttpStatus {
        /// HTTP status code.
        status: u16,
        /// Description of the error.
        message: String,
    },
    /// Invalid SemVer tag received from the release.
    #[error("Invalid SemVer tag '{tag}': {details}")]
    InvalidSemver {
        /// The invalid tag string.
        tag: String,
        /// Details of the parse error.
        details: String,
    },
}

/// Strips leading 'v' or 'V' and trims whitespace from a tag string.
pub fn clean_semver_tag(tag: &str) -> String {
    let trimmed = tag.trim();
    let stripped = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    stripped.to_string()
}

/// Normalizes a version tag string and parses it into a SemVer `Version`.
pub fn normalize_semver_tag(tag: &str) -> Result<semver::Version, semver::Error> {
    let cleaned = clean_semver_tag(tag);
    semver::Version::parse(&cleaned)
}

/// Compares whether `remote_tag` represents a strictly newer version than `current_version_str`.
pub fn is_newer_version(current_version_str: &str, remote_tag: &str) -> Result<bool, semver::Error> {
    let current = normalize_semver_tag(current_version_str)?;
    let remote = normalize_semver_tag(remote_tag)?;
    Ok(remote > current)
}

/// Selects an asset download URL matching the target OS name ("linux", "windows", "macos").
pub fn select_asset_for_os(assets: &[(String, String)], os: &str) -> Option<String> {
    match os {
        "windows" => {
            for (name, url) in assets {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".exe") || lower.ends_with(".msi") || lower.contains("windows") {
                    return Some(url.clone());
                }
            }
        }
        "linux" => {
            for (name, url) in assets {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".appimage")
                    || lower.ends_with(".tar.gz")
                    || lower.ends_with(".rpm")
                    || lower.ends_with(".deb")
                    || lower.contains("linux")
                {
                    return Some(url.clone());
                }
            }
        }
        "macos" | "darwin" => {
            for (name, url) in assets {
                let lower = name.to_ascii_lowercase();
                if lower.ends_with(".dmg")
                    || lower.ends_with(".zip")
                    || lower.contains("darwin")
                    || lower.contains("macos")
                {
                    return Some(url.clone());
                }
            }
        }
        _ => {}
    }
    // Fallback to first asset if no OS-specific match found
    assets.first().map(|(_, url)| url.clone())
}

/// Selects the best download URL from the available release assets for the current host operating system.
pub fn select_platform_download_url(assets: &[(String, String)]) -> Option<String> {
    select_asset_for_os(assets, std::env::consts::OS)
}

/// Parses the GitHub release JSON structure, extracts semver, assets, and compares against `current_version`.
pub fn parse_release_json(
    current_version: &str,
    json: &serde_json::Value,
) -> Result<Option<UpdateInfo>, UpdateError> {
    let tag_name = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UpdateError::ParseError("Missing 'tag_name' in release JSON".to_string()))?;

    let html_url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(GITHUB_RELEASES_URL)
        .to_string();

    let release_notes = json
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let is_newer = is_newer_version(current_version, tag_name).map_err(|e| {
        UpdateError::InvalidSemver {
            tag: tag_name.to_string(),
            details: e.to_string(),
        }
    })?;

    if !is_newer {
        return Ok(None);
    }

    let clean_version = clean_semver_tag(tag_name);

    // Extract assets
    let assets = json
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_string();
                    let url = item.get("browser_download_url")?.as_str()?.to_string();
                    Some((name, url))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let download_url = select_platform_download_url(&assets);

    Ok(Some(UpdateInfo {
        current_version: current_version.to_string(),
        latest_version: clean_version,
        release_url: html_url,
        download_url,
        release_notes,
    }))
}

/// Inspects HTTP response status and headers for GitHub rate limiting conditions (HTTP 429 or HTTP 403 with remaining 0).
pub fn check_rate_limit(
    status: u16,
    remaining_header: Option<&str>,
    reset_header: Option<&str>,
) -> Option<UpdateError> {
    let remaining = remaining_header.unwrap_or("").trim();
    if status == 429 || (status == 403 && remaining == "0") {
        let reset_time = reset_header
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("unknown")
            .trim()
            .to_string();
        Some(UpdateError::RateLimited { reset_time })
    } else {
        None
    }
}

/// Checks for updates against a specified HTTP endpoint with a configurable timeout.
pub fn check_for_updates_at_endpoint(
    current_version: &str,
    endpoint_url: &str,
    timeout_secs: u64,
) -> Result<Option<UpdateInfo>, UpdateError> {
    let agent = ureq::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build();

    let user_agent = format!("syncverse/{}", current_version);
    let response = agent
        .get(endpoint_url)
        .set("User-Agent", &user_agent)
        .set("Accept", "application/vnd.github.v3+json")
        .call();

    match response {
        Ok(resp) => {
            let json_body: serde_json::Value = resp
                .into_json()
                .map_err(|e| UpdateError::ParseError(e.to_string()))?;
            parse_release_json(current_version, &json_body)
        }
        Err(ureq::Error::Status(status, resp)) => {
            if let Some(rate_err) = check_rate_limit(
                status,
                resp.header("x-ratelimit-remaining"),
                resp.header("x-ratelimit-reset"),
            ) {
                return Err(rate_err);
            }

            let msg = resp.into_string().unwrap_or_else(|_| "Unknown error".to_string());
            Err(UpdateError::HttpStatus {
                status,
                message: msg,
            })
        }
        Err(ureq::Error::Transport(transport)) => {
            let msg = transport.to_string();
            let msg_lower = msg.to_lowercase();
            if msg_lower.contains("timed out") || msg_lower.contains("timeout") {
                Err(UpdateError::Timeout(timeout_secs))
            } else {
                Err(UpdateError::NetworkOffline(msg))
            }
        }
    }
}

/// Checks for updates against the official GitHub releases endpoint synchronously with default timeout.
pub fn check_for_updates(current_version: &str) -> Result<Option<UpdateInfo>, UpdateError> {
    check_for_updates_at_endpoint(
        current_version,
        GITHUB_RELEASES_URL,
        DEFAULT_UPDATE_TIMEOUT_SECS,
    )
}

/// Spawns a background thread to check for application updates without blocking the UI thread.
pub fn spawn_update_check(
    current_version: &'static str,
    event_tx: Sender<crate::event::UiEvent>,
    ctx: egui::Context,
) {
    let _ = std::thread::Builder::new()
        .name("syncverse-updater".into())
        .spawn(move || {
            match check_for_updates(current_version) {
                Ok(Some(info)) => {
                    let _ = event_tx.send(crate::event::UiEvent::UpdateAvailable {
                        latest_version: info.latest_version,
                        release_url: info.release_url,
                        download_url: info.download_url,
                    });
                }
                Ok(None) => {
                    let _ = event_tx.send(crate::event::UiEvent::UpdateChecked {
                        is_latest: true,
                        current_version: current_version.to_string(),
                    });
                }
                Err(err) => {
                    let _ = event_tx.send(crate::event::UiEvent::UpdateFailed {
                        error: err.to_string(),
                    });
                }
            }
            ctx.request_repaint();
        });
}

/// Launches the system default web browser or desktop handler for the given URL.
pub fn open_browser_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()
            .map(|_| ())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Unsupported operating system for launching browser URL",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_semver_tag() {
        assert_eq!(clean_semver_tag("v0.1.2"), "0.1.2");
        assert_eq!(clean_semver_tag("V1.0.0"), "1.0.0");
        assert_eq!(clean_semver_tag(" 0.2.0 "), "0.2.0");
        assert_eq!(clean_semver_tag("  v0.1.1  "), "0.1.1");
        assert_eq!(clean_semver_tag("v1.2.3-beta.1"), "1.2.3-beta.1");
        assert_eq!(clean_semver_tag(""), "");
    }

    #[test]
    fn test_normalize_semver_tag_valid() {
        let v1 = normalize_semver_tag("v0.1.2").expect("valid semver");
        assert_eq!(v1.major, 0);
        assert_eq!(v1.minor, 1);
        assert_eq!(v1.patch, 2);

        let v2 = normalize_semver_tag("1.2.3-rc.1").expect("valid prerelease");
        assert_eq!(v2.major, 1);
        assert_eq!(v2.minor, 2);
        assert_eq!(v2.patch, 3);
        assert_eq!(v2.pre.as_str(), "rc.1");
    }

    #[test]
    fn test_normalize_semver_tag_invalid() {
        assert!(normalize_semver_tag("invalid_tag").is_err());
        assert!(normalize_semver_tag("").is_err());
        assert!(normalize_semver_tag("v").is_err());
        assert!(normalize_semver_tag("1.0").is_err());
    }

    #[test]
    fn test_is_newer_version_comparisons() {
        // v0.1.2 > 0.1.1
        assert!(is_newer_version("0.1.1", "v0.1.2").unwrap());
        // 0.2.0 > 0.1.9
        assert!(is_newer_version("0.1.9", "0.2.0").unwrap());
        // 1.0.0 > 0.9.9
        assert!(is_newer_version("0.9.9", "v1.0.0").unwrap());

        // Same version is not newer
        assert!(!is_newer_version("0.1.1", "v0.1.1").unwrap());
        assert!(!is_newer_version("0.1.1", "0.1.1").unwrap());

        // Older version is not newer
        assert!(!is_newer_version("0.1.2", "v0.1.1").unwrap());
        assert!(!is_newer_version("1.0.0", "v0.9.9").unwrap());

        // Invalid tags return error
        assert!(is_newer_version("0.1.1", "not_a_version").is_err());
        assert!(is_newer_version("bad_current", "v0.1.2").is_err());
    }

    #[test]
    fn test_parse_release_json_success_with_update() {
        let json = serde_json::json!({
            "tag_name": "v0.1.2",
            "html_url": "https://github.com/Gauthampro7/syncverse/releases/tag/v0.1.2",
            "body": "Bugfixes and performance improvements.",
            "assets": [
                {
                    "name": "syncverse-linux-x86_64.tar.gz",
                    "browser_download_url": "https://github.com/downloads/syncverse-linux.tar.gz"
                },
                {
                    "name": "syncverse-windows-x64.exe",
                    "browser_download_url": "https://github.com/downloads/syncverse-windows.exe"
                }
            ]
        });

        let result = parse_release_json("0.1.1", &json).unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.current_version, "0.1.1");
        assert_eq!(info.latest_version, "0.1.2");
        assert_eq!(
            info.release_url,
            "https://github.com/Gauthampro7/syncverse/releases/tag/v0.1.2"
        );
        assert!(info.download_url.is_some());
        assert_eq!(
            info.release_notes.as_deref(),
            Some("Bugfixes and performance improvements.")
        );
    }

    #[test]
    fn test_parse_release_json_already_latest() {
        let json = serde_json::json!({
            "tag_name": "v0.1.1",
            "html_url": "https://github.com/Gauthampro7/syncverse/releases/tag/v0.1.1",
            "assets": []
        });

        let result = parse_release_json("0.1.1", &json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_release_json_older_version() {
        let json = serde_json::json!({
            "tag_name": "v0.1.0",
            "html_url": "https://github.com/Gauthampro7/syncverse/releases/tag/v0.1.0",
            "assets": []
        });

        let result = parse_release_json("0.1.1", &json).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_release_json_missing_tag() {
        let json = serde_json::json!({
            "html_url": "https://github.com/example/release"
        });

        let result = parse_release_json("0.1.1", &json);
        assert!(matches!(result, Err(UpdateError::ParseError(_))));
    }

    #[test]
    fn test_select_asset_for_os() {
        let assets = vec![
            (
                "syncverse-linux-x86_64.tar.gz".to_string(),
                "https://example.com/linux.tar.gz".to_string(),
            ),
            (
                "syncverse-setup.exe".to_string(),
                "https://example.com/windows.exe".to_string(),
            ),
            (
                "syncverse.dmg".to_string(),
                "https://example.com/macos.dmg".to_string(),
            ),
        ];

        assert_eq!(
            select_asset_for_os(&assets, "linux"),
            Some("https://example.com/linux.tar.gz".to_string())
        );
        assert_eq!(
            select_asset_for_os(&assets, "windows"),
            Some("https://example.com/windows.exe".to_string())
        );
        assert_eq!(
            select_asset_for_os(&assets, "macos"),
            Some("https://example.com/macos.dmg".to_string())
        );

        // Fallback for unknown OS is first asset
        assert_eq!(
            select_asset_for_os(&assets, "freebsd"),
            Some("https://example.com/linux.tar.gz".to_string())
        );

        // Empty assets
        assert_eq!(select_asset_for_os(&[], "linux"), None);
    }

    #[test]
    fn test_offline_graceful_handling() {
        // Query an unroutable port on localhost which fails immediately
        let res = check_for_updates_at_endpoint("0.1.1", "http://127.0.0.1:9", 1);
        assert!(res.is_err());
        match res.unwrap_err() {
            UpdateError::NetworkOffline(_) | UpdateError::Timeout(_) => {}
            other => panic!("Expected NetworkOffline or Timeout, got: {:?}", other),
        }
    }

    #[test]
    fn test_check_rate_limit_conditions() {
        // HTTP 429 with reset time
        let err429 = check_rate_limit(429, Some("0"), Some("1730000000"));
        assert_eq!(
            err429,
            Some(UpdateError::RateLimited {
                reset_time: "1730000000".to_string()
            })
        );

        // HTTP 403 with x-ratelimit-remaining = 0
        let err403 = check_rate_limit(403, Some("0"), Some("1730000123"));
        assert_eq!(
            err403,
            Some(UpdateError::RateLimited {
                reset_time: "1730000123".to_string()
            })
        );

        // HTTP 403 with remaining > 0 is standard forbidden, not rate limit
        let not_rate_limit = check_rate_limit(403, Some("10"), Some("1730000123"));
        assert_eq!(not_rate_limit, None);

        // HTTP 404 is not a rate limit
        let not_rate_limit_404 = check_rate_limit(404, None, None);
        assert_eq!(not_rate_limit_404, None);
    }

    #[test]
    fn test_rate_limit_e2e_mock_server() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let response = "HTTP/1.1 403 Forbidden\r\n\
                                Content-Type: application/json\r\n\
                                x-ratelimit-remaining: 0\r\n\
                                x-ratelimit-reset: 1730000000\r\n\
                                Content-Length: 35\r\n\
                                \r\n\
                                {\"message\":\"API rate limit exceeded\"}";
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let url = format!("http://{}", addr);
        let res = check_for_updates_at_endpoint("0.1.1", &url, 2);
        assert_eq!(
            res,
            Err(UpdateError::RateLimited {
                reset_time: "1730000000".to_string()
            })
        );
    }

    #[test]
    fn test_mock_server_successful_update() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let body = r#"{
                    "tag_name": "v0.2.0",
                    "html_url": "https://github.com/test/repo/releases/v0.2.0",
                    "assets": [
                        {
                            "name": "syncverse-linux.tar.gz",
                            "browser_download_url": "https://github.com/test/download/syncverse-linux.tar.gz"
                        }
                    ]
                }"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\
                     \r\n\
                     {}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let url = format!("http://{}", addr);
        let res = check_for_updates_at_endpoint("0.1.1", &url, 2).expect("success");
        assert!(res.is_some());
        let info = res.unwrap();
        assert_eq!(info.latest_version, "0.2.0");
        assert_eq!(info.release_url, "https://github.com/test/repo/releases/v0.2.0");
    }
}
