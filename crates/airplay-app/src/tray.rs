//! System tray (taskbar notification area) for airplay-app.
//!
//! Menu:
//! - Open config
//! - Open dump folder
//! - Open project / install folder
//! - Show status (About)
//! - Exit

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

/// Commands from the tray UI to the async main task.
#[derive(Debug, Clone)]
pub enum TrayCommand {
    Quit,
    OpenConfig,
    OpenDumpFolder,
    OpenInstallFolder,
    ShowStatus,
}

#[derive(Debug, Clone)]
pub struct TrayInfo {
    pub server_name: String,
    pub port: u16,
    pub player: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub config_path: PathBuf,
    pub dump_path: PathBuf,
}

enum UserEvent {
    Menu(MenuEvent),
    Tray(TrayIconEvent),
}

/// Spawn the tray on a background thread. Returns a channel of [`TrayCommand`]s.
pub fn spawn_tray(info: TrayInfo) -> Result<(Receiver<TrayCommand>, JoinHandle<()>), String> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<TrayCommand>();

    let handle = thread::Builder::new()
        .name("system-tray".into())
        .spawn(move || {
            if let Err(e) = run_tray_event_loop(info, cmd_tx) {
                tracing::error!(error = %e, "system tray exited with error");
            }
        })
        .map_err(|e| format!("spawn tray thread: {e}"))?;

    Ok((cmd_rx, handle))
}

fn run_tray_event_loop(info: TrayInfo, cmd_tx: Sender<TrayCommand>) -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("event loop: {e}"))?;

    let proxy = event_loop.create_proxy();
    let proxy_menu = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy_menu.send_event(UserEvent::Menu(event));
    }));
    let proxy_tray = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy_tray.send_event(UserEvent::Tray(event));
    }));

    let icon = load_tray_icon().map_err(|e| format!("tray icon: {e}"))?;

    let menu = Menu::new();
    let item_status = MenuItem::new("Status / About", true, None);
    let item_config = MenuItem::new("Open config.toml", true, None);
    let item_dump = MenuItem::new("Open dump folder", true, None);
    let item_folder = MenuItem::new("Open install folder", true, None);
    let item_exit = MenuItem::new("Exit", true, None);

    menu.append(&item_status)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_config)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_dump)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_folder)
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| format!("menu: {e}"))?;
    menu.append(&item_exit)
        .map_err(|e| format!("menu: {e}"))?;

    let tooltip = format!(
        "airplay2-rust\n{}\nport {} · {} · {}x{}@{}fps",
        info.server_name, info.port, info.player, info.width, info.height, info.fps
    );

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip)
        .with_icon(icon)
        .with_title("airplay2-rust")
        .build()
        .map_err(|e| format!("tray icon: {e}"))?;

    tracing::info!("system tray ready (right-click icon for menu)");

    let mut app = TrayApp {
        _tray: tray,
        cmd_tx,
        info,
        id_status: item_status.id().clone(),
        id_config: item_config.id().clone(),
        id_dump: item_dump.id().clone(),
        id_folder: item_folder.id().clone(),
        id_exit: item_exit.id().clone(),
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("tray event loop: {e}"))?;
    Ok(())
}

struct TrayApp {
    _tray: tray_icon::TrayIcon,
    cmd_tx: Sender<TrayCommand>,
    info: TrayInfo,
    id_status: tray_icon::menu::MenuId,
    id_config: tray_icon::menu::MenuId,
    id_dump: tray_icon::menu::MenuId,
    id_folder: tray_icon::menu::MenuId,
    id_exit: tray_icon::menu::MenuId,
}

impl ApplicationHandler<UserEvent> for TrayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Menu(ev) => {
                let id = ev.id;
                if id == self.id_exit {
                    let _ = self.cmd_tx.send(TrayCommand::Quit);
                    event_loop.exit();
                } else if id == self.id_config {
                    let _ = self.cmd_tx.send(TrayCommand::OpenConfig);
                } else if id == self.id_dump {
                    let _ = self.cmd_tx.send(TrayCommand::OpenDumpFolder);
                } else if id == self.id_folder {
                    let _ = self.cmd_tx.send(TrayCommand::OpenInstallFolder);
                } else if id == self.id_status {
                    let _ = self.cmd_tx.send(TrayCommand::ShowStatus);
                    // Also show immediately on tray thread (UI).
                    show_status_dialog(&self.info);
                }
            }
            UserEvent::Tray(ev) => {
                // Double-click opens status.
                if let TrayIconEvent::DoubleClick { .. } = ev {
                    show_status_dialog(&self.info);
                }
            }
        }
    }
}

fn load_tray_icon() -> Result<Icon, String> {
    // Prefer assets next to cwd / exe.
    let candidates = [
        PathBuf::from("assets/tray-icon.png"),
        PathBuf::from("assets/logo.png"),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("assets/tray-icon.png")))
            .unwrap_or_default(),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("tray-icon.png")))
            .unwrap_or_default(),
    ];

    for path in &candidates {
        if path.as_os_str().is_empty() || !path.is_file() {
            continue;
        }
        if let Ok(icon) = icon_from_png_path(path) {
            return Ok(icon);
        }
    }

    // Fallback: solid rust-orange circle-ish RGBA 32x32.
    Ok(fallback_icon())
}

fn icon_from_png_path(path: &Path) -> Result<Icon, String> {
    let img = image::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).map_err(|e| format!("icon rgba: {e}"))
}

fn fallback_icon() -> Icon {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let cx = (size as f32 - 1.0) / 2.0;
    let cy = cx;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let i = ((y * size + x) * 4) as usize;
            if dx * dx + dy * dy <= 14.0 * 14.0 {
                rgba[i] = 0xE8;
                rgba[i + 1] = 0x5D;
                rgba[i + 2] = 0x04;
                rgba[i + 3] = 0xFF;
            }
        }
    }
    Icon::from_rgba(rgba, size, size).expect("fallback icon")
}

fn show_status_dialog(info: &TrayInfo) {
    let msg = format!(
        "airplay2-rust\n\n\
         Receiver name: {}\n\
         Port: {}\n\
         Player: {}\n\
         Display: {}x{} @ {} fps (max)\n\
         Config: {}\n\
         Dump: {}\n\n\
         Right-click the tray icon for more tools.\n\
         Educational / research only — not affiliated with Apple.",
        info.server_name,
        info.port,
        info.player,
        info.width,
        info.height,
        info.fps,
        info.config_path.display(),
        info.dump_path.display(),
    );
    #[cfg(windows)]
    {
        message_box("airplay2-rust", &msg);
    }
    #[cfg(not(windows))]
    {
        tracing::info!("{msg}");
        let _ = open::that_detached(info.config_path.parent().unwrap_or(Path::new(".")));
    }
}

#[cfg(windows)]
fn message_box(title: &str, body: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HWND = *mut core::ffi::c_void;
    type UINT = u32;
    const MB_OK: UINT = 0x0000_0000;
    const MB_ICONINFORMATION: UINT = 0x0000_0040;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(h_wnd: HWND, lp_text: *const u16, lp_caption: *const u16, u_type: UINT)
            -> i32;
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    let t = wide(title);
    let b = wide(body);
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            b.as_ptr(),
            t.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

/// Handle tray commands on the async side (open paths, quit).
pub fn handle_tray_command(cmd: TrayCommand, info: &TrayInfo) {
    match cmd {
        TrayCommand::Quit => {
            // Caller exits the process.
        }
        TrayCommand::OpenConfig => {
            let path = &info.config_path;
            if path.is_file() {
                if let Err(e) = open::that(path) {
                    tracing::warn!(error = %e, path = %path.display(), "open config failed");
                }
            } else {
                tracing::warn!(path = %path.display(), "config file not found");
            }
        }
        TrayCommand::OpenDumpFolder => {
            let folder = info
                .dump_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let folder = if folder.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                folder
            };
            if let Err(e) = open::that(&folder) {
                tracing::warn!(error = %e, path = %folder.display(), "open dump folder failed");
            }
        }
        TrayCommand::OpenInstallFolder => {
            let folder = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."));
            if let Err(e) = open::that(&folder) {
                tracing::warn!(error = %e, path = %folder.display(), "open install folder failed");
            }
        }
        TrayCommand::ShowStatus => {
            // Dialog already shown on tray thread; log for console users.
            tracing::info!(
                name = %info.server_name,
                port = info.port,
                player = %info.player,
                "tray status"
            );
        }
    }
}
