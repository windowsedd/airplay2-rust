//! Per-client AirPlay session and session table.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use airplay_lib::AirPlay;
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::consumer::StreamGeneration;
use crate::control::OutboundRequest;

/// Registered reverse-channel writer for one purpose (e.g. `event`).
#[derive(Clone)]
pub struct ReverseWriter {
    pub generation: u64,
    pub tx: mpsc::Sender<OutboundRequest>,
}

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
    /// Generation assigned at the last video SETUP (for owner-checked TEARDOWN).
    pub video_generation: Option<StreamGeneration>,
    /// Generation assigned at the last audio SETUP.
    pub audio_generation: Option<StreamGeneration>,
    /// Media-server abort handles (abort on TEARDOWN).
    pub video_task: Option<AbortHandle>,
    pub audio_task: Option<AbortHandle>,
    pub audio_control_task: Option<AbortHandle>,
    /// Reverse HTTP writers keyed by `X-Apple-Purpose`.
    pub reverse_writers: HashMap<String, ReverseWriter>,
    /// Pending playlist proxy waiters keyed by remote `mlhls://localhost/...` URL.
    pub pending_playlists: HashMap<String, oneshot::Sender<Vec<u8>>>,
    next_reverse_generation: u64,
}

impl Session {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            airplay: Arc::new(Mutex::new(AirPlay::new())),
            video_port: None,
            audio_port: None,
            audio_control_port: None,
            video_generation: None,
            audio_generation: None,
            video_task: None,
            audio_task: None,
            audio_control_task: None,
            reverse_writers: HashMap::new(),
            pending_playlists: HashMap::new(),
            next_reverse_generation: 1,
        }
    }

    /// Abort media accept/recv tasks and clear ports.
    ///
    /// Returns the video/audio generations that were active so the caller can
    /// issue owner-checked consumer disconnects.
    pub fn stop_media(&mut self) -> (Option<StreamGeneration>, Option<StreamGeneration>) {
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
        let video_gen = self.video_generation.take();
        let audio_gen = self.audio_generation.take();
        (video_gen, audio_gen)
    }

    /// Cancel pending media playlist waiters (does not stop RTSP A/V tasks).
    pub fn cancel_media(&mut self) {
        self.pending_playlists.clear();
    }
}

/// Manages concurrent sessions with interior mutability.
///
/// `AirPlay` is not `Sync` by itself; each session wraps it in `Arc<Mutex<_>>`
/// so media tasks can lock decrypt independently of the session map.
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Session>>,
    /// Process-wide monotonic counter for stream generations.
    next_generation: AtomicU64,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
        }
    }

    /// Allocate a unique stream generation id (never zero).
    pub fn next_generation(&self) -> StreamGeneration {
        let gen = self.next_generation.fetch_add(1, Ordering::Relaxed);
        if gen == 0 {
            self.next_generation.fetch_add(1, Ordering::Relaxed)
        } else {
            gen
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

    /// Register a reverse writer. Returns the generation for later safe removal.
    pub fn register_reverse(
        &self,
        session_id: &str,
        purpose: &str,
        tx: mpsc::Sender<OutboundRequest>,
    ) -> u64 {
        self.with_session(session_id, |s| {
            let generation = s.next_reverse_generation;
            s.next_reverse_generation = s.next_reverse_generation.wrapping_add(1).max(1);
            s.reverse_writers.insert(
                purpose.to_string(),
                ReverseWriter {
                    generation,
                    tx,
                },
            );
            generation
        })
    }

    pub fn reverse_sender(
        &self,
        session_id: &str,
        purpose: &str,
    ) -> Option<mpsc::Sender<OutboundRequest>> {
        self.get_with(session_id, |s| {
            s.reverse_writers.get(purpose).map(|w| w.tx.clone())
        })
        .flatten()
    }

    /// Remove reverse writer only if `generation` still owns the slot.
    pub fn remove_reverse_if_generation(&self, session_id: &str, purpose: &str, generation: u64) {
        let _ = self.get_mut_with(session_id, |s| {
            if s.reverse_writers
                .get(purpose)
                .is_some_and(|w| w.generation == generation)
            {
                s.reverse_writers.remove(purpose);
            }
        });
    }

    /// Register a oneshot waiter for a remote playlist URL.
    ///
    /// Returns `None` if a waiter for this URL is already pending.
    pub fn register_playlist(
        &self,
        session_id: &str,
        remote_url: &str,
    ) -> Option<oneshot::Receiver<Vec<u8>>> {
        self.with_session(session_id, |s| {
            if s.pending_playlists.contains_key(remote_url) {
                return None;
            }
            let (tx, rx) = oneshot::channel();
            s.pending_playlists.insert(remote_url.to_string(), tx);
            Some(rx)
        })
    }

    /// Fulfill a pending playlist waiter. Returns false if none matched.
    pub fn fulfill_playlist(&self, session_id: &str, remote_url: &str, data: Vec<u8>) -> bool {
        let sender = self
            .get_mut_with(session_id, |s| s.pending_playlists.remove(remote_url))
            .flatten();
        match sender {
            Some(tx) => tx.send(data).is_ok(),
            None => false,
        }
    }

    pub fn remove_pending_playlist(&self, session_id: &str, remote_url: &str) {
        let _ = self.get_mut_with(session_id, |s| {
            s.pending_playlists.remove(remote_url);
        });
    }

    /// Cancel all pending playlist waiters for the session.
    pub fn cancel_media(&self, session_id: &str) {
        let _ = self.get_mut_with(session_id, |s| s.cancel_media());
    }

    /// Remove and drop a session if present.
    ///
    /// Returns active video/audio generations so the caller can disconnect the
    /// consumer with owner checks.
    pub fn remove(&self, id: &str) -> (Option<StreamGeneration>, Option<StreamGeneration>) {
        let mut map = self.sessions.lock().expect("session map lock poisoned");
        if let Some(mut s) = map.remove(id) {
            s.cancel_media();
            s.stop_media()
        } else {
            (None, None)
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

        let pk1 = mgr.with_session(session_id, |s| {
            assert_eq!(s.id, session_id);
            s.airplay.lock().expect("lock").pair_setup()
        });

        let pk2 = mgr.with_session(session_id, |s| s.airplay.lock().expect("lock").pair_setup());
        assert_eq!(pk1, pk2);
        assert_eq!(mgr.len(), 1);

        let verified = mgr.with_session(session_id, |s| {
            s.airplay.lock().expect("lock").is_pair_verified()
        });
        assert!(!verified);

        mgr.with_session("other", |_| {});
        assert_eq!(mgr.len(), 2);
        assert!(mgr.contains(session_id));
        assert!(mgr.contains("other"));

        let _ = mgr.remove(session_id);
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

    #[test]
    fn generations_are_unique_and_non_zero() {
        let mgr = SessionManager::new();
        let a = mgr.next_generation();
        let b = mgr.next_generation();
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn stop_media_returns_generations() {
        let mut s = Session::new("s");
        s.video_generation = Some(3);
        s.audio_generation = Some(4);
        let (v, a) = s.stop_media();
        assert_eq!(v, Some(3));
        assert_eq!(a, Some(4));
        assert!(s.video_generation.is_none());
        assert!(s.audio_generation.is_none());
    }

    #[tokio::test]
    async fn replacing_reverse_writer_does_not_let_old_disconnect_remove_new_writer() {
        let sessions = SessionManager::new();
        let (old_tx, _) = mpsc::channel(2);
        let old_generation = sessions.register_reverse("s1", "event", old_tx);
        let (new_tx, _) = mpsc::channel(2);
        let new_generation = sessions.register_reverse("s1", "event", new_tx.clone());
        sessions.remove_reverse_if_generation("s1", "event", old_generation);
        assert!(sessions.reverse_sender("s1", "event").is_some());
        sessions.remove_reverse_if_generation("s1", "event", new_generation);
        assert!(sessions.reverse_sender("s1", "event").is_none());
    }

    #[tokio::test]
    async fn pending_playlist_is_one_shot_and_cancelled_with_media() {
        let sessions = SessionManager::new();
        let rx = sessions
            .register_playlist("s1", "mlhls://localhost/master.m3u8")
            .unwrap();
        assert!(sessions.fulfill_playlist(
            "s1",
            "mlhls://localhost/master.m3u8",
            b"ok".to_vec()
        ));
        assert_eq!(rx.await.unwrap(), b"ok");
        assert!(!sessions.fulfill_playlist(
            "s1",
            "mlhls://localhost/master.m3u8",
            b"late".to_vec()
        ));

        let rx2 = sessions
            .register_playlist("s1", "mlhls://localhost/other.m3u8")
            .unwrap();
        sessions.cancel_media("s1");
        assert!(rx2.await.is_err());
    }

    #[test]
    fn duplicate_pending_playlist_rejected() {
        let sessions = SessionManager::new();
        assert!(sessions
            .register_playlist("s1", "mlhls://localhost/a")
            .is_some());
        assert!(sessions
            .register_playlist("s1", "mlhls://localhost/a")
            .is_none());
    }
}
