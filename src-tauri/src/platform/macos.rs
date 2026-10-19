use crate::{
    engine::{Action, Input},
    Device, Shared,
};
use std::{
    ffi::{c_char, c_void, CString},
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, OnceLock,
    },
    thread::{self, JoinHandle},
};

type Ref = *const c_void;
type Event = *mut c_void;
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
const UTF8: u32 = 0x08000100;
const COMMAND: u64 = 1 << 20;
const SHIFT: u64 = 1 << 17;
const CONTROL: u64 = 1 << 18;
const OPTION: u64 = 1 << 19;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Ref);
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
    fn CFStringGetCString(value: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> bool;
    fn CFNumberGetValue(value: Ref, kind: isize, result: *mut c_void) -> bool;
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFMachPortInvalidate(port: Ref);
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(loop_ref: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    static kCFRunLoopCommonModes: Ref;
    static kCFRunLoopDefaultMode: Ref;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(port: u32, matching: *mut c_void, iterator: *mut u32) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IORegistryEntryCreateCFProperty(entry: u32, key: Ref, allocator: Ref, options: u32) -> Ref;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Event, u32, Event, *mut c_void) -> Event,
        data: *mut c_void,
    ) -> Ref;
    fn CGPreflightPostEventAccess() -> bool;
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Event;
    fn CGEventSetFlags(event: Event, flags: u64);
    fn CGEventPost(tap: u32, event: Event);
}

unsafe fn property(service: u32, key: &str) -> Ref {
    let key = CString::new(key).unwrap();
    let cf_key = CFStringCreateWithCString(null(), key.as_ptr(), UTF8);
    if cf_key.is_null() {
        return null();
    }
    let value = IORegistryEntryCreateCFProperty(service, cf_key, null(), 0);
    CFRelease(cf_key);
    value
}

unsafe fn number(service: u32, key: &str) -> Option<u16> {
    let value = property(service, key);
    if value.is_null() {
        return None;
    }
    let mut result = 0i32;
    let valid = CFGetTypeID(value) == CFNumberGetTypeID()
        && CFNumberGetValue(value, 3, (&mut result as *mut i32).cast());
    CFRelease(value);
    if valid {
        u16::try_from(result).ok()
    } else {
        None
    }
}

unsafe fn string(service: u32, key: &str) -> Option<String> {
    let value = property(service, key);
    if value.is_null() {
        return None;
    }
    let mut bytes = [0u8; 512];
    let valid = CFGetTypeID(value) == CFStringGetTypeID()
        && CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, UTF8);
    CFRelease(value);
    if !valid {
        return None;
    }
    Some(
        String::from_utf8_lossy(
            &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())],
        )
        .into_owned(),
    )
}

pub fn devices() -> Result<Vec<Device>, String> {
    unsafe {
        let matching = IOServiceMatching(c"IOHIDDevice".as_ptr());
        if matching.is_null() {
            return Err("macOS could not create a device inventory.".into());
        }
        let mut iterator = 0;
        let code = IOServiceGetMatchingServices(0, matching, &mut iterator);
        if code != 0 {
            return Err(format!("macOS device inventory failed ({code})."));
        }
        let mut found = Vec::new();
        loop {
            let device = IOIteratorNext(iterator);
            if device == 0 {
                break;
            }
            if number(device, "VendorID") == Some(0x276d)
                && number(device, "ProductID") == Some(0x1902)
                && found.is_empty()
            {
                found.push(Device {
                    vendor_id: 0x276d,
                    product_id: 0x1902,
                    name: string(device, "Product")
                        .unwrap_or_else(|| "QZ30-compatible USB receiver".into()),
                    transport: string(device, "Transport")
                        .unwrap_or_else(|| "USB / 2.4 GHz receiver".into()),
                });
            }
            IOObjectRelease(device);
        }
        IOObjectRelease(iterator);
        Ok(found)
    }
}

pub fn permission() -> &'static str {
    if unsafe {
        AXIsProcessTrusted() && CGPreflightListenEventAccess() && CGPreflightPostEventAccess()
    } {
        "granted"
    } else {
        "required"
    }
}

pub fn ensure_no_vendor_utility() -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "Mouse Setting"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Could not check the original utility: {e}"))?;
    match status.code() {
        Some(0) => Err("Quit the original Mouse Setting utility before enabling QZ Flow.".into()),
        Some(1) => Ok(()),
        _ => Err("Could not check for the original mouse utility. Try again.".into()),
    }
}

pub fn open_input_settings() -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map_err(|e| e.to_string())?
        .success()
        .then_some(())
        .ok_or_else(|| "Could not open macOS permission settings.".into())
}

pub struct Hook {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start(shared: Arc<Shared>) -> Result<Hook, String> {
    if permission() != "granted" {
        return Err("Allow QZ Flow in System Settings > Privacy & Security > Accessibility and Input Monitoring, then save again.".into());
    }
    let _ = SHARED.set(shared.clone());
    let (sender, receiver) = mpsc::sync_channel(1);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let join = thread::spawn(move || unsafe {
        let mask = (1 << 22) | (1 << 25) | (1 << 26);
        let tap = CGEventTapCreate(1, 0, 0, mask, callback, null_mut());
        if tap.is_null() {
            let _ = sender.send(Err("macOS denied mouse access. Allow Accessibility and Input Monitoring for QZ Flow, then restart it.".to_string()));
            return;
        }
        let source = CFMachPortCreateRunLoopSource(null(), tap, 0);
        if source.is_null() {
            CFMachPortInvalidate(tap);
            CFRelease(tap);
            let _ = sender.send(Err(
                "Could not create the macOS mouse event source.".to_string()
            ));
            return;
        }
        let loop_ref = CFRunLoopGetCurrent();
        CFRunLoopAddSource(loop_ref, source, kCFRunLoopCommonModes);
        shared.hook_running.store(true, Ordering::Release);
        if sender.send(Ok(())).is_ok() {
            while !thread_stop.load(Ordering::Acquire) {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, false);
            }
        }
        shared.hook_running.store(false, Ordering::Release);
        shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .enabled = false;
        CFMachPortInvalidate(tap);
        CFRelease(source);
        CFRelease(tap);
    });
    match receiver.recv() {
        Ok(Ok(())) => Ok(Hook {
            stop,
            join: Some(join),
        }),
        Ok(Err(error)) => {
            let _ = join.join();
            Err(error)
        }
        Err(error) => {
            let _ = join.join();
            Err(error.to_string())
        }
    }
}

unsafe extern "C" fn callback(_proxy: Event, kind: u32, event: Event, _data: *mut c_void) -> Event {
    let Some(shared) = SHARED.get() else {
        return event;
    };
    if kind == u32::MAX || kind == u32::MAX - 1 {
        shared.hook_running.store(false, Ordering::Release);
        let mut engine = shared.engine.lock().unwrap_or_else(|e| e.into_inner());
        engine.enabled = false;
        engine.error =
            Some("macOS paused input access. Enable remapping again to reconnect.".into());
        return event;
    }
    if event.is_null() {
        return event;
    }
    let input = match kind {
        25 | 26 => match CGEventGetIntegerValueField(event, 3) {
            2 => Some((Input::Middle, kind == 25)),
            3 => Some((Input::Back, kind == 25)),
            4 => Some((Input::Forward, kind == 25)),
            _ => None,
        },
        // Preserve diagonal/vertical trackpad scrolling instead of discarding an entire event.
        22 if CGEventGetIntegerValueField(event, 11) == 0 => {
            match CGEventGetIntegerValueField(event, 12) {
                value if value > 0 => Some((Input::WheelLeft, true)),
                value if value < 0 => Some((Input::WheelRight, true)),
                _ => None,
            }
        }
        _ => None,
    };
    if let Some((input, down)) = input {
        if shared.process(input, down, perform) {
            return null_mut();
        }
    }
    event
}

fn shortcut(action: &Action) -> Result<(u16, u64), String> {
    Ok(match action {
        Action::Copy => (8, COMMAND),
        Action::Cut => (7, COMMAND),
        Action::Paste => (9, COMMAND),
        Action::Undo => (6, COMMAND),
        Action::Redo => (6, COMMAND | SHIFT),
        Action::SelectAll => (0, COMMAND),
        Action::Desktop => (103, 0), // F11 follows the person's Mission Control shortcut settings.
        Action::BrowserBack => (33, COMMAND),
        Action::BrowserForward => (30, COMMAND),
        Action::TaskView => (126, CONTROL),
        Action::PreviousDesktop => (123, CONTROL),
        Action::NextDesktop => (124, CONTROL),
        Action::SwitchApps => (48, COMMAND),
        Action::Minimize => (46, COMMAND),
        Action::Maximize => (3, CONTROL | COMMAND), // macOS full screen.
        Action::PreviousTab => (48, CONTROL | SHIFT),
        Action::NextTab => (48, CONTROL),
        Action::NewTab => (17, COMMAND),
        Action::CloseTab => (13, COMMAND),
        Action::ReopenTab => (17, COMMAND | SHIFT),
        Action::Refresh => (15, COMMAND),
        Action::ZoomIn => (24, COMMAND),
        Action::ZoomOut => (27, COMMAND),
        Action::ZoomReset => (29, COMMAND),
        Action::Screenshot => (20, COMMAND | SHIFT),
        Action::Default | Action::Disable => (0, 0),
        Action::CustomShortcut { key, modifiers } => {
            let mut flags = 0;
            for modifier in modifiers {
                let flag = match modifier.as_str() {
                    "control" => CONTROL,
                    "alt" => OPTION,
                    "shift" => SHIFT,
                    "meta" => COMMAND,
                    _ => return Err(format!("Unknown shortcut modifier: {modifier}")),
                };
                if flags & flag != 0 {
                    return Err("Shortcut modifiers must be unique.".into());
                }
                flags |= flag;
            }
            (
                custom_key(key)
                    .ok_or_else(|| format!("Unsupported shortcut key on macOS: {key}"))?,
                flags,
            )
        }
        Action::VolumeUp
        | Action::VolumeDown
        | Action::Mute
        | Action::PlayPause
        | Action::PreviousTrack
        | Action::NextTrack
        | Action::LaunchApplication { .. } => {
            return Err("This action does not use a keyboard shortcut.".into());
        }
    })
}

fn custom_key(key: &str) -> Option<u16> {
    Some(match key {
        "KeyA" => 0,
        "KeyS" => 1,
        "KeyD" => 2,
        "KeyF" => 3,
        "KeyH" => 4,
        "KeyG" => 5,
        "KeyZ" => 6,
        "KeyX" => 7,
        "KeyC" => 8,
        "KeyV" => 9,
        "KeyB" => 11,
        "KeyQ" => 12,
        "KeyW" => 13,
        "KeyE" => 14,
        "KeyR" => 15,
        "KeyY" => 16,
        "KeyT" => 17,
        "KeyO" => 31,
        "KeyU" => 32,
        "KeyI" => 34,
        "KeyP" => 35,
        "KeyL" => 37,
        "KeyJ" => 38,
        "KeyK" => 40,
        "KeyN" => 45,
        "KeyM" => 46,
        "Digit1" => 18,
        "Digit2" => 19,
        "Digit3" => 20,
        "Digit4" => 21,
        "Digit6" => 22,
        "Digit5" => 23,
        "Digit9" => 25,
        "Digit7" => 26,
        "Digit8" => 28,
        "Digit0" => 29,
        "F1" => 122,
        "F2" => 120,
        "F3" => 99,
        "F4" => 118,
        "F5" => 96,
        "F6" => 97,
        "F7" => 98,
        "F8" => 100,
        "F9" => 101,
        "F10" => 109,
        "F11" => 103,
        "F12" => 111,
        "ArrowLeft" => 123,
        "ArrowRight" => 124,
        "ArrowDown" => 125,
        "ArrowUp" => 126,
        "Space" => 49,
        "Enter" => 36,
        "Escape" => 53,
        "Tab" => 48,
        "Backspace" => 51,
        "Delete" => 117,
        "Home" => 115,
        "End" => 119,
        "PageUp" => 116,
        "PageDown" => 121,
        "Equal" => 24,
        "Minus" => 27,
        "BracketRight" => 30,
        "BracketLeft" => 33,
        "Quote" => 39,
        "Semicolon" => 41,
        "Backslash" => 42,
        "Comma" => 43,
        "Slash" => 44,
        "Period" => 47,
        "Backquote" => 50,
        _ => return None,
    })
}

fn validate_application(path: &str) -> Result<(), String> {
    let path = std::path::Path::new(path);
    if !path.is_absolute()
        || !path.is_dir()
        || path.extension().is_none_or(|extension| extension != "app")
        || !path.join("Contents/Info.plist").is_file()
    {
        return Err("Choose an existing application with an absolute .app path.".into());
    }
    Ok(())
}

pub fn validate_action(action: &Action) -> Result<(), String> {
    match action {
        Action::LaunchApplication { path } => validate_application(path),
        Action::VolumeUp
        | Action::VolumeDown
        | Action::Mute
        | Action::PlayPause
        | Action::PreviousTrack
        | Action::NextTrack => Ok(()),
        _ => shortcut(action).map(|_| ()),
    }
}

pub fn pick_application() -> Result<Option<String>, String> {
    // No user-controlled text enters AppleScript; paths return on stdout as data.
    let script = "try\nset selectedApp to choose application with prompt \"Choose an application\" as alias\nreturn POSIX path of selectedApp\non error number -128\nreturn \"\"\nend try";
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("Could not open application picker: {e}"))?;
    if !output.status.success() {
        return Err("Application picker failed.".into());
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "Application picker returned invalid text.")?;
    let path = text.strip_suffix('\n').unwrap_or(&text);
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return Ok(None);
    }
    validate_application(path)?;
    Ok(Some(path.into()))
}

fn perform(action: Action) -> Result<(), String> {
    if let Action::LaunchApplication { path } = action {
        // Filesystem validation and Launch Services can block; keep them off the event tap.
        return thread::Builder::new()
            .name("qz-launch".into())
            .spawn(move || {
                let result = validate_application(&path).and_then(|_| {
                    std::process::Command::new("/usr/bin/open")
                        .args(["-a", &path])
                        .status()
                        .map_err(|e| format!("Could not launch application: {e}"))?
                        .success()
                        .then_some(())
                        .ok_or_else(|| "macOS could not open the selected application.".into())
                });
                if let (Err(error), Some(shared)) = (result, SHARED.get()) {
                    shared
                        .engine
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .error = Some(error);
                }
            })
            .map(|_| ())
            .map_err(|e| format!("Could not schedule application launch: {e}"));
    }
    validate_action(&action)?;
    if action == Action::Disable {
        return Ok(());
    }
    if action == Action::Default {
        return Err("Default actions must pass through unchanged.".into());
    }
    unsafe {
        if !CGPreflightPostEventAccess() {
            return Err(
                "macOS no longer allows shortcuts. Restore Accessibility permission for QZ Flow."
                    .into(),
            );
        }
        if CGEventSourceFlagsState(0) & (COMMAND | SHIFT | CONTROL | OPTION) != 0 {
            return Err(
                "Release Command, Control, Option and Shift before using a mapped button.".into(),
            );
        }
        if let Some(key) = media_key(&action) {
            return objc2::rc::autoreleasepool(|_| media_key_event(key));
        }
        let (key, flags) = shortcut(&action)?;
        if CGEventSourceKeyState(1, key) {
            return Err("Release the shortcut key before using its mapped button.".into());
        }
        let down = CGEventCreateKeyboardEvent(null(), key, true);
        let up = CGEventCreateKeyboardEvent(null(), key, false);
        if down.is_null() || up.is_null() {
            if !down.is_null() {
                CFRelease(down);
            }
            if !up.is_null() {
                CFRelease(up);
            }
            return Err("macOS could not create the shortcut.".into());
        }
        CGEventSetFlags(down, flags);
        CGEventSetFlags(up, flags);
        CGEventPost(0, down);
        CGEventPost(0, up);
        CFRelease(down);
        CFRelease(up);
    }
    Ok(())
}

fn media_key(action: &Action) -> Option<isize> {
    match action {
        Action::VolumeUp => Some(0),
        Action::VolumeDown => Some(1),
        Action::Mute => Some(7),
        Action::PlayPause => Some(16),
        Action::NextTrack => Some(17),
        Action::PreviousTrack => Some(18),
        _ => None,
    }
}

fn media_key_event(key: isize) -> Result<(), String> {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;
    let create = |state: isize| {
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(),
            0.0, 0, None, 8, (key << 16) | state, -1,
        ).and_then(|event| event.CGEvent())
    };
    let down = create(0x0a00).ok_or("macOS could not create media key press.")?;
    let up = create(0x0b00).ok_or("macOS could not create media key release.")?;
    unsafe {
        CGEventPost(0, (&*down as *const _ as *mut c_void).cast());
        CGEventPost(0, (&*up as *const _ as *mut c_void).cast());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_use_command_and_redo_adds_shift() {
        assert_eq!(shortcut(&Action::Copy).unwrap(), (8, COMMAND));
        assert_eq!(shortcut(&Action::Paste).unwrap(), (9, COMMAND));
        assert_eq!(shortcut(&Action::Redo).unwrap(), (6, COMMAND | SHIFT));
        assert_eq!(shortcut(&Action::Maximize).unwrap(), (3, CONTROL | COMMAND));
    }
    #[test]
    fn custom_shortcuts_and_media_keys_are_explicit() {
        assert_eq!(
            shortcut(&Action::CustomShortcut {
                key: "KeyK".into(),
                modifiers: vec!["meta".into(), "shift".into()]
            })
            .unwrap(),
            (40, COMMAND | SHIFT)
        );
        for key in ["Insert", "Keya", "F13", "F01", "Shell", ""] {
            assert!(custom_key(key).is_none());
        }
        for key in ["KeyZ", "Digit0", "F12", "Delete", "Slash"] {
            assert!(custom_key(key).is_some());
        }
        assert!(validate_action(&Action::CustomShortcut {
            key: "KeyK".into(),
            modifiers: vec!["meta".into(), "meta".into()]
        })
        .is_err());
        assert!(validate_action(&Action::CustomShortcut {
            key: "KeyK".into(),
            modifiers: vec!["shell".into()]
        })
        .is_err());
        assert_eq!(media_key(&Action::Mute), Some(7));
        assert_eq!(media_key(&Action::PlayPause), Some(16));
        assert_eq!(media_key(&Action::PreviousTrack), Some(18));
        assert!(validate_application("Safari.app").is_err());
        assert!(validate_application("/Applications/Safari.app --args -c bad").is_err());
        assert_eq!(shortcut(&Action::PreviousDesktop).unwrap(), (123, CONTROL));
        assert_eq!(shortcut(&Action::NextDesktop).unwrap(), (124, CONTROL));
        assert_eq!(
            shortcut(&Action::PreviousTab).unwrap(),
            (48, CONTROL | SHIFT)
        );
        assert_eq!(shortcut(&Action::NextTab).unwrap(), (48, CONTROL));
        for letter in b'A'..=b'Z' {
            assert!(custom_key(&format!("Key{}", letter as char)).is_some());
        }
    }
}
