//! RTSP 1.0 / HTTP/1.1 request framing for the AirPlay control channel.

use std::collections::HashMap;
use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Parsed control request (RTSP or HTTP).
#[derive(Debug, Clone)]
pub struct ControlRequest {
    pub method: String,
    pub path: String,
    pub version: String,
    /// Original header names preserved; lookup helpers are case-insensitive.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl ControlRequest {
    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Session id from `Active-Remote` or `X-Apple-Session-ID`, else `"default"`.
    pub fn session_id(&self) -> &str {
        self.header("Active-Remote")
            .or_else(|| self.header("X-Apple-Session-ID"))
            .unwrap_or("default")
    }

    pub fn is_rtsp(&self) -> bool {
        self.version.to_ascii_uppercase().starts_with("RTSP/")
    }

    pub fn is_http(&self) -> bool {
        self.version.to_ascii_uppercase().starts_with("HTTP/")
    }

    /// Path without query string.
    pub fn path_only(&self) -> &str {
        self.path.split('?').next().unwrap_or(self.path.as_str())
    }
}

/// Outgoing control response.
#[derive(Debug, Clone)]
pub struct ControlResponse {
    pub version: String,
    pub status: u16,
    pub reason: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl ControlResponse {
    pub fn new(version: impl Into<String>, status: u16, reason: &'static str) -> Self {
        Self {
            version: version.into(),
            status,
            reason,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn ok_rtsp() -> Self {
        Self::new("RTSP/1.0", 200, "OK")
    }

    pub fn ok_http() -> Self {
        Self::new("HTTP/1.1", 200, "OK")
    }

    pub fn not_found(version: &str) -> Self {
        let v = if version.to_ascii_uppercase().starts_with("HTTP/") {
            "HTTP/1.1"
        } else {
            "RTSP/1.0"
        };
        Self::new(v, 404, "Not Found")
    }

    pub fn bad_request(version: &str) -> Self {
        let v = if version.to_ascii_uppercase().starts_with("HTTP/") {
            "HTTP/1.1"
        } else {
            "RTSP/1.0"
        };
        Self::new(v, 400, "Bad Request")
    }

    pub fn with_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Echo CSeq and add standard Server header when CSeq is present.
    pub fn with_cseq_from(mut self, request: &ControlRequest) -> Self {
        if let Some(cseq) = request.header("CSeq") {
            self.headers.push(("CSeq".into(), cseq.to_string()));
            self.headers
                .push(("Server".into(), "AirTunes/220.68".into()));
        }
        self
    }

    /// Serialize to wire bytes (status line + headers + body).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.body.len());
        out.extend_from_slice(
            format!("{} {} {}\r\n", self.version, self.status, self.reason).as_bytes(),
        );
        for (k, v) in &self.headers {
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(
            format!("Content-Length: {}\r\n\r\n", self.body.len()).as_bytes(),
        );
        out.extend_from_slice(&self.body);
        out
    }
}

/// Read one request from `reader`.
///
/// `pending` holds leftover bytes from a previous read (keep-alive pipelining).
/// Returns `Ok(None)` on clean EOF when no partial request is buffered.
pub async fn read_request<R: AsyncRead + Unpin>(
    reader: &mut R,
    pending: &mut Vec<u8>,
) -> io::Result<Option<ControlRequest>> {
    let mut tmp = [0u8; 2048];

    // Read until end of headers (using `pending` as the accumulation buffer).
    let header_end = loop {
        if let Some(pos) = find_header_end(pending) {
            break pos;
        }
        if pending.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control headers too large",
            ));
        }
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            if pending.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "eof before end of headers",
            ));
        }
        pending.extend_from_slice(&tmp[..n]);
    };

    let header_bytes = pending[..header_end].to_vec();
    // Drop headers + CRLFCRLF; remainder may include body and/or next request.
    let mut rest = pending.split_off(header_end + 4);
    pending.clear();

    let header_str = std::str::from_utf8(&header_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut lines = header_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing path"))?
        .to_string();
    let version = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing version"))?
        .to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_string(), value.trim().to_string());
        }
    }

    let content_length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);

    if content_length > MAX_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control body too large",
        ));
    }

    while rest.len() < content_length {
        let need = content_length - rest.len();
        let mut chunk = vec![0u8; need.min(8192)];
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "eof before full body",
            ));
        }
        rest.extend_from_slice(&chunk[..n]);
    }

    let body = rest[..content_length].to_vec();
    // Preserve any bytes past this request's body for the next call.
    *pending = rest[content_length..].to_vec();

    Ok(Some(ControlRequest {
        method,
        path,
        version,
        headers,
        body,
    }))
}

/// Write a full response to `writer`.
pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &ControlResponse,
) -> io::Result<()> {
    writer.write_all(&response.to_bytes()).await?;
    writer.flush().await
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn parse_get_info() {
        let raw = b"GET /info RTSP/1.0\r\nCSeq: 1\r\nActive-Remote: test-session\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let mut pending = Vec::new();
        let req = read_request(&mut reader, &mut pending)
            .await
            .expect("read")
            .expect("some");
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/info");
        assert_eq!(req.version, "RTSP/1.0");
        assert_eq!(req.session_id(), "test-session");
        assert_eq!(req.header("CSeq"), Some("1"));
        assert!(req.body.is_empty());
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn parse_body_content_length() {
        let raw = b"POST /pair-setup RTSP/1.0\r\nCSeq: 2\r\nContent-Length: 4\r\n\r\nabcd";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let mut pending = Vec::new();
        let req = read_request(&mut reader, &mut pending)
            .await
            .expect("read")
            .expect("some");
        assert_eq!(req.body, b"abcd");
    }

    #[tokio::test]
    async fn keep_alive_two_requests() {
        let raw = b"GET /info RTSP/1.0\r\nCSeq: 1\r\n\r\n\
POST /pair-setup RTSP/1.0\r\nCSeq: 2\r\nContent-Length: 0\r\n\r\n";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let mut pending = Vec::new();
        let r1 = read_request(&mut reader, &mut pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r1.method, "GET");
        let r2 = read_request(&mut reader, &mut pending)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(r2.method, "POST");
        assert_eq!(r2.path, "/pair-setup");
    }

    #[test]
    fn response_bytes_shape() {
        let req = ControlRequest {
            method: "GET".into(),
            path: "/info".into(),
            version: "RTSP/1.0".into(),
            headers: {
                let mut h = HashMap::new();
                h.insert("CSeq".into(), "1".into());
                h
            },
            body: vec![],
        };
        let resp = ControlResponse::ok_rtsp()
            .with_cseq_from(&req)
            .with_body(b"hi".to_vec());
        let bytes = resp.to_bytes();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("RTSP/1.0 200 OK\r\n"));
        assert!(s.contains("CSeq: 1\r\n"));
        assert!(s.contains("Content-Length: 2\r\n"));
        assert!(bytes.ends_with(b"hi"));
    }
}
