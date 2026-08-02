//! Windows Core Audio endpoint volume control (IAudioEndpointVolume).
//!
//! All COM work runs on a dedicated worker thread so Tokio tasks never call
//! endpoint APIs directly. Failures are logged and never tear down the receiver.

use super::mapping::{airplay_db_is_mute, airplay_db_to_amplitude, clamp_db_to_range};
use super::SystemVolumeController;

use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

/// Commands for the COM volume worker.
enum VolumeCommand {
    SetDb(f64),
    SetMuted(bool),
    Shutdown,
}

/// Windows default-render endpoint volume controller.
pub struct WindowsSystemVolumeController {
    tx: Sender<VolumeCommand>,
    _join: Option<JoinHandle<()>>,
}

impl WindowsSystemVolumeController {
    pub fn new() -> Result<Self, String> {
        let (tx, rx) = mpsc::channel::<VolumeCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        let join = thread::Builder::new()
            .name("windows-system-volume".into())
            .spawn(move || {
                let result = worker_main(rx, |status| {
                    let _ = ready_tx.send(status);
                });
                if let Err(e) = result {
                    tracing::warn!(error = %e, "Windows volume worker exited with error");
                }
                tracing::info!("Windows volume worker task exited");
            })
            .map_err(|e| format!("spawn volume worker: {e}"))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx,
                _join: Some(join),
            }),
            Ok(Err(e)) => {
                let _ = tx.send(VolumeCommand::Shutdown);
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                let _ = join.join();
                Err("volume worker died before ready".into())
            }
        }
    }

    fn send(&self, cmd: VolumeCommand) -> Result<(), String> {
        self.tx
            .send(cmd)
            .map_err(|_| "Windows volume worker channel closed".to_string())
    }
}

impl SystemVolumeController for WindowsSystemVolumeController {
    fn set_airplay_volume_db(&self, db: f64) -> Result<(), String> {
        tracing::info!(db, source = "controller", "AirPlay volume update");
        self.send(VolumeCommand::SetDb(db))
    }

    fn set_muted(&self, muted: bool) -> Result<(), String> {
        tracing::info!(muted, source = "controller", "AirPlay mute update");
        self.send(VolumeCommand::SetMuted(muted))
    }
}

impl Drop for WindowsSystemVolumeController {
    fn drop(&mut self) {
        let _ = self.tx.send(VolumeCommand::Shutdown);
        if let Some(j) = self._join.take() {
            let _ = j.join();
        }
    }
}

fn worker_main(
    rx: mpsc::Receiver<VolumeCommand>,
    ready: impl FnOnce(Result<(), String>),
) -> Result<(), String> {
    // SAFETY: COM init on this dedicated thread only.
    let hr = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_MULTITHREADED) };
    if hr < 0 && hr != RPC_E_CHANGED_MODE {
        ready(Err(format!("CoInitializeEx failed: HRESULT 0x{hr:08X}")));
        return Err(format!("CoInitializeEx failed: 0x{hr:08X}"));
    }
    let _com = ComGuard;

    let mut endpoint = match EndpointVolume::open_default() {
        Ok(e) => {
            ready(Ok(()));
            Some(e)
        }
        Err(e) => {
            ready(Err(e.clone()));
            return Err(e);
        }
    };

    let mut last_db = 0.0_f64;

    while let Ok(cmd) = rx.recv() {
        match cmd {
            VolumeCommand::Shutdown => break,
            VolumeCommand::SetDb(db) => {
                last_db = db;
                if let Some(ep) = endpoint.as_mut() {
                    if let Err(e) = ep.apply_airplay_db(db) {
                        tracing::warn!(error = %e, "apply volume failed; reacquiring endpoint");
                        endpoint = EndpointVolume::open_default().ok();
                        if let Some(ep) = endpoint.as_mut() {
                            if let Err(e2) = ep.apply_airplay_db(db) {
                                tracing::warn!(error = %e2, "volume apply failed after reacquire");
                            }
                        }
                    }
                } else if let Ok(mut ep) = EndpointVolume::open_default() {
                    let _ = ep.apply_airplay_db(db);
                    endpoint = Some(ep);
                }
            }
            VolumeCommand::SetMuted(muted) => {
                if let Some(ep) = endpoint.as_mut() {
                    if let Err(e) = ep.set_mute(muted) {
                        tracing::warn!(error = %e, "set mute failed; reacquiring endpoint");
                        endpoint = EndpointVolume::open_default().ok();
                        if let Some(ep) = endpoint.as_mut() {
                            let _ = ep.set_mute(muted);
                            if !muted {
                                let _ = ep.apply_airplay_db(last_db);
                            }
                        }
                    } else if !muted {
                        // Restoring unmute: re-apply last level.
                        let _ = ep.apply_airplay_db(last_db);
                    }
                }
            }
        }
    }
    Ok(())
}

struct ComGuard;
impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

struct EndpointVolume {
    /// IAudioEndpointVolume*
    ptr: *mut std::ffi::c_void,
    min_db: f32,
    max_db: f32,
    /// Whether SetMasterVolumeLevel (dB) is preferred.
    use_db: bool,
}

// SAFETY: EndpointVolume is only used on the COM worker thread.
unsafe impl Send for EndpointVolume {}

impl EndpointVolume {
    fn open_default() -> Result<Self, String> {
        unsafe {
            let mut enumerator: *mut std::ffi::c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_MMDEVICE_ENUMERATOR,
                std::ptr::null_mut(),
                CLSCTX_ALL,
                &IID_IMMDEVICE_ENUMERATOR,
                &mut enumerator,
            );
            if hr < 0 || enumerator.is_null() {
                return Err(format!("CoCreateInstance MMDeviceEnumerator: 0x{hr:08X}"));
            }

            let enum_vtbl = *(enumerator as *mut *mut ImmDeviceEnumeratorVtbl);
            let mut device: *mut std::ffi::c_void = std::ptr::null_mut();
            let hr = ((*enum_vtbl).get_default_audio_endpoint)(
                enumerator,
                E_RENDER,
                E_MULTIMEDIA,
                &mut device,
            );
            if hr < 0 || device.is_null() {
                ((*enum_vtbl).release)(enumerator);
                return Err(format!("GetDefaultAudioEndpoint: 0x{hr:08X}"));
            }

            let dev_vtbl = *(device as *mut *mut ImmDeviceVtbl);
            let mut endpoint: *mut std::ffi::c_void = std::ptr::null_mut();
            let hr = ((*dev_vtbl).activate)(
                device,
                &IID_IAUDIO_ENDPOINT_VOLUME,
                CLSCTX_ALL,
                std::ptr::null_mut(),
                &mut endpoint,
            );
            ((*dev_vtbl).release)(device);
            ((*enum_vtbl).release)(enumerator);

            if hr < 0 || endpoint.is_null() {
                return Err(format!("Activate IAudioEndpointVolume: 0x{hr:08X}"));
            }

            let vol_vtbl = *(endpoint as *mut *mut IAudioEndpointVolumeVtbl);
            let mut min_db = 0.0f32;
            let mut max_db = 0.0f32;
            let mut step = 0.0f32;
            let hr = ((*vol_vtbl).get_volume_range)(endpoint, &mut min_db, &mut max_db, &mut step);
            let use_db = hr >= 0 && min_db < max_db;
            if !use_db {
                tracing::warn!(
                    hr = format_args!("0x{hr:08X}"),
                    "GetVolumeRange failed; will use scalar fallback"
                );
                min_db = -96.0;
                max_db = 0.0;
            } else {
                tracing::info!(
                    min_db,
                    max_db,
                    step_db = step,
                    "Windows audio endpoint volume range"
                );
            }

            Ok(Self {
                ptr: endpoint,
                min_db,
                max_db,
                use_db,
            })
        }
    }

    fn apply_airplay_db(&mut self, db: f64) -> Result<(), String> {
        if airplay_db_is_mute(db) {
            self.set_master_scalar(0.0)?;
            self.set_mute(true)?;
            tracing::info!(db, "Windows audio endpoint muted");
            return Ok(());
        }

        self.set_mute(false)?;
        tracing::info!("Windows audio endpoint unmuted");

        if self.use_db {
            let target = clamp_db_to_range(db, self.min_db as f64, self.max_db as f64) as f32;
            self.set_master_db(target)?;
            if (target - 0.0).abs() < 0.01 {
                tracing::info!(db = target, "Windows master volume set to maximum");
            } else {
                tracing::info!(db = target, "Windows master volume set");
            }
        } else {
            let scalar = airplay_db_to_amplitude(db) as f32;
            self.set_master_scalar(scalar)?;
            tracing::info!(
                db,
                scalar,
                "Windows master volume set (scalar fallback)"
            );
        }
        Ok(())
    }

    fn set_master_db(&self, db: f32) -> Result<(), String> {
        unsafe {
            let vtbl = *(self.ptr as *mut *mut IAudioEndpointVolumeVtbl);
            let hr = ((*vtbl).set_master_volume_level)(self.ptr, db, std::ptr::null());
            if hr < 0 {
                return Err(format!("SetMasterVolumeLevel: 0x{hr:08X}"));
            }
        }
        Ok(())
    }

    fn set_master_scalar(&self, scalar: f32) -> Result<(), String> {
        let s = scalar.clamp(0.0, 1.0);
        unsafe {
            let vtbl = *(self.ptr as *mut *mut IAudioEndpointVolumeVtbl);
            let hr = ((*vtbl).set_master_volume_level_scalar)(self.ptr, s, std::ptr::null());
            if hr < 0 {
                return Err(format!("SetMasterVolumeLevelScalar: 0x{hr:08X}"));
            }
        }
        Ok(())
    }

    fn set_mute(&self, muted: bool) -> Result<(), String> {
        unsafe {
            let vtbl = *(self.ptr as *mut *mut IAudioEndpointVolumeVtbl);
            let hr = ((*vtbl).set_mute)(self.ptr, i32::from(muted), std::ptr::null());
            if hr < 0 {
                return Err(format!("SetMute: 0x{hr:08X}"));
            }
        }
        Ok(())
    }
}

impl Drop for EndpointVolume {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                let vtbl = *(self.ptr as *mut *mut IAudioEndpointVolumeVtbl);
                ((*vtbl).release)(self.ptr);
            }
            self.ptr = std::ptr::null_mut();
        }
    }
}

// ---- Minimal COM / Core Audio FFI (no extra crates) ----

const COINIT_MULTITHREADED: u32 = 0x0;
const RPC_E_CHANGED_MODE: i32 = -2147417850; // 0x80010106 as i32
const CLSCTX_ALL: u32 = 0x17;
const E_RENDER: u32 = 0; // eRender
const E_MULTIMEDIA: u32 = 1; // eMultimedia

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

// CLSID_MMDeviceEnumerator = bcde0395-e52f-467c-8e3d-c4579291692e
const CLSID_MMDEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xbcde_0395,
    data2: 0xe52f,
    data3: 0x467c,
    data4: [0x8e, 0x3d, 0xc4, 0x57, 0x92, 0x91, 0x69, 0x2e],
};

// IID_IMMDeviceEnumerator = a95664d2-9614-4f35-a746-de8db63617e6
const IID_IMMDEVICE_ENUMERATOR: Guid = Guid {
    data1: 0xa956_64d2,
    data2: 0x9614,
    data3: 0x4f35,
    data4: [0xa7, 0x46, 0xde, 0x8d, 0xb6, 0x36, 0x17, 0xe6],
};

// IID_IAudioEndpointVolume = 5CDF2C82-841E-4546-9722-0CF74078229A
const IID_IAUDIO_ENDPOINT_VOLUME: Guid = Guid {
    data1: 0x5cdf_2c82,
    data2: 0x841e,
    data3: 0x4546,
    data4: [0x97, 0x22, 0x0c, 0xf7, 0x40, 0x78, 0x22, 0x9a],
};

#[repr(C)]
struct ImmDeviceEnumeratorVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    enum_audio_endpoints: usize,
    get_default_audio_endpoint: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        u32,
        u32,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    // remaining methods unused
}

#[repr(C)]
struct ImmDeviceVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    activate: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const Guid,
        u32,
        *mut std::ffi::c_void,
        *mut *mut std::ffi::c_void,
    ) -> i32,
}

#[repr(C)]
struct IAudioEndpointVolumeVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    register_control_change_notify: usize,
    unregister_control_change_notify: usize,
    get_channel_count: usize,
    set_master_volume_level: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        f32,
        *const Guid,
    ) -> i32,
    set_master_volume_level_scalar: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        f32,
        *const Guid,
    ) -> i32,
    get_master_volume_level: usize,
    get_master_volume_level_scalar: usize,
    set_channel_volume_level: usize,
    set_channel_volume_level_scalar: usize,
    get_channel_volume_level: usize,
    get_channel_volume_level_scalar: usize,
    set_mute: unsafe extern "system" fn(*mut std::ffi::c_void, i32, *const Guid) -> i32,
    get_mute: usize,
    get_volume_step_info: usize,
    volume_step_up: usize,
    volume_step_down: usize,
    query_hardware_support: usize,
    get_volume_range: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *mut f32,
        *mut f32,
        *mut f32,
    ) -> i32,
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pvreserved: *mut std::ffi::c_void, dwcoinit: u32) -> i32;
    fn CoUninitialize();
    fn CoCreateInstance(
        rclsid: *const Guid,
        punkouter: *mut std::ffi::c_void,
        dwclscontext: u32,
        riid: *const Guid,
        ppv: *mut *mut std::ffi::c_void,
    ) -> i32;
}
