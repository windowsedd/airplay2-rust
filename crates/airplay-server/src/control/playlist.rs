//! Managed-live-HLS (`mlhls://localhost`) playlist URL conversion and rewriting.
//!
//! Ports Java `ControlHandler` playlist helpers:
//! - `playlistUriToLocal` / `playlistPathToRemote`
//! - master playlist URI rewriting
//! - `YT-EXT-CONDENSED-URL` media segment expansion

use thiserror::Error;

const MAX_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_SESSION_ID_BYTES: usize = 256;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlaylistError {
    #[error("playlist exceeds 2097152 bytes")]
    TooLarge,
    #[error("playlist is not UTF-8")]
    Utf8,
    #[error("invalid playlist URL")]
    Url,
    #[error("invalid condensed URL metadata")]
    Condensed,
    #[error("invalid session id")]
    Session,
}

/// Convert a remote `mlhls://localhost/...` URL into a loopback proxy URL.
///
/// Example: `mlhls://localhost/master.m3u8` →
/// `http://127.0.0.1:7000/playlist/master.m3u8?session=session-1`
pub fn local_playlist_url(
    control_port: u16,
    session_id: &str,
    remote_url: &str,
) -> Result<String, PlaylistError> {
    validate_session_id(session_id)?;
    let path = mlhls_path(remote_url)?;
    let encoded_session = percent_encode_component(session_id.as_bytes());
    Ok(format!(
        "http://127.0.0.1:{control_port}/playlist{path}?session={encoded_session}"
    ))
}

/// Parse a local `/playlist/...` request path+query back into `(session_id, remote_mlhls_url)`.
pub fn remote_playlist_url(local_path_and_query: &str) -> Result<(String, String), PlaylistError> {
    let (path, query) = match local_path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (local_path_and_query, ""),
    };
    if !path.starts_with("/playlist") {
        return Err(PlaylistError::Url);
    }
    let rest = &path["/playlist".len()..];
    if !rest.is_empty() && !rest.starts_with('/') {
        return Err(PlaylistError::Url);
    }
    let session = query_param(query, "session").ok_or(PlaylistError::Session)?;
    let session = percent_decode_component(session)?;
    validate_session_id(&session)?;
    let remote = format!("mlhls://localhost{rest}");
    // Drop any accidental query on remote (Java splits on '?').
    let remote = remote.split('?').next().unwrap_or(&remote).to_string();
    Ok((session, remote))
}

/// Rewrite playlist body: convert `mlhls://localhost` URIs to local proxy URLs and
/// expand YouTube condensed segment URLs when present.
pub fn rewrite_playlist(
    body: &[u8],
    control_port: u16,
    session_id: &str,
) -> Result<Vec<u8>, PlaylistError> {
    if body.len() > MAX_PLAYLIST_BYTES {
        return Err(PlaylistError::TooLarge);
    }
    validate_session_id(session_id)?;
    let text = std::str::from_utf8(body).map_err(|_| PlaylistError::Utf8)?;
    let had_trailing_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();

    let condensed = lines
        .iter()
        .find_map(|line| parse_condensed_comment(line).transpose())
        .transpose()?;

    let mut out_lines = Vec::with_capacity(lines.len());
    for line in lines {
        if line.starts_with('#') {
            // Attribute URI rewrite inside tags (e.g. EXT-X-MEDIA).
            out_lines.push(rewrite_tag_uris(line, control_port, session_id)?);
            continue;
        }
        if line.is_empty() {
            out_lines.push(String::new());
            continue;
        }
        if let Some(ref condensed) = condensed {
            if let Some(expanded) = expand_condensed_segment(line, condensed)? {
                out_lines.push(expanded);
                continue;
            }
        }
        if line.starts_with("mlhls://localhost") {
            out_lines.push(local_playlist_url(control_port, session_id, line)?);
        } else {
            out_lines.push(line.to_string());
        }
    }

    let mut out = out_lines.join("\n");
    if had_trailing_newline {
        out.push('\n');
    }
    Ok(out.into_bytes())
}

#[derive(Debug, Clone)]
struct CondensedUrl {
    base_uri: String,
    prefix: String,
    params: Vec<String>,
}

fn parse_condensed_comment(line: &str) -> Result<Option<CondensedUrl>, PlaylistError> {
    const MARKER: &str = "#YT-EXT-CONDENSED-URL:";
    if !line.starts_with(MARKER) {
        return Ok(None);
    }
    let attrs = &line[MARKER.len()..];
    let mut base_uri = None;
    let mut prefix = None;
    let mut params = None;
    for (key, value) in parse_attribute_list(attrs) {
        match key.as_str() {
            "BASE-URI" => base_uri = Some(value),
            "PREFIX" => prefix = Some(value),
            "PARAMS" => params = Some(value),
            _ => {}
        }
    }
    let (Some(base_uri), Some(prefix), Some(params_raw)) = (base_uri, prefix, params) else {
        return Err(PlaylistError::Condensed);
    };
    if base_uri.is_empty() || prefix.is_empty() || params_raw.is_empty() {
        return Err(PlaylistError::Condensed);
    }
    let params: Vec<String> = params_raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if params.is_empty() {
        return Err(PlaylistError::Condensed);
    }
    Ok(Some(CondensedUrl {
        base_uri,
        prefix,
        params,
    }))
}

fn expand_condensed_segment(
    uri: &str,
    condensed: &CondensedUrl,
) -> Result<Option<String>, PlaylistError> {
    if !uri.starts_with(&condensed.prefix) {
        return Ok(None);
    }
    let rest = &uri[condensed.prefix.len()..];
    let values: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if values.len() != condensed.params.len() {
        return Err(PlaylistError::Condensed);
    }
    let mut result = condensed.base_uri.clone();
    for (name, value) in condensed.params.iter().zip(values.iter()) {
        result.push('/');
        result.push_str(name);
        result.push('/');
        result.push_str(value);
    }
    Ok(Some(result))
}

fn rewrite_tag_uris(
    line: &str,
    control_port: u16,
    session_id: &str,
) -> Result<String, PlaylistError> {
    // Rewrite URI="mlhls://localhost/..." attributes only.
    const NEEDLE: &str = "URI=\"mlhls://localhost";
    let mut out = String::with_capacity(line.len() + 32);
    let mut rest = line;
    while let Some(idx) = rest.find(NEEDLE) {
        out.push_str(&rest[..idx]);
        out.push_str("URI=\"");
        let after = &rest[idx + "URI=\"".len()..];
        let end = after.find('"').ok_or(PlaylistError::Url)?;
        let remote = &after[..end];
        let local = local_playlist_url(control_port, session_id, remote)?;
        out.push_str(&local);
        out.push('"');
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn parse_attribute_list(attrs: &str) -> Vec<(String, String)> {
    // KEY="value" or KEY=value, comma-separated (lenient).
    let mut out = Vec::new();
    let bytes = attrs.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = attrs[key_start..i].trim().to_string();
        i += 1; // '='
        if i >= bytes.len() {
            break;
        }
        let value = if bytes[i] == b'"' {
            i += 1;
            let vstart = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = attrs[vstart..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let vstart = i;
            while i < bytes.len() && bytes[i] != b',' {
                i += 1;
            }
            attrs[vstart..i].trim().to_string()
        };
        out.push((key, value));
    }
    out
}

fn mlhls_path(remote_url: &str) -> Result<&str, PlaylistError> {
    const PREFIX: &str = "mlhls://localhost";
    if !remote_url.starts_with(PREFIX) {
        return Err(PlaylistError::Url);
    }
    let rest = &remote_url[PREFIX.len()..];
    if rest.is_empty() {
        return Ok("/");
    }
    if !rest.starts_with('/') {
        return Err(PlaylistError::Url);
    }
    // Strip query if present.
    Ok(rest.split('?').next().unwrap_or(rest))
}

fn validate_session_id(session_id: &str) -> Result<(), PlaylistError> {
    if session_id.is_empty() || session_id.len() > MAX_SESSION_ID_BYTES {
        return Err(PlaylistError::Session);
    }
    if session_id.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(PlaylistError::Session);
    }
    Ok(())
}

fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    for part in query.split('&') {
        let mut kv = part.splitn(2, '=');
        let key = kv.next()?;
        let value = kv.next().unwrap_or("");
        if key == name {
            return Some(value);
        }
    }
    None
}

fn percent_encode_component(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for &b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(nibble(b >> 4));
                out.push(nibble(b & 0x0f));
            }
        }
    }
    out
}

fn nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'A' + (n - 10)) as char,
        _ => '0',
    }
}

fn percent_decode_component(input: &str) -> Result<String, PlaylistError> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(PlaylistError::Session);
                }
                let hi = from_hex(bytes[i + 1]).ok_or(PlaylistError::Session)?;
                let lo = from_hex(bytes[i + 2]).ok_or(PlaylistError::Session)?;
                out.push((hi << 4) | lo);
                i += 3;
            }
            b if b < 0x20 || b == 0x7f => return Err(PlaylistError::Session),
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    if out.len() > MAX_SESSION_ID_BYTES {
        return Err(PlaylistError::Session);
    }
    String::from_utf8(out).map_err(|_| PlaylistError::Session)
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_remote_playlist_urls_round_trip() {
        let local =
            local_playlist_url(7000, "session-1", "mlhls://localhost/master.m3u8").unwrap();
        assert_eq!(
            local,
            "http://127.0.0.1:7000/playlist/master.m3u8?session=session-1"
        );
        assert_eq!(
            remote_playlist_url("/playlist/master.m3u8?session=session-1").unwrap(),
            (
                "session-1".into(),
                "mlhls://localhost/master.m3u8".into()
            )
        );
    }

    #[test]
    fn rejects_non_mlhls_and_bad_session() {
        assert_eq!(
            local_playlist_url(7000, "s", "https://example.com/x"),
            Err(PlaylistError::Url)
        );
        assert_eq!(
            local_playlist_url(7000, "", "mlhls://localhost/x"),
            Err(PlaylistError::Session)
        );
        assert!(remote_playlist_url("/other/x?session=s").is_err());
    }

    #[test]
    fn rewrites_master_uris_and_preserves_tags() {
        let input = b"\
#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",URI=\"mlhls://localhost/audio.m3u8\"
#EXT-X-STREAM-INF:BANDWIDTH=2500000,AUDIO=\"audio\"
mlhls://localhost/video.m3u8
";
        let out = rewrite_playlist(input, 7000, "session-1").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(
            "URI=\"http://127.0.0.1:7000/playlist/audio.m3u8?session=session-1\""
        ));
        assert!(text.contains(
            "http://127.0.0.1:7000/playlist/video.m3u8?session=session-1"
        ));
        assert!(text.contains("#EXT-X-STREAM-INF:BANDWIDTH=2500000"));
        assert!(!text.contains("mlhls://localhost"));
    }

    #[test]
    fn expands_condensed_segment_url() {
        let input = b"\
#EXTM3U
#YT-EXT-CONDENSED-URL:BASE-URI=\"https://example.invalid/videoplayback\",PREFIX=\"seg/\",PARAMS=\"id,range\"
#EXTINF:2.0,
seg/demo/0-999
";
        let out = rewrite_playlist(input, 7000, "session-1").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains(
            "https://example.invalid/videoplayback/id/demo/range/0-999"
        ));
    }

    #[test]
    fn condensed_param_mismatch_errors() {
        let input = b"\
#EXTM3U
#YT-EXT-CONDENSED-URL:BASE-URI=\"https://example.invalid/v\",PREFIX=\"seg/\",PARAMS=\"id,range\"
seg/only-one
";
        assert_eq!(
            rewrite_playlist(input, 7000, "s"),
            Err(PlaylistError::Condensed)
        );
    }

    #[test]
    fn oversized_playlist_rejected() {
        let big = vec![b'a'; MAX_PLAYLIST_BYTES + 1];
        assert_eq!(
            rewrite_playlist(&big, 1, "s"),
            Err(PlaylistError::TooLarge)
        );
    }
}
