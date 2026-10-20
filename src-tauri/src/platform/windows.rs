use crate::{
    engine::{Action, Input},
    Device, Shared,
};
mod keyboard;
mod trace;
use std::{
    mem::size_of,
    ptr::null_mut,
    sync::{atomic::Ordering, mpsc, Arc, Mutex, OnceLock},
    thread::{self, JoinHandle},
};
pub use trace::trace_buttons;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE, LPARAM, LRESULT,
        POINT, WPARAM,
    },
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::*,
        Input::{
            GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICENAME,
        },
        WindowsAndMessaging::*,
    },
};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
const INJECTED_TAG: usize = 0x515A464C;
static INJECTED_KEYS: Mutex<keyboard::Keyboard> = Mutex::new(keyboard::Keyboard::new());

fn send_inputs(inputs: &[INPUT]) -> usize {
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        ) as usize
    }
}

fn injected_key_is_up(virtual_key: u16, scan: u16, flags: u32) -> Option<bool> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null()
            || GetCursorPos(&mut POINT::default()) == 0
            || flags & KEYEVENTF_UNICODE != 0
        {
            return None;
        }
        let key = if flags & KEYEVENTF_SCANCODE != 0 {
            let foreground_thread = GetWindowThreadProcessId(foreground, null_mut());
            let layout = GetKeyboardLayout(foreground_thread);
            if layout.is_null() {
                return None;
            }
            let scan = scan as u32
                | if flags & KEYEVENTF_EXTENDEDKEY != 0 {
                    0xe000
                } else {
                    0
                };
            MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, layout)
        } else {
            virtual_key as u32
        };
        if !(1..=254).contains(&key) {
            return None;
        }
        Some(GetAsyncKeyState(key as i32) >= 0)
    }
}

fn pause_keyboard_error(shared: &Shared, error: &str) {
    let mut engine = shared.engine.lock().unwrap_or_else(|e| e.into_inner());
    engine.enabled = false;
    engine.paused = true;
    engine.error = Some(error.into());
}

fn keyboard_result(result: Result<(), String>) -> Result<(), String> {
    if let (Err(error), Some(shared)) = (&result, SHARED.get()) {
        pause_keyboard_error(shared, error);
    }
    result
}

pub fn release_injected() -> Result<(), String> {
    let result = INJECTED_KEYS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .recover(&mut injected_key_is_up);
    keyboard_result(result)
}

pub fn devices() -> Result<Vec<Device>, String> {
    unsafe {
        let mut count = 0;
        let size = size_of::<RAWINPUTDEVICELIST>() as u32;
        if GetRawInputDeviceList(null_mut(), &mut count, size) == u32::MAX {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let found = GetRawInputDeviceList(list.as_mut_ptr(), &mut count, size);
        if found == u32::MAX {
            return Err("Mouse inventory changed while scanning. Try again.".into());
        }
        let mut devices = Vec::new();
        for item in list.into_iter().take(found as usize) {
            let mut length = 0;
            if GetRawInputDeviceInfoW(item.hDevice, RIDI_DEVICENAME, null_mut(), &mut length)
                == u32::MAX
            {
                continue;
            }
            let mut name = vec![0u16; length as usize + 1];
            if GetRawInputDeviceInfoW(
                item.hDevice,
                RIDI_DEVICENAME,
                name.as_mut_ptr().cast(),
                &mut length,
            ) == u32::MAX
            {
                continue;
            }
            let path = String::from_utf16_lossy(&name).to_ascii_uppercase();
            if compatible_path(&path) && devices.is_empty() {
                devices.push(Device {
                    vendor_id: 0x276d,
                    product_id: 0x1902,
                    name: "QZ30-compatible USB receiver".into(),
                    transport: "USB / 2.4 GHz receiver".into(),
                });
            }
        }
        Ok(devices)
    }
}

fn compatible_path(path: &str) -> bool {
    path.split(['&', '#', '\\']).any(|part| part == "VID_276D")
        && path.split(['&', '#', '\\']).any(|part| part == "PID_1902")
}

pub fn permission() -> &'static str {
    "granted"
}

pub fn ensure_no_vendor_utility() -> Result<(), String> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err("Could not check for the original mouse utility. Try again.".into());
        }
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut exists = Process32FirstW(snapshot, &mut entry) != 0;
        let mut found = false;
        while exists {
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length])
                .eq_ignore_ascii_case("MAssistant.exe")
            {
                found = true;
                break;
            }
            exists = Process32NextW(snapshot, &mut entry) != 0;
        }
        let scan_failed = !found && GetLastError() != ERROR_NO_MORE_FILES;
        CloseHandle(snapshot);
        if scan_failed {
            Err("Could not finish checking for the original mouse utility. Try again.".into())
        } else if found {
            Err("Close the original Mouse Assistant from its system tray menu before enabling QZ Flow.".into())
        } else {
            Ok(())
        }
    }
}

pub fn open_input_settings() -> Result<(), String> {
    Err("Windows does not need an input permission dialog. Elevated apps may reject remapped shortcuts.".into())
}

pub struct Hook {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start(shared: Arc<Shared>) -> Result<Hook, String> {
    let _ = SHARED.set(shared.clone());
    let (sender, receiver) = mpsc::sync_channel(1);
    let join = thread::spawn(move || unsafe {
        let mut msg = MSG::default();
        PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
        let hook = SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(callback),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if hook.is_null() {
            let _ = sender.send(Err(std::io::Error::last_os_error().to_string()));
            return;
        }
        let receiver_window = match trace::vendor_window() {
            Ok(hwnd) => hwnd,
            Err(error) => {
                UnhookWindowsHookEx(hook);
                let _ = sender.send(Err(error));
                return;
            }
        };
        shared.hook_running.store(true, Ordering::Release);
        if sender.send(Ok(GetCurrentThreadId())).is_ok() {
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        DestroyWindow(receiver_window);
        UnhookWindowsHookEx(hook);
        shared.hook_running.store(false, Ordering::Release);
        shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .enabled = false;
    });
    match receiver.recv() {
        Ok(Ok(thread_id)) => Ok(Hook {
            thread_id,
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

unsafe extern "system" fn callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        let data = &*(lparam as *const MSLLHOOKSTRUCT);
        if data.flags & LLMHF_INJECTED == 0 && data.dwExtraInfo != INJECTED_TAG {
            let event = match wparam as u32 {
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let button = data.mouseData >> 16;
                    match button {
                        1 => Some((Input::Back, wparam as u32 == WM_XBUTTONDOWN)),
                        2 => Some((Input::Forward, wparam as u32 == WM_XBUTTONDOWN)),
                        _ => None,
                    }
                }
                WM_MBUTTONDOWN => Some((Input::Middle, true)),
                WM_MBUTTONUP => Some((Input::Middle, false)),
                WM_MOUSEHWHEEL => {
                    let delta = (data.mouseData >> 16) as i16;
                    if delta == 0 {
                        None
                    } else {
                        Some((
                            if delta > 0 {
                                Input::WheelRight
                            } else {
                                Input::WheelLeft
                            },
                            true,
                        ))
                    }
                }
                _ => None,
            };
            if let (Some(shared), Some((input, down))) = (SHARED.get(), event) {
                if shared.process(input, down, perform) {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

fn shortcut(action: &Action) -> Result<Vec<u16>, String> {
    Ok(match action {
        Action::Default | Action::Disable => vec![],
        Action::Copy => vec![VK_CONTROL, b'C' as u16],
        Action::Cut => vec![VK_CONTROL, b'X' as u16],
        Action::Paste => vec![VK_CONTROL, b'V' as u16],
        Action::Undo => vec![VK_CONTROL, b'Z' as u16],
        Action::Redo => vec![VK_CONTROL, b'Y' as u16],
        Action::SelectAll => vec![VK_CONTROL, b'A' as u16],
        Action::Desktop => vec![VK_LWIN, b'D' as u16],
        Action::BrowserBack => vec![VK_BROWSER_BACK],
        Action::BrowserForward => vec![VK_BROWSER_FORWARD],
        Action::TaskView => vec![VK_LWIN, VK_TAB],
        Action::VolumeUp => vec![VK_VOLUME_UP],
        Action::VolumeDown => vec![VK_VOLUME_DOWN],
        Action::PreviousDesktop => vec![VK_CONTROL, VK_LWIN, VK_LEFT],
        Action::NextDesktop => vec![VK_CONTROL, VK_LWIN, VK_RIGHT],
        Action::SwitchApps => vec![VK_MENU, VK_TAB],
        Action::PreviousTab => vec![VK_CONTROL, VK_SHIFT, VK_TAB],
        Action::NextTab => vec![VK_CONTROL, VK_TAB],
        Action::NewTab => vec![VK_CONTROL, b'T' as u16],
        Action::CloseTab => vec![VK_CONTROL, b'W' as u16],
        Action::ReopenTab => vec![VK_CONTROL, VK_SHIFT, b'T' as u16],
        Action::Refresh => vec![VK_CONTROL, b'R' as u16],
        Action::Mute => vec![VK_VOLUME_MUTE],
        Action::PlayPause => vec![VK_MEDIA_PLAY_PAUSE],
        Action::PreviousTrack => vec![VK_MEDIA_PREV_TRACK],
        Action::NextTrack => vec![VK_MEDIA_NEXT_TRACK],
        Action::ZoomIn => vec![VK_CONTROL, VK_OEM_PLUS],
        Action::ZoomOut => vec![VK_CONTROL, VK_OEM_MINUS],
        Action::ZoomReset => vec![VK_CONTROL, b'0' as u16],
        Action::Screenshot => vec![VK_LWIN, VK_SHIFT, b'S' as u16],
        Action::CustomShortcut { key, modifiers } => {
            let mut keys = Vec::new();
            for modifier in modifiers {
                let code = match modifier.as_str() {
                    "control" => VK_CONTROL,
                    "alt" => VK_MENU,
                    "shift" => VK_SHIFT,
                    "meta" => VK_LWIN,
                    _ => return Err(format!("Unknown shortcut modifier: {modifier}")),
                };
                if keys.contains(&code) {
                    return Err("Shortcut modifiers must be unique.".into());
                }
                keys.push(code);
            }
            keys.push(custom_key(key).ok_or_else(|| format!("Unsupported shortcut key: {key}"))?);
            keys
        }
        Action::Minimize | Action::Maximize | Action::LaunchApplication { .. } => {
            return Err("This action does not use a keyboard shortcut.".into());
        }
    })
}

fn custom_key(key: &str) -> Option<u16> {
    if let Some(letter) = key.strip_prefix("Key").filter(|s| s.len() == 1) {
        return letter.as_bytes()[0]
            .is_ascii_uppercase()
            .then_some(letter.as_bytes()[0] as u16);
    }
    if let Some(digit) = key.strip_prefix("Digit").filter(|s| s.len() == 1) {
        return digit.as_bytes()[0]
            .is_ascii_digit()
            .then_some(digit.as_bytes()[0] as u16);
    }
    if let Some(number) = key.strip_prefix('F').and_then(|s| s.parse::<u16>().ok()) {
        if (1..=12).contains(&number) && key == format!("F{number}") {
            return Some(VK_F1 + number - 1);
        }
    }
    Some(match key {
        "ArrowLeft" => VK_LEFT,
        "ArrowRight" => VK_RIGHT,
        "ArrowUp" => VK_UP,
        "ArrowDown" => VK_DOWN,
        "Space" => VK_SPACE,
        "Enter" => VK_RETURN,
        "Escape" => VK_ESCAPE,
        "Tab" => VK_TAB,
        "Backspace" => VK_BACK,
        "Delete" => VK_DELETE,
        "Home" => VK_HOME,
        "End" => VK_END,
        "PageUp" => VK_PRIOR,
        "PageDown" => VK_NEXT,
        "Insert" => VK_INSERT,
        "Minus" => VK_OEM_MINUS,
        "Equal" => VK_OEM_PLUS,
        "BracketLeft" => VK_OEM_4,
        "BracketRight" => VK_OEM_6,
        "Backslash" => VK_OEM_5,
        "Semicolon" => VK_OEM_1,
        "Quote" => VK_OEM_7,
        "Comma" => VK_OEM_COMMA,
        "Period" => VK_OEM_PERIOD,
        "Slash" => VK_OEM_2,
        "Backquote" => VK_OEM_3,
        _ => return None,
    })
}

fn custom_scan(key: &str) -> Option<u16> {
    Some(match key {
        "Escape" => 0x01,
        "Digit1" => 0x02,
        "Digit2" => 0x03,
        "Digit3" => 0x04,
        "Digit4" => 0x05,
        "Digit5" => 0x06,
        "Digit6" => 0x07,
        "Digit7" => 0x08,
        "Digit8" => 0x09,
        "Digit9" => 0x0a,
        "Digit0" => 0x0b,
        "Minus" => 0x0c,
        "Equal" => 0x0d,
        "Backspace" => 0x0e,
        "Tab" => 0x0f,
        "KeyQ" => 0x10,
        "KeyW" => 0x11,
        "KeyE" => 0x12,
        "KeyR" => 0x13,
        "KeyT" => 0x14,
        "KeyY" => 0x15,
        "KeyU" => 0x16,
        "KeyI" => 0x17,
        "KeyO" => 0x18,
        "KeyP" => 0x19,
        "BracketLeft" => 0x1a,
        "BracketRight" => 0x1b,
        "Enter" => 0x1c,
        "KeyA" => 0x1e,
        "KeyS" => 0x1f,
        "KeyD" => 0x20,
        "KeyF" => 0x21,
        "KeyG" => 0x22,
        "KeyH" => 0x23,
        "KeyJ" => 0x24,
        "KeyK" => 0x25,
        "KeyL" => 0x26,
        "Semicolon" => 0x27,
        "Quote" => 0x28,
        "Backquote" => 0x29,
        "Backslash" => 0x2b,
        "KeyZ" => 0x2c,
        "KeyX" => 0x2d,
        "KeyC" => 0x2e,
        "KeyV" => 0x2f,
        "KeyB" => 0x30,
        "KeyN" => 0x31,
        "KeyM" => 0x32,
        "Comma" => 0x33,
        "Period" => 0x34,
        "Slash" => 0x35,
        "Space" => 0x39,
        "F1" => 0x3b,
        "F2" => 0x3c,
        "F3" => 0x3d,
        "F4" => 0x3e,
        "F5" => 0x3f,
        "F6" => 0x40,
        "F7" => 0x41,
        "F8" => 0x42,
        "F9" => 0x43,
        "F10" => 0x44,
        "F11" => 0x57,
        "F12" => 0x58,
        "Home" => 0xe047,
        "ArrowUp" => 0xe048,
        "PageUp" => 0xe049,
        "ArrowLeft" => 0xe04b,
        "ArrowRight" => 0xe04d,
        "End" => 0xe04f,
        "ArrowDown" => 0xe050,
        "PageDown" => 0xe051,
        "Insert" => 0xe052,
        "Delete" => 0xe053,
        _ => return None,
    })
}

fn validate_application(path: &str) -> Result<(), String> {
    let path = std::path::Path::new(path);
    if !path.is_absolute()
        || !path.is_file()
        || !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err("Choose an existing application with an absolute .exe path.".into());
    }
    Ok(())
}

pub fn validate_action(action: &Action) -> Result<(), String> {
    match action {
        Action::LaunchApplication { path } => validate_application(path),
        Action::Minimize | Action::Maximize => Ok(()),
        _ => shortcut(action).map(|_| ()),
    }
}

pub fn pick_application() -> Result<Option<String>, String> {
    use std::os::windows::process::CommandExt;
    // The script is fixed; the selected path is data and is never interpolated into PowerShell.
    let script = "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.OpenFileDialog; $dialog.Title = 'Choose an application'; $dialog.Filter = 'Applications (*.exe)|*.exe'; if ($dialog.ShowDialog() -eq 'OK') { [Console]::Write($dialog.FileName) }";
    let powershell = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or("Windows system directory is unavailable.")?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let output = std::process::Command::new(powershell)
        .args(["-NoProfile", "-STA", "-NonInteractive", "-Command", script])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Could not open application picker: {e}"))?;
    if !output.status.success() {
        return Err("Application picker failed.".into());
    }
    let path = String::from_utf8(output.stdout)
        .map_err(|_| "Application picker returned invalid text.")?;
    if path.is_empty() {
        return Ok(None);
    }
    validate_application(&path)?;
    Ok(Some(path))
}

fn key_input(key: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }
                    | if matches!(
                        key,
                        VK_LEFT
                            | VK_RIGHT
                            | VK_UP
                            | VK_DOWN
                            | VK_HOME
                            | VK_END
                            | VK_PRIOR
                            | VK_NEXT
                            | VK_INSERT
                            | VK_DELETE
                            | VK_LWIN
                            | VK_RWIN
                    ) {
                        KEYEVENTF_EXTENDEDKEY
                    } else {
                        0
                    },
                time: 0,
                dwExtraInfo: INJECTED_TAG,
            },
        },
    }
}

fn shortcut_inputs(keys: &[u16], scan: Option<u16>, up: bool) -> Vec<INPUT> {
    let mut inputs: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            if let Some(scan) = scan.filter(|_| index + 1 == keys.len()) {
                INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT {
                            wVk: 0,
                            wScan: scan & 0xff,
                            dwFlags: KEYEVENTF_SCANCODE
                                | if scan & 0xff00 == 0xe000 {
                                    KEYEVENTF_EXTENDEDKEY
                                } else {
                                    0
                                }
                                | if up { KEYEVENTF_KEYUP } else { 0 },
                            time: 0,
                            dwExtraInfo: INJECTED_TAG,
                        },
                    },
                }
            } else {
                key_input(*key, up)
            }
        })
        .collect();
    if up {
        inputs.reverse();
    }
    inputs
}

fn perform(action: Action) -> Result<(), String> {
    if let Action::LaunchApplication { path } = action {
        // Filesystem validation and process creation can block; never delay the mouse hook.
        return thread::Builder::new()
            .name("qz-launch".into())
            .spawn(move || {
                let result = validate_application(&path).and_then(|_| {
                    std::process::Command::new(&path)
                        .spawn()
                        .map(|_| ())
                        .map_err(|e| format!("Could not launch application: {e}"))
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
    let keys = match &action {
        Action::Disable => return Ok(()),
        Action::Default => return Err("Default actions must pass through unchanged.".into()),
        Action::Minimize | Action::Maximize => {
            unsafe {
                let window = GetForegroundWindow();
                if window.is_null() {
                    return Err("No foreground window is available.".into());
                }
                if ShowWindowAsync(
                    window,
                    if action == Action::Minimize {
                        SW_MINIMIZE
                    } else if IsZoomed(window) != 0 {
                        SW_RESTORE
                    } else {
                        SW_MAXIMIZE
                    },
                ) == 0
                {
                    return Err("Windows could not change the foreground window.".into());
                }
            }
            return Ok(());
        }
        _ => shortcut(&action)?,
    };
    let scan = if let Action::CustomShortcut { key, .. } = &action {
        Some(custom_scan(key).ok_or("This physical shortcut key is unsupported.")?)
    } else {
        None
    };
    let mut keyboard = INJECTED_KEYS.lock().unwrap_or_else(|e| e.into_inner());
    // Pending keys must already be manually released; delayed key-ups could release physical keys.
    if let Err(error) = keyboard.recover(&mut injected_key_is_up) {
        drop(keyboard);
        return keyboard_result(Err(error));
    }
    unsafe {
        let mut held_keys = keys.clone();
        if let Some(scan) = scan {
            let foreground_thread = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
            let key = MapVirtualKeyExW(
                scan as u32,
                MAPVK_VSC_TO_VK_EX,
                GetKeyboardLayout(foreground_thread),
            );
            if key == 0 {
                return Err("The active keyboard layout does not support this key.".into());
            }
            *held_keys.last_mut().ok_or("The shortcut has no key.")? = key as u16;
        }
        // Never release a modifier that the person is physically holding.
        if [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .chain(held_keys.iter())
            .any(|key| GetAsyncKeyState(*key as i32) < 0)
        {
            return Err(
                "Release Ctrl, Alt, Shift and Windows keys before using a mapped button.".into(),
            );
        }
        let mut inputs = shortcut_inputs(&keys, scan, false);
        inputs.extend(shortcut_inputs(&keys, scan, true));
        let result = keyboard.send(&inputs, &mut send_inputs, &mut injected_key_is_up);
        drop(keyboard);
        keyboard_result(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_only_the_observed_receiver_pair() {
        assert!(compatible_path(r"\\?\HID#VID_276D&PID_1902&MI_01"));
        assert!(!compatible_path(r"\\?\HID#VID_276D&PID_1903"));
        assert!(!compatible_path(r"\\?\HID#VID_276D&PID_19020"));
        assert!(!compatible_path(""));
    }
    #[test]
    fn copy_and_paste_use_control_and_distinct_keys() {
        assert_eq!(
            shortcut(&Action::Copy).unwrap(),
            vec![VK_CONTROL, b'C' as u16]
        );
        assert_eq!(
            shortcut(&Action::Paste).unwrap(),
            vec![VK_CONTROL, b'V' as u16]
        );
        assert!(shortcut(&Action::Default).unwrap().is_empty());
    }
    #[test]
    fn custom_shortcuts_validate_keys_and_modifiers() {
        assert_eq!(
            shortcut(&Action::CustomShortcut {
                key: "KeyK".into(),
                modifiers: vec!["control".into(), "shift".into()]
            })
            .unwrap(),
            vec![VK_CONTROL, VK_SHIFT, b'K' as u16]
        );
        for key in ["Keya", "F13", "F01", "Shell", ""] {
            assert!(custom_key(key).is_none());
        }
        for key in ["KeyZ", "Digit0", "F12", "Insert", "Slash"] {
            assert!(custom_key(key).is_some());
        }
        assert!(validate_action(&Action::CustomShortcut {
            key: "KeyK".into(),
            modifiers: vec!["control".into(), "control".into()]
        })
        .is_err());
        assert!(validate_action(&Action::CustomShortcut {
            key: "KeyK".into(),
            modifiers: vec!["shell".into()]
        })
        .is_err());
        assert!(validate_application("calc.exe").is_err());
        assert!(validate_application(r"C:\Windows\System32\cmd.exe /c echo bad").is_err());
        assert!(validate_application(std::env::current_exe().unwrap().to_str().unwrap()).is_ok());
    }
    #[test]
    fn desktop_media_and_tab_actions_use_distinct_native_keys() {
        assert_eq!(
            shortcut(&Action::PreviousDesktop).unwrap(),
            vec![VK_CONTROL, VK_LWIN, VK_LEFT]
        );
        assert_eq!(
            shortcut(&Action::NextDesktop).unwrap(),
            vec![VK_CONTROL, VK_LWIN, VK_RIGHT]
        );
        assert_eq!(
            shortcut(&Action::PreviousTab).unwrap(),
            vec![VK_CONTROL, VK_SHIFT, VK_TAB]
        );
        assert_eq!(
            shortcut(&Action::NextTab).unwrap(),
            vec![VK_CONTROL, VK_TAB]
        );
        assert_eq!(
            shortcut(&Action::PlayPause).unwrap(),
            vec![VK_MEDIA_PLAY_PAUSE]
        );
        assert_eq!(shortcut(&Action::Mute).unwrap(), vec![VK_VOLUME_MUTE]);
        assert!(shortcut(&Action::Disable).unwrap().is_empty());
        assert!(shortcut(&Action::Minimize).is_err());
        assert!(validate_action(&Action::Minimize).is_ok());
        for letter in b'A'..=b'Z' {
            assert_eq!(
                custom_key(&format!("Key{}", letter as char)),
                Some(letter as u16)
            );
        }
    }
    #[test]
    fn custom_physical_keys_pair_scan_code_presses_and_releases() {
        assert_eq!(custom_scan("KeyY"), Some(0x15));
        assert_eq!(custom_scan("KeyZ"), Some(0x2c));
        assert_eq!(custom_scan("Semicolon"), Some(0x27));
        assert_eq!(custom_scan("Slash"), Some(0x35));
        assert_eq!(custom_scan("ArrowLeft"), Some(0xe04b));
        assert!(custom_scan("NumpadEnter").is_none());
        let keys = [VK_CONTROL, VK_LEFT];
        let down = shortcut_inputs(&keys, Some(0xe04b), false);
        let up = shortcut_inputs(&keys, Some(0xe04b), true);
        unsafe {
            assert_eq!(down[0].Anonymous.ki.wVk, VK_CONTROL);
            assert_eq!(down[1].Anonymous.ki.wVk, 0);
            assert_eq!(down[1].Anonymous.ki.wScan, 0x4b);
            assert_eq!(
                down[1].Anonymous.ki.dwFlags,
                KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY
            );
            assert_eq!(up[0].Anonymous.ki.wScan, 0x4b);
            assert_eq!(
                up[0].Anonymous.ki.dwFlags,
                KEYEVENTF_SCANCODE | KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP
            );
            assert_eq!(up[1].Anonymous.ki.wVk, VK_CONTROL);
            assert_eq!(up[1].Anonymous.ki.dwFlags, KEYEVENTF_KEYUP);
        }
    }
    #[test]
    fn injection_failure_pauses_actual_engine_without_changing_assignments() {
        let shared = Shared::default();
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        pause_keyboard_error(&shared, "Pending key release");
        let engine = shared.engine.lock().unwrap();
        assert!(engine.paused);
        assert!(!engine.enabled);
        assert_eq!(engine.mappings.back, Action::Copy);
        assert_eq!(engine.error.as_deref(), Some("Pending key release"));
    }
}
