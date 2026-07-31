//! Per-client AirPlay session and session table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use airplay_lib::AirPlay;
use tokio::task::AbortHandle;

/// Active AirPlay session keyed by `Active-Remote` / session id.
///
/// `airplay` is shared with media tasks via `Arc<Mutex<_>>` so control-plane
/// pair/setup and data-plane decrypt can run concurrently without holding the
/// session-map lock across I/O.
pub struct Session {
    pub id: String,
    pub airplay: Arc<Mutex<AirPlay>>,
    /// Bound video data port (ephemeral), if SETUP video ran.
    pub video_port: Option<u16>,
    /// Bound audio data port (ephemeral), if SETUP audio ran.
    pub audio_port: Option<u16>,
    /// Bound audio control port (ephemeral), if SETUP audio ran.
    pub audio_control_port: Option<u16>,
    /// Media-server abort handles (abort on TEARDOWN).
    pub video_task: Option<AbortHandle>,
    pub audio_task: Option<AbortHandle>,
    pub audio_control_task: Option<AbortHandle>,
}

impl Session {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            airplay: Arc::new(Mutex::new(AirPlay::new())),
            video_port: None,
            audio_port: None,
            audio_control_port: None,
            video_task: None,
            audio_task: None,
            audio_control_task: None,
        }
    }

    /// Abort media accept/recv tasks and clear ports.
    pub fn stop_media(&mut self) {
        if let Some(h) = self.video_task.take() {
            h.abort();
        }
        if let Some(h) = self.audio_task.take() {
            h.abort();
        }
        if let Some(h) = self.audio_control_task.take() {
            h.abort();
        }
        self.video_port = None;
        self.audio_port = None;
        self.audio_control_port = None;
    }
}

/// Manages concurrent sessions with interior mutability.
///
/// `AirPlay` is not `Sync` by itself; each session wraps it in `Arc<Mutex<_>>`
/// so media tasks can lock decrypt independently of the session map.
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Session>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Run `f` with a mutable reference to the session for `id`, creating it if absent.
    pub fn with_session<R>(&self, id: &str, f: impl FnOnce(&mut Session) -> R) -> R {
        let mut map = self.sessions.lock().expect("session map lock poisoned");
        let session = map
            .entry(id.to_string())
            .or_insert_with(|| Session::new(id));
        f(session)
    }

    /// Immutable-style callback if the session exists.
    pub fn get_with<R>(&self, id: &str, f: impl FnOnce(&Session) -> R) -> Option<R> {
        let map = self.sessions.lock().expect("session map lock poisoned");
        map.get(id).map(f)
    }

    /// Mutable callback if the session exists (does not create).
    pub fn get_mut_with<R>(&self, id: &str, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
        let mut map = self.sessions.lock().expect("session map lock poisoned");
        map.get_mut(id).map(f)
    }

    /// Whether a session with this id is present.
    pub fn contains(&self, id: &str) -> bool {
        let map = self.sessions.lock().expect("session map lock poisoned");
        map.contains_key(id)
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        let map = self.sessions.lock().expect("session map lock poisoned");
        map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remove and drop a session if present.
    pub fn remove(&self, id: &str) {
        let mut map = self.sessions.lock().expect("session map lock poisoned");
        if let Some(mut s) = map.remove(id) {
            s.stop_media();
        }
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_session_reuses_same_airplay_instance() {
        let mgr = SessionManager::new();
        let session_id = "active-remote-1";

        // First access: create session and run pair_setup.
        let pk1 = mgr.with_session(session_id, |s| {
            assert_eq!(s.id, session_id);
            s.airplay.lock().expect("lock").pair_setup()
        });

        // Second access: same session / AirPlay identity key.
        let pk2 = mgr.with_session(session_id, |s| s.airplay.lock().expect("lock").pair_setup());
        assert_eq!(pk1, pk2);
        assert_eq!(mgr.len(), 1);

        let verified = mgr.with_session(session_id, |s| {
            s.airplay.lock().expect("lock").is_pair_verified()
        });
        assert!(!verified);

        // Different id creates a second session.
        mgr.with_session("other", |_| {});
        assert_eq!(mgr.len(), 2);
        assert!(mgr.contains(session_id));
        assert!(mgr.contains("other"));

        mgr.remove(session_id);
        assert!(!mgr.contains(session_id));
        assert_eq!(mgr.len(), 1);
        assert!(mgr.get_with(session_id, |_| ()).is_none());
    }

    #[test]
    fn get_mut_with_does_not_create() {
        let mgr = SessionManager::new();
        assert!(mgr.get_mut_with("missing", |_| ()).is_none());
        assert!(mgr.is_empty());
    }
}
