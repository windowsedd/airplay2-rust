//! Top-level AirPlay receiver server (control TCP + Bonjour).

use std::sync::Arc;

use airplay_lib::{AirPlayBonjour, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::AirPlayConfig;
use crate::consumer::AirPlayConsumer;
use crate::control::{read_request, write_response, ControlHandler};
use crate::session::SessionManager;

/// AirPlay receiver: control server + mDNS advertisement.
pub struct AirPlayServer {
    config: AirPlayConfig,
    consumer: Arc<dyn AirPlayConsumer>,
    sessions: Arc<SessionManager>,
    port: u16,
    bonjour: Option<AirPlayBonjour>,
    shutdown_tx: Option<watch::Sender<bool>>,
    accept_task: Option<JoinHandle<()>>,
}

impl AirPlayServer {
    pub fn new(config: AirPlayConfig, consumer: Arc<dyn AirPlayConsumer>) -> Self {
        Self {
            config,
            consumer,
            sessions: Arc::new(SessionManager::new()),
            port: 0,
            bonjour: None,
            shutdown_tx: None,
            accept_task: None,
        }
    }

    /// Bind `0.0.0.0:0`, spawn accept loop, start Bonjour on the bound port.
    pub async fn start(&mut self) -> Result<()> {
        if self.accept_task.is_some() {
            return Err(airplay_lib::AirPlayError::InvalidState(
                "AirPlayServer already started".into(),
            ));
        }

        let listener = TcpListener::bind("0.0.0.0:0").await?;
        let port = listener.local_addr()?.port();
        self.port = port;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let sessions = Arc::clone(&self.sessions);
        let config = self.config.clone();
        let consumer = Arc::clone(&self.consumer);

        let accept_task = tokio::spawn(async move {
            control_accept_loop(listener, sessions, config, consumer, port, shutdown_rx).await;
        });
        self.accept_task = Some(accept_task);

        let mut bonjour = AirPlayBonjour::new(self.config.server_name.clone());
        bonjour.start(port)?;
        self.bonjour = Some(bonjour);

        info!(port, "AirPlay control server listening");
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stop Bonjour and the accept loop (idempotent).
    pub async fn stop(&mut self) {
        if let Some(bonjour) = self.bonjour.as_mut() {
            bonjour.stop();
        }
        self.bonjour = None;

        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }

        if let Some(handle) = self.accept_task.take() {
            // Abort if the loop doesn't exit promptly.
            handle.abort();
            let _ = handle.await;
        }

        // Tear down any placeholder media tasks.
        // SessionManager has no clear-all; leave sessions until drop.
        info!("AirPlay control server stopped");
    }
}

impl Drop for AirPlayServer {
    fn drop(&mut self) {
        if let Some(bonjour) = self.bonjour.as_mut() {
            bonjour.stop();
        }
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.accept_task.take() {
            handle.abort();
        }
    }
}

async fn control_accept_loop(
    listener: TcpListener,
    sessions: Arc<SessionManager>,
    config: AirPlayConfig,
    consumer: Arc<dyn AirPlayConsumer>,
    control_port: u16,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer)) => {
                        info!(%peer, "control connection");
                        let sessions = Arc::clone(&sessions);
                        let config = config.clone();
                        let consumer = Arc::clone(&consumer);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(
                                stream,
                                sessions,
                                config,
                                consumer,
                                control_port,
                            )
                            .await
                            {
                                debug_connection_err(e);
                            }
                        });
                    }
                    Err(e) => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                        error!("control accept error: {e}");
                    }
                }
            }
        }
    }
}

fn debug_connection_err(e: std::io::Error) {
    match e.kind() {
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset => {
            // normal client disconnect
        }
        _ => warn!("control connection error: {e}"),
    }
}

async fn handle_connection(
    stream: TcpStream,
    sessions: Arc<SessionManager>,
    config: AirPlayConfig,
    consumer: Arc<dyn AirPlayConsumer>,
    control_port: u16,
) -> std::io::Result<()> {
    let handler = ControlHandler::new(sessions, config, consumer, control_port);
    let (mut reader, mut writer) = stream.into_split();
    let mut pending = Vec::new();

    loop {
        let req = match read_request(&mut reader, &mut pending).await? {
            Some(r) => r,
            None => break,
        };
        let response = handler.handle(&req).await;
        write_response(&mut writer, &response).await?;
    }
    Ok(())
}
