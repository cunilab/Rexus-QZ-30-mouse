#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod platform;

use engine::{Action, Engine, Input, Mappings};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tauri::Manager;

#[derive(Clone, Debug, Serialize)]
pub struct Device {
    vendor_id: u16,
    product_id: u16,
    name: String,
    transport: String,
}

#[derive(Default)]
struct Inventory {
    devices: Vec<Device>,
    error: Option<String>,
}

#[derive(Default)]
pub struct Shared {
    engine: Mutex<Engine>,
    inventory: Mutex<Inventory>,
    hook_running: AtomicBool,
    stopping: AtomicBool,
}

impl Shared {
    fn refresh(&self) {
        let result = platform::devices();
        let mut inventory = self.inventory.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(devices) => {
                inventory.devices = devices;
                inventory.error = None;
            }
            Err(error) => {
                inventory.devices.clear();
                inventory.error = Some(error);
            }
        }
        if inventory.devices.is_empty() {
            self.engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .enabled = false;
        }
    }

    fn process(&self, input: Input, down: bool, perform: fn(Action) -> Result<(), String>) -> bool {
        // This lock never spans filesystem work, inventory calls, or UI operations.
        let mut engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        if !down {
            engine.action(input, false);
            return engine.suppress_release(input);
        }
        if engine.is_suppressed(input) {
            return true;
        }
        let action = engine.action(input, true);
        if action == Action::Default {
            return false;
        }
        match perform(action) {
            Ok(()) => {
                engine.suppress_press(input);
                engine.error = None;
                true
            }
            Err(error) => {
                engine.error = Some(error);
                false
            }
        }
    }

    fn status(&self) -> Status {
        let inventory = self.inventory.lock().unwrap_or_else(|e| e.into_inner());
        let engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        Status {
            platform: std::env::consts::OS,
            devices: inventory.devices.clone(),
            enabled: engine.enabled,
            permission: platform::permission(),
            hook_running: self.hook_running.load(Ordering::Acquire),
            mappings: engine.mappings.clone(),
            last_input: engine.last_input.clone(),
            error: inventory.error.clone().or_else(|| engine.error.clone()),
            thumb_available: cfg!(windows),
        }
    }
}

#[derive(Serialize)]
struct Status {
    platform: &'static str,
    devices: Vec<Device>,
    enabled: bool,
    permission: &'static str,
    hook_running: bool,
    mappings: Mappings,
    last_input: Option<String>,
    error: Option<String>,
    thumb_available: bool,
}

struct AppState {
    shared: Arc<Shared>,
    control: Mutex<Option<platform::Hook>>,
    settings_path: PathBuf,
}

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> Status {
    state.shared.status()
}

#[tauri::command]
fn save_mappings(mappings: Mappings, state: tauri::State<AppState>) -> Result<Status, String> {
    if !cfg!(windows) && mappings.thumb != Action::Default {
        return Err(
            "Thumb-rest input has not been identified yet; keep button 4 at default.".into(),
        );
    }
    let _control = state
        .control
        .lock()
        .map_err(|_| "App control is unavailable. Restart QZ Flow.")?;
    if state
        .shared
        .engine
        .lock()
        .map_err(|_| "Mapping state is unavailable.")?
        .enabled
    {
        return Err("Turn off remapping before changing assignments.".into());
    }
    save_settings(&state.settings_path, &mappings)?;
    let mut engine = state
        .shared
        .engine
        .lock()
        .map_err(|_| "Mapping state is unavailable.")?;
    engine.mappings = mappings;
    engine.error = None;
    drop(engine);
    Ok(state.shared.status())
}

#[tauri::command]
fn set_enabled(enabled: bool, state: tauri::State<AppState>) -> Result<Status, String> {
    let mut hook = state
        .control
        .lock()
        .map_err(|_| "App control is unavailable. Restart QZ Flow.")?;
    if !enabled {
        state
            .shared
            .engine
            .lock()
            .map_err(|_| "Mapping state is unavailable.")?
            .enabled = false;
        return Ok(state.shared.status());
    }
    platform::ensure_no_vendor_utility()?;
    state.shared.refresh();
    if state
        .shared
        .inventory
        .lock()
        .map_err(|_| "Device inventory is unavailable.")?
        .devices
        .is_empty()
    {
        return Err("Connect the compatible USB receiver first. Bluetooth identification is not verified yet.".into());
    }
    if state
        .shared
        .engine
        .lock()
        .map_err(|_| "Mapping state is unavailable.")?
        .mappings
        .is_default()
    {
        return Err("Save at least one button assignment first.".into());
    }
    if !state.shared.hook_running.load(Ordering::Acquire) {
        hook.take();
        state
            .shared
            .engine
            .lock()
            .map_err(|_| "Mapping state is unavailable.")?
            .reset_pressed();
        *hook = Some(platform::start(state.shared.clone())?);
    }
    {
        let inventory = state
            .shared
            .inventory
            .lock()
            .map_err(|_| "Device inventory is unavailable.")?;
        if inventory.devices.is_empty() || !state.shared.hook_running.load(Ordering::Acquire) {
            return Err("The receiver or input listener disconnected. Try enabling again.".into());
        }
        state
            .shared
            .engine
            .lock()
            .map_err(|_| "Mapping state is unavailable.")?
            .enabled = true;
    }
    Ok(state.shared.status())
}

#[tauri::command]
fn open_input_settings() -> Result<(), String> {
    platform::open_input_settings()
}

fn save_settings(path: &Path, mappings: &Mappings) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid settings directory.")?;
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create settings directory: {e}"))?;
    let bytes = serde_json::to_vec_pretty(mappings).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!(".mappings-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| format!("Cannot stage settings: {e}"))?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Settings were not saved: {e}"))
}

fn main() {
    if std::env::args().any(|arg| arg == "--trace-buttons") {
        #[cfg(windows)]
        if let Err(error) = platform::trace_buttons() {
            eprintln!("Input check failed: {error}");
            std::process::exit(1);
        }
        #[cfg(not(windows))]
        eprintln!("The vendor button input check currently requires Windows.");
        return;
    }
    if std::env::args().any(|arg| arg == "--diagnose" || arg == "--check-hook") {
        let shared = Arc::new(Shared::default());
        shared.refresh();
        let _hook = if std::env::args().any(|arg| arg == "--check-hook") {
            match platform::start(shared.clone()) {
                Ok(hook) => Some(hook),
                Err(error) => {
                    eprintln!("Hook check failed: {error}");
                    std::process::exit(1);
                }
            }
        } else {
            None
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&shared.status()).unwrap()
        );
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("mappings.json");
            let shared = Arc::new(Shared::default());
            match fs::read(&settings_path) {
                Ok(bytes) => match serde_json::from_slice::<Mappings>(&bytes) {
                    Ok(mappings) => shared.engine.lock().unwrap().mappings = mappings,
                    Err(error) => shared.engine.lock().unwrap().error = Some(format!("Saved mappings could not be read; defaults are active. Your file was retained: {error}")),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => shared.engine.lock().unwrap().error = Some(format!("Saved mappings could not be opened: {error}")),
            }
            shared.refresh();
            app.manage(AppState { shared: shared.clone(), control: Mutex::new(None), settings_path });
            thread::spawn(move || {
                while !shared.stopping.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_secs(2));
                    shared.refresh();
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_status, save_mappings, set_enabled, open_input_settings])
        .build(tauri::generate_context!())
        .expect("Could not start QZ Flow")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                state.shared.stopping.store(true, Ordering::Release);
                state.shared.engine.lock().unwrap_or_else(|e| e.into_inner()).enabled = false;
                state.control.lock().unwrap_or_else(|e| e.into_inner()).take();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saving_twice_replaces_valid_settings_without_leaving_staging_file() {
        let dir = std::env::temp_dir().join(format!("qz-flow-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mappings.json");
        let mut mappings = Mappings::default();
        save_settings(&path, &mappings).unwrap();
        mappings.back = Action::Copy;
        save_settings(&path, &mappings).unwrap();
        assert_eq!(
            serde_json::from_slice::<Mappings>(&fs::read(&path).unwrap()).unwrap(),
            mappings
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn input_is_consumed_only_after_shortcut_succeeds() {
        let shared = Shared::default();
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        assert!(!shared.process(Input::Back, true, |_| Err("denied".into())));
        assert!(!shared.process(Input::Back, false, |_| Ok(())));
        assert!(shared.process(Input::Back, true, |_| Ok(())));
        assert!(shared.process(Input::Back, true, |_| panic!(
            "Duplicate down must not inject twice"
        )));
        shared.engine.lock().unwrap().enabled = false;
        assert!(shared.process(Input::Back, false, |_| Ok(())));
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.suppress_press(Input::Middle);
            engine.reset_pressed();
            assert!(!engine.is_suppressed(Input::Middle));
        }
    }

    #[test]
    fn failed_staging_keeps_the_previous_settings_file() {
        let dir = std::env::temp_dir().join(format!("qz-flow-staging-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mappings.json");
        let original = b"previous settings";
        fs::write(&path, original).unwrap();
        let staging = dir.join(format!(".mappings-{}.tmp", std::process::id()));
        fs::write(&staging, b"existing staged data").unwrap();
        assert!(save_settings(&path, &Mappings::default()).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read(&staging).unwrap(), b"existing staged data");
        fs::remove_file(staging).unwrap();
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
