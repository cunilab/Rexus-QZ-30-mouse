use crate::{
    engine::Mappings,
    platform,
    settings::{self, Settings},
    AppState,
};
use std::sync::atomic::Ordering;

fn validate(mappings: &Mappings) -> Result<(), String> {
    if !cfg!(windows) && mappings.thumb != crate::Action::Default {
        return Err(
            "Thumb-rest input has not been identified yet; keep button 4 at default.".into(),
        );
    }
    Ok(())
}

fn prepare(
    state: &AppState,
    hook: &mut Option<platform::Hook>,
    mappings: &Mappings,
) -> Result<(), String> {
    if state.shared.stopping.load(Ordering::Acquire) {
        return Err("QZ Flow is shutting down.".into());
    }
    if mappings.is_default() {
        return Ok(());
    }
    let result = (|| {
        validate(mappings)?;
        platform::ensure_no_vendor_utility()?;
        state.shared.refresh();
        let inventory = state
            .shared
            .inventory
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if inventory.devices.is_empty() {
            return Err(inventory.error.clone().unwrap_or_else(|| "Connect the compatible USB receiver. Bluetooth identification is not verified yet.".into()));
        }
        drop(inventory);
        if platform::permission() != "granted" {
            return Err(
                "Input permission is required. Open Input settings, then save again.".into(),
            );
        }
        if !state.shared.hook_running.load(Ordering::Acquire) {
            hook.take();
            state
                .shared
                .engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .reset_pressed();
            *hook = Some(platform::start(state.shared.clone())?);
        }
        if !state.shared.hook_running.load(Ordering::Acquire) {
            return Err(
                "The input listener is unavailable. QZ Flow will retry automatically.".into(),
            );
        }
        Ok(())
    })();
    if let Err(error) = &result {
        let mut engine = state
            .shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        engine.enabled = false;
        engine.error = Some(error.clone());
    }
    result
}

// Persistence succeeds before new assignments become visible to the input callback.
fn apply(
    state: &AppState,
    mappings: Mappings,
    prepare: impl FnOnce(&Mappings) -> Result<(), String>,
    persist: impl FnOnce(&Settings) -> Result<(), String>,
) -> Result<(), String> {
    prepare(&mappings)?;
    persist(&Settings::new(mappings.clone(), false))?;
    let mut engine = state
        .shared
        .engine
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    engine.enabled = !mappings.is_default()
        && state.shared.hook_running.load(Ordering::Acquire)
        && !state.shared.stopping.load(Ordering::Acquire);
    engine.mappings = mappings;
    engine.paused = false;
    engine.error = if !engine.mappings.is_default() && !engine.enabled {
        Some("Assignments saved; waiting for the input listener to reconnect.".into())
    } else {
        None
    };
    Ok(())
}

pub fn save_and_apply(state: &AppState, mappings: Mappings) -> Result<(), String> {
    validate(&mappings)?;
    let mut hook = state
        .control
        .lock()
        .map_err(|_| "App control is unavailable. Restart QZ Flow.")?;
    apply(
        state,
        mappings,
        |mappings| prepare(state, &mut hook, mappings),
        |settings| settings::save(&state.settings_path, settings),
    )
}

pub fn change_enabled(state: &AppState, enabled: bool) -> Result<(), String> {
    let mut hook = state
        .control
        .lock()
        .map_err(|_| "App control is unavailable. Restart QZ Flow.")?;
    let mappings = {
        let mut engine = state
            .shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !enabled {
            engine.enabled = false;
            engine.paused = true;
        }
        engine.mappings.clone()
    };
    if enabled {
        apply(
            state,
            mappings,
            |mappings| prepare(state, &mut hook, mappings),
            |settings| settings::save(&state.settings_path, settings),
        )
    } else {
        settings::save(&state.settings_path, &Settings::new(mappings, true)).map_err(|error| {
            format!("Paused for this session; the pause could not be saved: {error}")
        })
    }
}

pub fn reconcile(state: &AppState) {
    let Ok(mut hook) = state.control.lock() else {
        return;
    };
    if state.shared.stopping.load(Ordering::Acquire) {
        return;
    }
    state.shared.refresh();
    let mappings = {
        let mut engine = state
            .shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if engine.paused || engine.mappings.is_default() {
            engine.enabled = false;
            return;
        }
        engine.mappings.clone()
    };
    if prepare(state, &mut hook, &mappings).is_ok() {
        let mut engine = state
            .shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        engine.enabled = !state.shared.stopping.load(Ordering::Acquire)
            && state.shared.hook_running.load(Ordering::Acquire);
        engine.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{Action, Input},
        Shared,
    };
    use std::sync::{Arc, Mutex};
    fn state() -> AppState {
        AppState {
            shared: Arc::new(Shared::default()),
            control: Mutex::new(None),
            settings_path: "unused.json".into(),
        }
    }
    #[test]
    fn failed_preparation_or_save_retains_assignments_and_pause() {
        let state = state();
        state.shared.engine.lock().unwrap().paused = true;
        let mappings = Mappings {
            back: Action::Copy,
            ..Mappings::default()
        };
        assert!(apply(
            &state,
            mappings.clone(),
            |_| Err("receiver".into()),
            |_| panic!("must not save")
        )
        .is_err());
        assert!(apply(&state, mappings, |_| Ok(()), |_| Err("disk".into())).is_err());
        let engine = state.shared.engine.lock().unwrap();
        assert!(engine.mappings.is_default());
        assert!(engine.paused);
        assert!(!engine.enabled);
    }
    #[test]
    fn failed_write_keeps_previous_working_mapping_active() {
        let state = state();
        {
            let mut engine = state.shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        let mappings = Mappings {
            back: Action::Paste,
            ..Mappings::default()
        };
        assert!(apply(&state, mappings, |_| Ok(()), |_| Err("disk full".into())).is_err());
        let engine = state.shared.engine.lock().unwrap();
        assert!(engine.enabled);
        assert!(!engine.paused);
        assert_eq!(engine.mappings.back, Action::Copy);
    }

    #[test]
    fn save_commits_after_persistence_and_preserves_consumed_release() {
        let state = state();
        state.shared.hook_running.store(true, Ordering::Release);
        state
            .shared
            .engine
            .lock()
            .unwrap()
            .suppress_press(Input::Back);
        let mappings = Mappings {
            back: Action::Copy,
            ..Mappings::default()
        };
        apply(
            &state,
            mappings.clone(),
            |_| Ok(()),
            |settings| {
                assert!(state.shared.engine.lock().unwrap().mappings.is_default());
                assert_eq!(settings.mappings, mappings);
                assert!(!settings.paused);
                Ok(())
            },
        )
        .unwrap();
        let mut engine = state.shared.engine.lock().unwrap();
        assert!(engine.enabled);
        assert_eq!(engine.mappings.back, Action::Copy);
        assert!(engine.suppress_release(Input::Back));
    }
    #[test]
    fn listener_loss_during_persistence_is_saved_but_never_reported_active() {
        let state = state();
        state.shared.hook_running.store(true, Ordering::Release);
        let mappings = Mappings {
            back: Action::Copy,
            ..Mappings::default()
        };
        apply(
            &state,
            mappings.clone(),
            |_| Ok(()),
            |_| {
                state.shared.hook_running.store(false, Ordering::Release);
                Ok(())
            },
        )
        .unwrap();
        let status = state.shared.status();
        assert_eq!(status.mappings, mappings);
        assert!(!status.enabled);
        assert_eq!(status.mapping_state, "unavailable");
        assert!(status.error.unwrap().contains("saved"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unsupported_saved_thumb_mapping_reports_reason_before_native_calls() {
        let state = state();
        let mappings = Mappings {
            thumb: Action::Copy,
            ..Mappings::default()
        };
        assert!(prepare(&state, &mut None, &mappings).is_err());
        assert!(state
            .shared
            .engine
            .lock()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("Thumb-rest"));
        assert!(!state.shared.engine.lock().unwrap().enabled);
    }

    #[test]
    fn defaults_require_no_receiver_and_paused_reconnect_stays_paused() {
        let state = state();
        prepare(&state, &mut None, &Mappings::default()).unwrap();
        {
            let mut engine = state.shared.engine.lock().unwrap();
            engine.paused = true;
            engine.mappings.back = Action::Copy;
        }
        reconcile(&state);
        assert!(state.shared.engine.lock().unwrap().paused);
        assert!(!state.shared.engine.lock().unwrap().enabled);
        assert!(state.control.lock().unwrap().is_none());
    }
    #[test]
    fn failed_pause_persistence_still_stops_session() {
        let mut state = state();
        state.settings_path = std::env::temp_dir(); // A directory cannot be replaced by settings.
        state.shared.engine.lock().unwrap().enabled = true;
        assert!(change_enabled(&state, false)
            .unwrap_err()
            .contains("Paused for this session"));
        assert!(state.shared.engine.lock().unwrap().paused);
        assert!(!state.shared.engine.lock().unwrap().enabled);
    }
}
