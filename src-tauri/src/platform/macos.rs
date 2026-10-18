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
    fn CGRequestListenEventAccess() -> bool;
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
    if unsafe { AXIsProcessTrusted() } {
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
    unsafe {
        CGRequestListenEventAccess();
    }
    if permission() != "granted" {
        return Err("Allow QZ Flow in System Settings > Privacy & Security > Accessibility and Input Monitoring, then restart the app.".into());
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

fn shortcut(action: Action) -> (u16, u64) {
    match action {
        Action::Copy => (8, COMMAND),
        Action::Paste => (9, COMMAND),
        Action::Undo => (6, COMMAND),
        Action::Redo => (6, COMMAND | SHIFT),
        Action::SelectAll => (0, COMMAND),
        Action::Desktop => (103, 0), // F11 follows the person's Mission Control shortcut settings.
        Action::BrowserBack => (33, COMMAND),
        Action::BrowserForward => (30, COMMAND),
        Action::TaskView => (126, CONTROL),
        Action::VolumeUp | Action::VolumeDown => (0, 0),
        Action::Default => (0, 0),
    }
}

fn perform(action: Action) -> Result<(), String> {
    let (key, flags) = shortcut(action);
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
        if matches!(action, Action::VolumeUp | Action::VolumeDown) {
            return media_volume(action == Action::VolumeUp);
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

fn media_volume(up: bool) -> Result<(), String> {
    objc2::rc::autoreleasepool(|_| media_volume_event(up))
}

fn media_volume_event(up: bool) -> Result<(), String> {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    use objc2_foundation::NSPoint;
    let key = if up { 0 } else { 1 };
    let create = |state| {
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(),
            0.0, 0, None, 8, (key << 16) | state, -1,
        ).and_then(|event| event.CGEvent())
    };
    let down = create(0x0a00).ok_or("macOS could not create volume key press.")?;
    let up = create(0x0b00).ok_or("macOS could not create volume key release.")?;
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
        assert_eq!(shortcut(Action::Copy), (8, COMMAND));
        assert_eq!(shortcut(Action::Paste), (9, COMMAND));
        assert_eq!(shortcut(Action::Redo), (6, COMMAND | SHIFT));
    }
}
