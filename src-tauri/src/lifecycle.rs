use crate::{change_enabled, AppState, Status};
use std::sync::atomic::Ordering;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

struct TrayItems {
    status: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
}

pub fn login_launch(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == "--autostart")
}

pub fn show(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide()?;
    }
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory)?;
    Ok(())
}

fn menu_state(status: &Status) -> (&'static str, &'static str, bool) {
    let label = match status.mapping_state {
        "active" => "Mappings: Active",
        "paused" => "Mappings: Paused",
        "default" => "Mappings: Default",
        _ => "Mappings: Unavailable",
    };
    (
        label,
        if status.paused {
            "Resume mappings"
        } else {
            "Pause mappings"
        },
        status.paused || !status.mappings.is_default(),
    )
}

pub fn sync_tray(app: &AppHandle) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let state = app.state::<AppState>();
    let status = state.shared.status();
    let (label, toggle, available) = menu_state(&status);
    let _ = items.status.set_text(label);
    let _ = items.toggle.set_text(toggle);
    let _ = items.toggle.set_enabled(available);
    if let Some(tray) = app.tray_by_id("qz-flow") {
        let _ = tray.set_tooltip(Some(format!("QZ Flow — {label}")));
    }
}

pub fn stop(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    stop_state(&state);
}

fn stop_state(state: &AppState) {
    state.shared.stopping.store(true, Ordering::Release);
    let mut hook = state
        .control
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    state
        .shared
        .engine
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .enabled = false;
    hook.take();
    #[cfg(windows)]
    if let Err(error) = crate::platform::release_injected() {
        state
            .shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .error = Some(error);
    }
    state
        .shared
        .engine
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .reset_pressed();
}

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open QZ Flow", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "Mappings: Paused", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Resume mappings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &status, &toggle, &quit])?;
    app.manage(TrayItems { status, toggle });
    let icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("Application icon".into()))?
        .clone();
    TrayIconBuilder::with_id("qz-flow")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show(app),
            "toggle" => {
                let state = app.state::<AppState>();
                let status = state.shared.status();
                if let Err(error) = change_enabled(status.paused, &state) {
                    state
                        .shared
                        .engine
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .error = Some(error);
                    show(app);
                }
                sync_tray(app);
            }
            "quit" => {
                stop(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        })
        .build(app)?;
    sync_tray(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{engine::Action, Shared};

    #[test]
    fn only_explicit_login_flag_starts_hidden() {
        assert!(!login_launch(["qz-flow".into()]));
        assert!(login_launch(["qz-flow".into(), "--autostart".into()]));
        assert!(!login_launch(["qz-flow--autostart".into()]));
    }

    #[test]
    fn tray_distinguishes_working_paused_and_unavailable_mappings() {
        let shared = Shared::default();
        let mut status = shared.status();
        assert_eq!(
            menu_state(&status),
            ("Mappings: Default", "Pause mappings", false)
        );
        status.mappings.back = Action::Copy;
        status.mapping_state = "unavailable";
        assert_eq!(
            menu_state(&status),
            ("Mappings: Unavailable", "Pause mappings", true)
        );
        status.paused = true;
        status.mapping_state = "paused";
        assert_eq!(
            menu_state(&status),
            ("Mappings: Paused", "Resume mappings", true)
        );
        status.paused = false;
        status.mapping_state = "active";
        assert_eq!(
            menu_state(&status),
            ("Mappings: Active", "Pause mappings", true)
        );
    }

    #[test]
    fn shutdown_is_idempotent_and_clears_mapping_state() {
        let shared = std::sync::Arc::new(Shared::default());
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.suppress_press(crate::Input::Middle);
        }
        let state = AppState {
            shared: shared.clone(),
            control: std::sync::Mutex::new(None),
            settings_path: "unused.json".into(),
        };
        stop_state(&state);
        stop_state(&state);
        assert!(shared.stopping.load(Ordering::Acquire));
        let engine = shared.engine.lock().unwrap();
        assert!(!engine.enabled);
        assert!(!engine.is_suppressed(crate::Input::Middle));
        assert!(state.control.lock().unwrap().is_none());
    }
}
