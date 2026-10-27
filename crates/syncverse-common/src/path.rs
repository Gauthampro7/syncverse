//! Path and URI normalization utilities for Syncplay-Alt.
//!
//! Provides pure-Rust zero-dependency URI stripping (`file://localhost`, `file://`, `file:`)
//! and percent-decoding (`%20` -> space, `%2F` -> `/`, etc.) for canonical filesystem path resolution.

/// Decodes a hexadecimal ASCII byte into its 4-bit numeric value.
#[inline]
fn from_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-decodes a percent-encoded string slice into a decoded UTF-8 `String`.
///
/// Converts escape sequences of the form `%XX` (where `XX` are hexadecimal digits)
/// into their decoded byte values. If a `%` is not followed by two valid hex digits,
/// it is preserved verbatim. Invalid UTF-8 byte sequences are replaced losslessly.
pub fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h1), Some(h2)) = (from_hex_digit(bytes[i + 1]), from_hex_digit(bytes[i + 2])) {
                decoded.push((h1 << 4) | h2);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(decoded)
        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// Normalizes a target string into a canonical filesystem path.
///
/// If `target` is a `file:` URI:
/// - Strips `file://localhost`, `file://`, or `file:` scheme prefixes.
/// - Percent-decodes escape sequences (e.g., `%20` -> space, `%2F` -> `/`).
///
/// If `target` is not a `file:` URI (e.g. standard file paths, or synthetic test vectors like
/// `"synthetic-mkv"`), it is returned unchanged.
pub fn normalize_target_path(target: &str) -> String {
    let raw_path = if let Some(stripped) = target.strip_prefix("file://") {
        if let Some(local) = stripped.strip_prefix("localhost") {
            local
        } else {
            stripped
        }
    } else if let Some(stripped) = target.strip_prefix("file:") {
        stripped
    } else {
        return target.to_string();
    };

    percent_decode(raw_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_raw_path_untouched() {
        assert_eq!(normalize_target_path("/home/user/video.mkv"), "/home/user/video.mkv");
        assert_eq!(normalize_target_path("synthetic-mkv"), "synthetic-mkv");
        assert_eq!(normalize_target_path("synthetic-full-mp4"), "synthetic-full-mp4");
        assert_eq!(normalize_target_path("relative/path/movie.mp4"), "relative/path/movie.mp4");
    }

    #[test]
    fn test_normalize_file_uri_triple_slash() {
        assert_eq!(
            normalize_target_path("file:///home/user/video.mkv"),
            "/home/user/video.mkv"
        );
        assert_eq!(
            normalize_target_path("file:///tmp/test.webm"),
            "/tmp/test.webm"
        );
    }

    #[test]
    fn test_normalize_file_uri_localhost() {
        assert_eq!(
            normalize_target_path("file://localhost/home/user/video.mkv"),
            "/home/user/video.mkv"
        );
        assert_eq!(
            normalize_target_path("file://localhost/tmp/test.mp4"),
            "/tmp/test.mp4"
        );
    }

    #[test]
    fn test_normalize_file_uri_single_slash() {
        assert_eq!(
            normalize_target_path("file:/home/user/video.mkv"),
            "/home/user/video.mkv"
        );
    }

    #[test]
    fn test_percent_decoding_spaces_and_symbols() {
        assert_eq!(
            normalize_target_path("file:///home/user/My%20Summer%20Vacation.mkv"),
            "/home/user/My Summer Vacation.mkv"
        );
        assert_eq!(
            normalize_target_path("file:///tmp/path%2Fwith%2Fslashes.mkv"),
            "/tmp/path/with/slashes.mkv"
        );
        assert_eq!(
            normalize_target_path("file:///tmp/video%231%3F%26%2B.mp4"),
            "/tmp/video#1?&+.mp4"
        );
    }

    #[test]
    fn test_percent_decoding_multibyte_utf8() {
        // "éléphant" in UTF-8: %C3%A9, %C3%A9
        assert_eq!(
            normalize_target_path("file:///tmp/%C3%A9l%C3%A9phant.mkv"),
            "/tmp/éléphant.mkv"
        );
        // "東京" (Tokyo) in UTF-8: %E6%9D%B1, %E4%BA%AC
        assert_eq!(
            normalize_target_path("file:///tmp/%E6%9D%B1%E4%BA%AC.mkv"),
            "/tmp/東京.mkv"
        );
    }

    #[test]
    fn test_percent_decoding_malformed_and_boundary() {
        // Trailing % without digits
        assert_eq!(normalize_target_path("file:///tmp/movie%"), "/tmp/movie%");
        // % followed by only one digit
        assert_eq!(normalize_target_path("file:///tmp/movie%2"), "/tmp/movie%2");
        // % followed by non-hex digits
        assert_eq!(normalize_target_path("file:///tmp/movie%ZZ"), "/tmp/movie%ZZ");
        // Empty target
        assert_eq!(normalize_target_path(""), "");
        assert_eq!(normalize_target_path("file://"), "");
        assert_eq!(normalize_target_path("file://localhost"), "");
    }
}
