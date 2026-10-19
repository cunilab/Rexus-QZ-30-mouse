use crate::engine::Action;
use std::{
    collections::BTreeMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

static RECORDING: Mutex<()> = Mutex::new(());
const TIMEOUT: Duration = Duration::from_secs(10);
const RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

// Invoke on a blocking worker: native event delivery and focus checks need the UI thread free.
pub fn record(app: &AppHandle) -> Result<Option<Action>, String> {
    let _recording = RECORDING
        .try_lock()
        .map_err(|_| "A shortcut recording is already in progress.")?;
    let window = app
        .get_webview_window("main")
        .ok_or("QZ Flow's window is unavailable.")?;
    if !window.is_focused().unwrap_or(false) {
        return Err("Open and focus QZ Flow before recording a shortcut.".into());
    }
    let result = native::record(|| window.is_focused().unwrap_or(false))?;
    if window.is_focused().unwrap_or(false) {
        Ok(result)
    } else {
        Ok(None)
    }
}

#[derive(Default)]
struct Capture {
    pressed: BTreeMap<u16, Option<&'static str>>,
    result: Option<Result<Option<Action>, String>>,
    finished_at: Option<Instant>,
}

impl Capture {
    fn finish(&mut self, result: Result<Option<Action>, String>) {
        if self.result.is_none() {
            self.result = Some(result);
            self.finished_at = Some(Instant::now());
        }
    }

    fn event(
        &mut self,
        physical: u16,
        code: Option<&str>,
        modifier: Option<&'static str>,
        down: bool,
    ) -> bool {
        if self.result.is_some() && !self.pressed.contains_key(&physical) {
            return false;
        }
        if !down {
            return self.pressed.remove(&physical).is_some();
        }
        if self.pressed.insert(physical, modifier).is_some() || self.result.is_some() {
            return true;
        }
        if modifier.is_some() {
            return true;
        }
        match code {
            Some("Escape") => self.finish(Ok(None)),
            Some(key) => {
                let modifiers = ["control", "shift", "alt", "meta"].into_iter()
                    .filter(|name| self.pressed.values().any(|value| *value == Some(*name)))
                    .map(str::to_owned).collect();
                self.finish(Ok(Some(Action::CustomShortcut { key: key.into(), modifiers })));
            }
            None => self.finish(Err("That key cannot be recorded. Choose a letter, number, F1–F12, navigation or punctuation key.".into())),
        }
        true
    }

    fn poll(&mut self, started: Instant, focused: bool) -> bool {
        if !focused {
            self.finish(Ok(None));
            self.result = Some(Ok(None));
        } else if started.elapsed() >= TIMEOUT {
            self.finish(Ok(None));
        }
        self.result.is_some()
            && (self.pressed.is_empty()
                || self
                    .finished_at
                    .is_some_and(|time| time.elapsed() >= RELEASE_TIMEOUT))
    }

    fn into_result(self) -> Result<Option<Action>, String> {
        if !self.pressed.is_empty() {
            return Err(
                "Recording stopped while keys were held. Release all keys, then try again.".into(),
            );
        }
        self.result.unwrap_or(Ok(None))
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{cell::RefCell, ptr::null_mut, thread};
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    thread_local! { static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) }; }

    fn key(vk: u32, scan: u32, extended: bool) -> (Option<&'static str>, Option<&'static str>) {
        let modifier = match vk as u16 {
            VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some("control"),
            VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some("shift"),
            VK_MENU | VK_LMENU | VK_RMENU => Some("alt"),
            VK_LWIN | VK_RWIN => Some("meta"),
            _ => None,
        };
        // DOM code describes the physical key, independently of the active keyboard layout.
        let code = if extended {
            match scan {
                0x47 => Some("Home"),
                0x48 => Some("ArrowUp"),
                0x49 => Some("PageUp"),
                0x4b => Some("ArrowLeft"),
                0x4d => Some("ArrowRight"),
                0x4f => Some("End"),
                0x50 => Some("ArrowDown"),
                0x51 => Some("PageDown"),
                0x52 => Some("Insert"),
                0x53 => Some("Delete"),
                _ => None,
            }
        } else {
            Some(match scan {
                0x01 => "Escape",
                0x02 => "Digit1",
                0x03 => "Digit2",
                0x04 => "Digit3",
                0x05 => "Digit4",
                0x06 => "Digit5",
                0x07 => "Digit6",
                0x08 => "Digit7",
                0x09 => "Digit8",
                0x0a => "Digit9",
                0x0b => "Digit0",
                0x0c => "Minus",
                0x0d => "Equal",
                0x0e => "Backspace",
                0x0f => "Tab",
                0x10 => "KeyQ",
                0x11 => "KeyW",
                0x12 => "KeyE",
                0x13 => "KeyR",
                0x14 => "KeyT",
                0x15 => "KeyY",
                0x16 => "KeyU",
                0x17 => "KeyI",
                0x18 => "KeyO",
                0x19 => "KeyP",
                0x1a => "BracketLeft",
                0x1b => "BracketRight",
                0x1c => "Enter",
                0x1e => "KeyA",
                0x1f => "KeyS",
                0x20 => "KeyD",
                0x21 => "KeyF",
                0x22 => "KeyG",
                0x23 => "KeyH",
                0x24 => "KeyJ",
                0x25 => "KeyK",
                0x26 => "KeyL",
                0x27 => "Semicolon",
                0x28 => "Quote",
                0x29 => "Backquote",
                0x2b => "Backslash",
                0x2c => "KeyZ",
                0x2d => "KeyX",
                0x2e => "KeyC",
                0x2f => "KeyV",
                0x30 => "KeyB",
                0x31 => "KeyN",
                0x32 => "KeyM",
                0x33 => "Comma",
                0x34 => "Period",
                0x35 => "Slash",
                0x39 => "Space",
                0x3b => "F1",
                0x3c => "F2",
                0x3d => "F3",
                0x3e => "F4",
                0x3f => "F5",
                0x40 => "F6",
                0x41 => "F7",
                0x42 => "F8",
                0x43 => "F9",
                0x44 => "F10",
                0x57 => "F11",
                0x58 => "F12",
                _ => return (None, modifier),
            })
        };
        (code, modifier)
    }

    unsafe extern "system" fn callback(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
        if code < 0 || data == 0 {
            return CallNextHookEx(null_mut(), code, message, data);
        }
        let event = &*(data as *const KBDLLHOOKSTRUCT);
        if event.flags & LLKHF_INJECTED != 0 {
            return CallNextHookEx(null_mut(), code, message, data);
        }
        let swallowed = CAPTURE.with(|capture| {
            let mut capture = capture.borrow_mut();
            let Some(capture) = capture.as_mut() else {
                return false;
            };
            let down = message as u32 == WM_KEYDOWN || message as u32 == WM_SYSKEYDOWN;
            let (key, modifier) = key(
                event.vkCode,
                event.scanCode,
                event.flags & LLKHF_EXTENDED != 0,
            );
            let physical = event.scanCode as u16
                | if event.flags & LLKHF_EXTENDED != 0 {
                    0x100
                } else {
                    0
                };
            capture.event(physical, key, modifier, down)
        });
        if swallowed {
            1
        } else {
            CallNextHookEx(null_mut(), code, message, data)
        }
    }

    struct Hook(HHOOK);
    impl Drop for Hook {
        fn drop(&mut self) {
            unsafe {
                UnhookWindowsHookEx(self.0);
            }
        }
    }

    pub fn record(focused: impl Fn() -> bool) -> Result<Option<Action>, String> {
        unsafe {
            if (8..=254).any(|key| GetAsyncKeyState(key) < 0) {
                return Err("Release all keyboard keys before recording a shortcut.".into());
            }
            CAPTURE.with(|capture| *capture.borrow_mut() = Some(Capture::default()));
            let hook = Hook(SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(callback),
                GetModuleHandleW(std::ptr::null()),
                0,
            ));
            if hook.0.is_null() {
                CAPTURE.with(|capture| capture.borrow_mut().take());
                return Err(format!(
                    "Windows could not start shortcut recording: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if (8..=254).any(|key| GetAsyncKeyState(key) < 0) {
                drop(hook);
                CAPTURE.with(|capture| capture.borrow_mut().take());
                return Err("Release all keyboard keys before recording a shortcut.".into());
            }
            let started = Instant::now();
            let mut next_focus = Instant::now();
            let mut has_focus = true;
            loop {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if message.message == WM_QUIT {
                        has_focus = false;
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if next_focus.elapsed() >= Duration::from_millis(50) {
                    has_focus &= focused();
                    next_focus = Instant::now();
                }
                if CAPTURE.with(|capture| {
                    capture
                        .borrow_mut()
                        .as_mut()
                        .unwrap()
                        .poll(started, has_focus)
                }) {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            drop(hook);
            CAPTURE.with(|capture| capture.borrow_mut().take().unwrap().into_result())
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn dom_codes_use_physical_scan_codes_and_distinguish_navigation_from_numpad() {
            assert_eq!(key(0x5a, 0x15, false).0, Some("KeyY"));
            assert_eq!(key(VK_LEFT as u32, 0x4b, true).0, Some("ArrowLeft"));
            assert_eq!(key(VK_LEFT as u32, 0x4b, false).0, None);
            assert_eq!(key(VK_RETURN as u32, 0x1c, true).0, None);
            assert_eq!(key(VK_RCONTROL as u32, 0x1d, true).1, Some("control"));
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use std::{
        ffi::c_void,
        ptr::{null, null_mut},
    };
    type Ref = *const c_void;
    type Event = *mut c_void;
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            mask: u64,
            callback: unsafe extern "C" fn(Event, u32, Event, *mut c_void) -> Event,
            data: *mut c_void,
        ) -> Ref;
        fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
        fn CFMachPortInvalidate(port: Ref);
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
        fn CFRelease(value: Ref);
        static kCFRunLoopCommonModes: Ref;
        static kCFRunLoopDefaultMode: Ref;
    }

    fn key(key: u16) -> (Option<&'static str>, Option<&'static str>) {
        let modifier = match key {
            59 | 62 => Some("control"),
            56 | 60 => Some("shift"),
            58 | 61 => Some("alt"),
            54 | 55 => Some("meta"),
            _ => None,
        };
        let code = match key {
            0 => "KeyA",
            1 => "KeyS",
            2 => "KeyD",
            3 => "KeyF",
            4 => "KeyH",
            5 => "KeyG",
            6 => "KeyZ",
            7 => "KeyX",
            8 => "KeyC",
            9 => "KeyV",
            11 => "KeyB",
            12 => "KeyQ",
            13 => "KeyW",
            14 => "KeyE",
            15 => "KeyR",
            16 => "KeyY",
            17 => "KeyT",
            18 => "Digit1",
            19 => "Digit2",
            20 => "Digit3",
            21 => "Digit4",
            22 => "Digit6",
            23 => "Digit5",
            24 => "Equal",
            25 => "Digit9",
            26 => "Digit7",
            27 => "Minus",
            28 => "Digit8",
            29 => "Digit0",
            30 => "BracketRight",
            31 => "KeyO",
            32 => "KeyU",
            33 => "BracketLeft",
            34 => "KeyI",
            35 => "KeyP",
            36 => "Enter",
            37 => "KeyL",
            38 => "KeyJ",
            39 => "Quote",
            40 => "KeyK",
            41 => "Semicolon",
            42 => "Backslash",
            43 => "Comma",
            44 => "Slash",
            45 => "KeyN",
            46 => "KeyM",
            47 => "Period",
            48 => "Tab",
            49 => "Space",
            50 => "Backquote",
            51 => "Backspace",
            53 => "Escape",
            122 => "F1",
            120 => "F2",
            99 => "F3",
            118 => "F4",
            96 => "F5",
            97 => "F6",
            98 => "F7",
            100 => "F8",
            101 => "F9",
            109 => "F10",
            103 => "F11",
            111 => "F12",
            115 => "Home",
            119 => "End",
            116 => "PageUp",
            121 => "PageDown",
            117 => "Delete",
            123 => "ArrowLeft",
            124 => "ArrowRight",
            125 => "ArrowDown",
            126 => "ArrowUp",
            _ => return (None, modifier),
        };
        (Some(code), modifier)
    }

    unsafe extern "C" fn callback(
        _proxy: Event,
        kind: u32,
        event: Event,
        data: *mut c_void,
    ) -> Event {
        let capture = &mut *(data as *mut Capture);
        if kind == u32::MAX || kind == u32::MAX - 1 {
            capture.finish(Err(
                "macOS interrupted recording. Secure system shortcuts cannot be captured reliably."
                    .into(),
            ));
            return event;
        }
        if event.is_null() {
            return event;
        }
        // User-space synthetic events belong to their destination, not this recording.
        if CGEventGetIntegerValueField(event, 41) != 0 {
            return event;
        }
        let physical = CGEventGetIntegerValueField(event, 9) as u16;
        let (key, modifier) = key(physical);
        let down = if kind == 12 {
            !capture.pressed.contains_key(&physical)
        } else {
            kind == 10
        };
        if capture.event(physical, key, modifier, down) {
            null_mut()
        } else {
            event
        }
    }

    struct Tap {
        port: Ref,
        source: Ref,
        run_loop: Ref,
    }
    impl Drop for Tap {
        fn drop(&mut self) {
            unsafe {
                CFRunLoopRemoveSource(self.run_loop, self.source, kCFRunLoopCommonModes);
                CFMachPortInvalidate(self.port);
                CFRelease(self.source);
                CFRelease(self.port);
            }
        }
    }

    pub fn record(focused: impl Fn() -> bool) -> Result<Option<Action>, String> {
        if crate::platform::permission() != "granted" {
            return Err(
                "Allow QZ Flow in Accessibility and Input Monitoring before recording a shortcut."
                    .into(),
            );
        }
        unsafe {
            if (0..128).any(|key| CGEventSourceKeyState(1, key)) {
                return Err("Release all keyboard keys before recording a shortcut.".into());
            }
            let mut capture = Box::<Capture>::default();
            let port = CGEventTapCreate(
                1,
                0,
                0,
                (1 << 10) | (1 << 11) | (1 << 12),
                callback,
                (&mut *capture as *mut Capture).cast(),
            );
            if port.is_null() {
                return Err("macOS could not start recording. Secure input or input permissions may be blocking access.".into());
            }
            let source = CFMachPortCreateRunLoopSource(null(), port, 0);
            if source.is_null() {
                CFMachPortInvalidate(port);
                CFRelease(port);
                return Err("macOS could not create the recording event source.".into());
            }
            let tap = Tap {
                port,
                source,
                run_loop: CFRunLoopGetCurrent(),
            };
            CFRunLoopAddSource(tap.run_loop, source, kCFRunLoopCommonModes);
            if (0..128).any(|key| CGEventSourceKeyState(1, key)) {
                return Err("Release all keyboard keys before recording a shortcut.".into());
            }
            let started = Instant::now();
            let mut has_focus = true;
            loop {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.02, false);
                has_focus &= focused();
                if capture.poll(started, has_focus) {
                    break;
                }
            }
            drop(tap);
            capture.into_result()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_one_chord_and_waits_for_all_consumed_releases() {
        let mut capture = Capture::default();
        capture.event(1, None, Some("control"), true);
        capture.event(2, None, Some("shift"), true);
        capture.event(3, Some("KeyA"), None, true);
        capture.event(3, Some("KeyA"), None, true);
        assert!(!capture.poll(Instant::now(), true));
        capture.event(3, Some("KeyA"), None, false);
        capture.event(1, None, Some("control"), false);
        assert!(!capture.poll(Instant::now(), true));
        capture.event(2, None, Some("shift"), false);
        assert!(capture.poll(Instant::now(), true));
        assert_eq!(
            capture.into_result().unwrap(),
            Some(Action::CustomShortcut {
                key: "KeyA".into(),
                modifiers: vec!["control".into(), "shift".into()],
            })
        );
    }

    #[test]
    fn cancellation_drains_and_timeout_does_not_record_a_later_key() {
        let mut capture = Capture::default();
        capture.event(1, Some("Escape"), None, true);
        assert!(!capture.poll(Instant::now(), false));
        capture.event(2, Some("KeyA"), None, true);
        capture.event(1, Some("Escape"), None, false);
        capture.event(2, Some("KeyA"), None, false);
        assert_eq!(capture.into_result().unwrap(), None);
        let mut capture = Capture::default();
        assert!(capture.poll(Instant::now() - TIMEOUT, true));
        assert_eq!(capture.into_result().unwrap(), None);
    }

    #[test]
    fn held_keys_at_cleanup_deadline_return_an_error() {
        let mut capture = Capture::default();
        capture.event(1, None, Some("alt"), true);
        capture.finish(Ok(None));
        capture.finished_at = Some(Instant::now() - RELEASE_TIMEOUT);
        assert!(capture.poll(Instant::now(), true));
        assert!(capture
            .into_result()
            .unwrap_err()
            .contains("Release all keys"));
    }

    #[test]
    fn focus_loss_discards_captured_chord_and_only_drains_consumed_keys() {
        let mut capture = Capture::default();
        assert!(capture.event(1, None, Some("control"), true));
        assert!(capture.event(2, Some("KeyA"), None, true));
        assert!(!capture.poll(Instant::now(), false));
        assert!(!capture.event(3, Some("KeyB"), None, true));
        assert!(!capture.event(3, Some("KeyB"), None, false));
        assert!(capture.event(2, Some("KeyA"), None, true));
        assert!(capture.event(2, Some("KeyA"), None, false));
        assert!(capture.event(1, None, Some("control"), false));
        assert!(capture.poll(Instant::now(), false));
        assert_eq!(capture.into_result().unwrap(), None);
    }
}
