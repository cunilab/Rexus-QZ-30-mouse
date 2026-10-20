#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod lifecycle;
mod platform;
mod recording;
mod runtime;
mod settings;
mod startup;

use engine::{Action, Engine, Input, Mappings};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
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
    recording: AtomicBool,
    processing: AtomicBool,
    dispatch_releases: AtomicU8,
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
        let action = {
            let mut engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
            if self.stopping.load(Ordering::Acquire) {
                engine.enabled = false;
            }
            if !down {
                if self.processing.load(Ordering::Acquire) {
                    self.dispatch_releases
                        .fetch_or(input.button_bit(), Ordering::AcqRel);
                }
                engine.action(input, false);
                return engine.suppress_release(input);
            }
            if engine.is_suppressed(input) {
                return true;
            }
            if self.recording.load(Ordering::Acquire) {
                engine.action(input, false);
                return false;
            }
            let action = engine.action(input, true);
            if action == Action::Default {
                return false;
            }
            if engine.navigation_throttled(input, &action, std::time::Instant::now()) {
                return true;
            }
            if self
                .processing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return false;
            }
            self.dispatch_releases.store(0, Ordering::Release);
            engine.error = None;
            action
        };
        struct Dispatch<'a>(&'a AtomicBool);
        impl Drop for Dispatch<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _dispatch = Dispatch(&self.processing);
        // Native input can pump messages; neither UI status nor reentrant input may wait on this call.
        let result = perform(action);
        let mut engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(()) => {
                if self.dispatch_releases.load(Ordering::Acquire) & input.button_bit() == 0 {
                    engine.suppress_press(input);
                }
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
        let hook_running = self.hook_running.load(Ordering::Acquire);
        let enabled = engine.enabled
            && hook_running
            && !engine.paused
            && !self.stopping.load(Ordering::Acquire);
        let mapping_state = if engine.paused {
            "paused"
        } else if enabled {
            "active"
        } else if engine.mappings.is_default() && engine.error.is_none() {
            "default"
        } else {
            "unavailable"
        };
        Status {
            paused: engine.paused,
            mapping_state,
            platform: std::env::consts::OS,
            devices: inventory.devices.clone(),
            enabled,
            permission: platform::permission(),
            hook_running,
            mappings: engine.mappings.clone(),
            last_input: engine.last_input.clone(),
            error: inventory.error.clone().or_else(|| engine.error.clone()).or_else(|| {
                (mapping_state == "unavailable").then(|| "Mappings are waiting for the receiver and input listener. QZ Flow will retry automatically.".into())
            }),
            thumb_available: cfg!(windows),
            launch_at_login: None,
        }
    }
}

#[derive(Serialize)]
struct Status {
    paused: bool,
    mapping_state: &'static str,
    platform: &'static str,
    devices: Vec<Device>,
    enabled: bool,
    permission: &'static str,
    hook_running: bool,
    mappings: Mappings,
    last_input: Option<String>,
    error: Option<String>,
    thumb_available: bool,
    launch_at_login: Option<bool>,
}

struct AppState {
    shared: Arc<Shared>,
    control: Mutex<Option<platform::Hook>>,
    settings_path: PathBuf,
}

#[tauri::command]
fn get_status(app: tauri::AppHandle, state: tauri::State<AppState>) -> Status {
    app_status(&app, &state)
}

fn app_status(app: &tauri::AppHandle, state: &AppState) -> Status {
    let mut status = state.shared.status();
    match startup::is_enabled(app) {
        Ok(enabled) => status.launch_at_login = Some(enabled),
        Err(error) => {
            status
                .error
                .get_or_insert_with(|| format!("Could not read launch-at-login settings: {error}"));
        }
    }
    status
}

#[tauri::command]
fn set_launch_at_login(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<Status, String> {
    let _control = state
        .control
        .lock()
        .map_err(|_| "App control is unavailable. Restart QZ Flow.")?;
    startup::set_enabled(&app, enabled)
        .map_err(|error| format!("Could not change launch-at-login settings: {error}"))?;
    let status = app_status(&app, &state);
    if status.launch_at_login != Some(enabled) {
        return Err("The operating system did not confirm the login setting. Try again.".into());
    }
    Ok(status)
}

#[tauri::command]
fn save_mappings(
    mappings: Mappings,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<Status, String> {
    runtime::save_and_apply(&state, mappings)?;
    lifecycle::sync_tray(&app);
    Ok(app_status(&app, &state))
}

fn change_enabled(enabled: bool, state: &AppState) -> Result<(), String> {
    runtime::change_enabled(state, enabled)
}

#[tauri::command]
fn open_input_settings() -> Result<(), String> {
    platform::open_input_settings()
}

#[tauri::command]
async fn record_shortcut(app: tauri::AppHandle) -> Result<Option<Action>, String> {
    let shared = app.state::<AppState>().shared.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if shared
            .recording
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A shortcut recording is already running.".into());
        }
        struct Session(Arc<Shared>);
        impl Drop for Session {
            fn drop(&mut self) {
                self.0.recording.store(false, Ordering::Release);
            }
        }
        let _session = Session(shared);
        let action = recording::record(&app)?;
        if let Some(action) = &action {
            action.validate()?;
            platform::validate_action(action)?;
        }
        Ok(action)
    })
    .await
    .map_err(|error| format!("Shortcut recorder failed: {error}"))?
}

#[tauri::command]
async fn pick_application() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(platform::pick_application)
        .await
        .map_err(|error| format!("Application picker failed: {error}"))?
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
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !lifecycle::login_launch(args) {
                lifecycle::show(app);
            }
        }))
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("mappings.json");
            let shared = Arc::new(Shared::default());
            match settings::load(&settings_path) {
                Ok(settings) => {
                    let mut engine = shared.engine.lock().unwrap();
                    engine.mappings = settings.mappings;
                    engine.paused = settings.paused;
                }
                Err(error) => shared.engine.lock().unwrap().error = Some(format!("Saved mappings could not be read; defaults are active. Your file was retained: {error}")),
            }
            shared.refresh();
            app.manage(AppState { shared: shared.clone(), control: Mutex::new(None), settings_path });
            runtime::reconcile(&app.state::<AppState>());
            lifecycle::install(app.handle())?;
            if !lifecycle::login_launch(std::env::args()) {
                lifecycle::show(app.handle());
            } else {
                lifecycle::hide(app.handle())?;
            }
            let handle = app.handle().clone();
            thread::spawn(move || {
                while !shared.stopping.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_secs(2));
                    if shared.stopping.load(Ordering::Acquire) { break; }
                    runtime::reconcile(&handle.state::<AppState>());
                    lifecycle::sync_tray(&handle);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Err(error) = lifecycle::hide(window.app_handle()) {
                    let state = window.state::<AppState>();
                    state.shared.engine.lock().unwrap_or_else(|e| e.into_inner()).error = Some(format!("Could not hide QZ Flow: {error}"));
                    lifecycle::show(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![get_status, save_mappings, open_input_settings, set_launch_at_login, record_shortcut, pick_application])
        .build(tauri::generate_context!())
        .expect("Could not start QZ Flow")
        .run(|app, event| {
            match event {
                tauri::RunEvent::Exit => lifecycle::stop(app),
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => lifecycle::show(app),
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_never_reports_dead_listener_as_active_and_retains_explicit_pause() {
        let shared = Shared::default();
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        let status = shared.status();
        assert!(!status.enabled);
        assert_eq!(status.mapping_state, "unavailable");
        assert!(status.error.is_some());
        shared.hook_running.store(true, Ordering::Release);
        assert_eq!(shared.status().mapping_state, "active");
        shared.engine.lock().unwrap().paused = true;
        let status = shared.status();
        assert!(!status.enabled);
        assert!(status.paused);
        assert_eq!(status.mapping_state, "paused");
    }

    #[test]
    fn recording_bypasses_mapped_actions_but_retains_consumed_releases() {
        let shared = Shared::default();
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
            engine.suppress_press(Input::Middle);
        }
        shared.recording.store(true, Ordering::Release);
        assert!(!shared.process(Input::Back, true, |_| panic!(
            "must not inject during recording"
        )));
        assert!(shared.process(Input::Middle, false, |_| Ok(())));
        shared.recording.store(false, Ordering::Release);
        assert!(shared.process(Input::Back, true, |_| Ok(())));
    }

    thread_local! {
        static DISPATCH_TEST: std::cell::RefCell<Option<Arc<Shared>>> = const { std::cell::RefCell::new(None) };
    }

    #[test]
    fn native_dispatch_can_read_status_and_reenter_without_holding_engine_lock() {
        let shared = Arc::new(Shared::default());
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        DISPATCH_TEST.with(|slot| *slot.borrow_mut() = Some(shared.clone()));
        assert!(shared.process(Input::Back, true, |_| {
            DISPATCH_TEST.with(|slot| {
                let slot = slot.borrow();
                let shared = slot.as_ref().unwrap();
                assert!(
                    shared.engine.try_lock().is_ok(),
                    "Native dispatch must release the engine lock"
                );
                let _status = shared.status();
                assert!(!shared.process(Input::Back, true, |_| panic!(
                    "Reentrant input must not inject"
                )));
                shared.engine.lock().unwrap().enabled = false;
            });
            Ok(())
        }));
        assert!(!shared.processing.load(Ordering::Acquire));
        assert!(shared.process(Input::Back, false, |_| panic!("Release must not inject")));
        DISPATCH_TEST.with(|slot| slot.borrow_mut().take());
    }

    #[test]
    fn release_during_native_dispatch_cannot_leave_suppression_for_the_next_press() {
        let shared = Arc::new(Shared::default());
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::Copy;
        }
        DISPATCH_TEST.with(|slot| *slot.borrow_mut() = Some(shared.clone()));
        assert!(shared.process(Input::Thumb, true, |_| {
            DISPATCH_TEST.with(|slot| {
                assert!(!slot
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .process(Input::Thumb, false, |_| panic!("Release must not inject")));
            });
            Ok(())
        }));
        assert!(!shared.engine.lock().unwrap().is_suppressed(Input::Thumb));
        fn next_press(_: Action) -> Result<(), String> {
            Ok(())
        }
        assert!(shared.process(Input::Thumb, true, next_press));
        assert!(shared.engine.lock().unwrap().is_suppressed(Input::Thumb));
        assert!(shared.process(Input::Thumb, false, next_press));
        DISPATCH_TEST.with(|slot| slot.borrow_mut().take());
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
}
