//! RTSP control client: connect, GET /info, pair-setup, pair-verify.

use std::collections::HashMap;
use std::io;

use aes::cipher::{KeyIvInit, StreamCipher};
use aes::Aes128;
use airplay_lib::{AirPlayError, Result};
use ctr::Ctr128BE;
use ed25519_dalek::{Signer, SigningKey};
use rand_core::OsRng;
use sha2::{Digest, Sha512};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

type Aes128Ctr = Ctr128BE<Aes128>;

const USER_AGENT: &str = "AirPlay/670.6.2";
const DEFAULT_DACP_ID: &str = "184F380D0A5B7139";
const DEFAULT_ACTIVE_REMOTE: &str = "1589992423";
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Outgoing / parsed control response (status line + headers + body).
#[derive(Debug, Clone)]
pub struct ControlResponse {
    pub version: String,
    pub status: u16,
    pub reason: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl ControlResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// AirPlay RTSP control client (sender → receiver).
pub struct ControlClient {
    stream: TcpStream,
    pending: Vec<u8>,
    cseq: u32,
    session_id: String,
    dacp_id: String,
    /// ECDH shared secret after successful pair-verify (both phases).
    shared_secret: Option<[u8; 32]>,
}

impl ControlClient {
    /// TCP connect to receiver control port.
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| AirPlayError::Io(e))?;
        info!(%addr, "ControlClient connected");
        Ok(Self {
            stream,
            pending: Vec::new(),
            cseq: 0,
            session_id: DEFAULT_ACTIVE_REMOTE.into(),
            dacp_id: DEFAULT_DACP_ID.into(),
            shared_secret: None,
        })
    }

    /// Override `Active-Remote` session id (default matches Java client sample).
    pub fn set_session_id(&mut self, id: impl Into<String>) {
        self.session_id = id.into();
    }

    /// ECDH shared secret from the last successful [`Self::pair_verify`], if any.
    pub fn shared_secret(&self) -> Option<&[u8; 32]> {
        self.shared_secret.as_ref()
    }

    /// `GET /info` — binary plist body (raw bytes).
    pub async fn get_info(&mut self) -> Result<Vec<u8>> {
        let resp = self
            .exchange("GET", "/info", &[])
            .await?;
        ensure_ok(&resp)?;
        Ok(resp.body)
    }

    /// `POST /pair-setup` — returns receiver Ed25519 public key (32 bytes).
    pub async fn pair_setup(&mut self) -> Result<[u8; 32]> {
        let resp = self
            .exchange("POST", "/pair-setup", &[])
            .await?;
        ensure_ok(&resp)?;
        if resp.body.len() != 32 {
            return Err(AirPlayError::Pairing(format!(
                "pair-setup expected 32-byte body, got {}",
                resp.body.len()
            )));
        }
        let mut pk = [0u8; 32];
        pk.copy_from_slice(&resp.body);
        Ok(pk)
    }

    /// Full client-side pair-verify (phase 1 + phase 2).
    ///
    /// Returns the ECDH shared secret on success and stores it on `self`.
    pub async fn pair_verify(&mut self) -> Result<[u8; 32]> {
        // Generate client keys.
        let client_ed = SigningKey::generate(&mut OsRng);
        let client_curve_secret = StaticSecret::random_from_rng(OsRng);
        let client_curve_pub = X25519PublicKey::from(&client_curve_secret);

        // Phase 1: flag=1 || 3 zero bytes || ecdh_pub || ed_pub
        let mut phase1 = Vec::with_capacity(68);
        phase1.extend_from_slice(&[1, 0, 0, 0]);
        phase1.extend_from_slice(client_curve_pub.as_bytes());
        phase1.extend_from_slice(client_ed.verifying_key().as_bytes());

        let resp1 = self
            .exchange("POST", "/pair-verify", &phase1)
            .await?;
        ensure_ok(&resp1)?;
        if resp1.body.len() < 96 {
            return Err(AirPlayError::Pairing(format!(
                "pair-verify phase1 response too short: {}",
                resp1.body.len()
            )));
        }

        let atv_curve_pub_bytes: [u8; 32] = resp1.body[..32]
            .try_into()
            .map_err(|_| AirPlayError::Pairing("atv curve pub".into()))?;
        let atv_curve_pub = X25519PublicKey::from(atv_curve_pub_bytes);
        let shared = client_curve_secret.diffie_hellman(&atv_curve_pub);
        let ecdh_secret = *shared.as_bytes();

        // AES-CTR key/IV from shared secret (Pair-Verify-AES-Key/IV).
        let mut hasher = Sha512::new();
        hasher.update(b"Pair-Verify-AES-Key");
        hasher.update(ecdh_secret);
        let aes_key: [u8; 16] = hasher.finalize()[..16]
            .try_into()
            .map_err(|_| AirPlayError::Crypto("AES key".into()))?;

        let mut hasher = Sha512::new();
        hasher.update(b"Pair-Verify-AES-IV");
        hasher.update(ecdh_secret);
        let aes_iv: [u8; 16] = hasher.finalize()[..16]
            .try_into()
            .map_err(|_| AirPlayError::Crypto("AES IV".into()))?;

        // Advance CTR with server's encrypted signature (bytes 32..96), then encrypt client sig.
        let mut cipher = Aes128Ctr::new((&aes_key).into(), (&aes_iv).into());
        let mut server_enc_sig = resp1.body[32..96].to_vec();
        cipher.apply_keystream(&mut server_enc_sig);

        let mut data_to_sign = [0u8; 64];
        data_to_sign[..32].copy_from_slice(client_curve_pub.as_bytes());
        data_to_sign[32..].copy_from_slice(&atv_curve_pub_bytes);
        let client_sig = client_ed.sign(&data_to_sign);
        let mut encrypted_client_sig = client_sig.to_bytes();
        cipher.apply_keystream(&mut encrypted_client_sig);

        // Phase 2: flag=0 || 3 zero bytes || encrypted signature
        let mut phase2 = Vec::with_capacity(68);
        phase2.extend_from_slice(&[0, 0, 0, 0]);
        phase2.extend_from_slice(&encrypted_client_sig);

        let resp2 = self
            .exchange("POST", "/pair-verify", &phase2)
            .await?;
        ensure_ok(&resp2)?;

        self.shared_secret = Some(ecdh_secret);
        info!("ControlClient: pair-verify completed");
        Ok(ecdh_secret)
    }

    /// Low-level RTSP exchange: send request, read one response.
    pub async fn exchange(
        &mut self,
        method: &str,
        path: &str,
        body: &[u8],
    ) -> Result<ControlResponse> {
        self.cseq = self.cseq.wrapping_add(1);
        let req = format_request(
            method,
            path,
            self.cseq,
            &self.session_id,
            &self.dacp_id,
            body,
        );
        debug!(
            method,
            path,
            cseq = self.cseq,
            body_len = body.len(),
            "ControlClient send"
        );
        self.stream
            .write_all(&req)
            .await
            .map_err(AirPlayError::Io)?;
        self.stream.flush().await.map_err(AirPlayError::Io)?;
        read_response(&mut self.stream, &mut self.pending)
            .await
            .map_err(AirPlayError::Io)
    }
}

fn ensure_ok(resp: &ControlResponse) -> Result<()> {
    if resp.status == 200 {
        Ok(())
    } else {
        Err(AirPlayError::Rtsp(format!(
            "unexpected status {} {}",
            resp.status, resp.reason
        )))
    }
}

/// Format an RTSP/1.0 request matching the Java client / server codec.
///
/// Pure function for unit testing (no network).
pub fn format_request(
    method: &str,
    path: &str,
    cseq: u32,
    session_id: &str,
    dacp_id: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 + body.len());
    out.extend_from_slice(format!("{method} {path} RTSP/1.0\r\n").as_bytes());
    out.extend_from_slice(format!("CSeq: {cseq}\r\n").as_bytes());
    out.extend_from_slice(format!("DACP-ID: {dacp_id}\r\n").as_bytes());
    out.extend_from_slice(format!("Active-Remote: {session_id}\r\n").as_bytes());
    out.extend_from_slice(format!("User-Agent: {USER_AGENT}\r\n").as_bytes());
    out.extend_from_slice(b"Connection: keep-alive\r\n");
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

/// Read one RTSP/HTTP-style response from `reader`, using `pending` as a leftover buffer.
pub async fn read_response<R: AsyncReadExt + Unpin>(
    reader: &mut R,
    pending: &mut Vec<u8>,
) -> io::Result<ControlResponse> {
    let mut tmp = [0u8; 2048];

    let header_end = loop {
        if let Some(pos) = find_header_end(pending) {
            break pos;
        }
        if pending.len() >= MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "control response headers too large",
            ));
        }
        let n = reader.read(&mut tmp).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "eof before end of response headers",
            ));
        }
        pending.extend_from_slice(&tmp[..n]);
    };

    let header_bytes = pending[..header_end].to_vec();
    let mut rest = pending.split_off(header_end + 4);
    pending.clear();

    let header_str = std::str::from_utf8(&header_bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut lines = header_str.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let mut parts = status_line.split_whitespace();
    let version = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing version"))?
        .to_string();
    let status: u16 = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing status"))?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let reason = parts.collect::<Vec<_>>().join(" ");

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
            "control response body too large",
        ));
    }

    while rest.len() < content_length {
        let need = content_length - rest.len();
        let mut chunk = vec![0u8; need.min(8192)];
        let n = reader.read(&mut chunk).await?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "eof before full response body",
            ));
        }
        rest.extend_from_slice(&chunk[..n]);
    }

    let body = rest[..content_length].to_vec();
    *pending = rest[content_length..].to_vec();

    Ok(ControlResponse {
        version,
        status,
        reason,
        headers,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tokio::io::BufReader;

    #[test]
    fn format_get_info_shape() {
        let bytes = format_request("GET", "/info", 1, "test-session", DEFAULT_DACP_ID, &[]);
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("GET /info RTSP/1.0\r\n"));
        assert!(s.contains("CSeq: 1\r\n"));
        assert!(s.contains("Active-Remote: test-session\r\n"));
        assert!(s.contains("DACP-ID: 184F380D0A5B7139\r\n"));
        assert!(s.contains("User-Agent: AirPlay/670.6.2\r\n"));
        assert!(s.contains("Connection: keep-alive\r\n"));
        assert!(s.contains("Content-Length: 0\r\n\r\n"));
        assert!(s.ends_with("\r\n\r\n"));
    }

    #[test]
    fn format_pair_setup_with_body() {
        let body = b"abcd";
        let bytes = format_request(
            "POST",
            "/pair-setup",
            2,
            "sess",
            "DACP",
            body,
        );
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("POST /pair-setup RTSP/1.0\r\n"));
        assert!(s.contains("CSeq: 2\r\n"));
        assert!(s.contains("Content-Length: 4\r\n\r\n"));
        assert!(bytes.ends_with(b"abcd"));
    }

    #[test]
    fn format_pair_verify_headers() {
        let payload = vec![1u8; 68];
        let bytes = format_request(
            "POST",
            "/pair-verify",
            3,
            DEFAULT_ACTIVE_REMOTE,
            DEFAULT_DACP_ID,
            &payload,
        );
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("POST /pair-verify RTSP/1.0\r\n"));
        assert!(s.contains("Content-Length: 68\r\n"));
        assert_eq!(&bytes[bytes.len() - 68..], payload.as_slice());
    }

    #[tokio::test]
    async fn parse_response_with_body() {
        let raw = b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: 4\r\n\r\nabcd";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let mut pending = Vec::new();
        let resp = read_response(&mut reader, &mut pending)
            .await
            .expect("read");
        assert_eq!(resp.version, "RTSP/1.0");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.reason, "OK");
        assert_eq!(resp.header("CSeq"), Some("1"));
        assert_eq!(resp.body, b"abcd");
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn parse_two_pipelined_responses() {
        let raw = b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: 0\r\n\r\n\
RTSP/1.0 200 OK\r\nCSeq: 2\r\nContent-Length: 2\r\n\r\nok";
        let mut reader = BufReader::new(Cursor::new(raw.as_slice()));
        let mut pending = Vec::new();
        let r1 = read_response(&mut reader, &mut pending).await.unwrap();
        assert_eq!(r1.status, 200);
        assert_eq!(r1.header("CSeq"), Some("1"));
        let r2 = read_response(&mut reader, &mut pending).await.unwrap();
        assert_eq!(r2.header("CSeq"), Some("2"));
        assert_eq!(r2.body, b"ok");
    }
}
