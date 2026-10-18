use crate::{
    engine::{Action, Input},
    Device, Shared,
};
mod trace;
use std::{
    mem::size_of,
    ptr::null_mut,
    sync::{atomic::Ordering, mpsc, Arc, OnceLock},
    thread::{self, JoinHandle},
};
pub use trace::trace_buttons;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_NO_MORE_FILES, INVALID_HANDLE_VALUE, LPARAM, LRESULT,
        WPARAM,
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

fn shortcut(action: Action) -> Vec<u16> {
    match action {
        Action::Default => vec![],
        Action::Copy => vec![VK_CONTROL, b'C' as u16],
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
    }
}

fn key_input(key: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: INJECTED_TAG,
            },
        },
    }
}

fn perform(action: Action) -> Result<(), String> {
    let keys = shortcut(action);
    unsafe {
        // Never release a modifier that the person is physically holding.
        if [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .chain(keys.iter())
            .any(|key| GetAsyncKeyState(*key as i32) < 0)
        {
            return Err(
                "Release Ctrl, Alt, Shift and Windows keys before using a mapped button.".into(),
            );
        }
        let mut inputs: Vec<_> = keys.iter().map(|key| key_input(*key, false)).collect();
        inputs.extend(keys.iter().rev().map(|key| key_input(*key, true)));
        let sent = SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        );
        if sent != inputs.len() as u32 {
            if sent > 0 {
                let releases: Vec<_> = keys.iter().rev().map(|key| key_input(*key, true)).collect();
                SendInput(
                    releases.len() as u32,
                    releases.as_ptr(),
                    size_of::<INPUT>() as i32,
                );
            }
            return Err(
                "Windows rejected the shortcut. An elevated application may be blocking input."
                    .into(),
            );
        }
    }
    Ok(())
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
        assert_eq!(shortcut(Action::Copy), vec![VK_CONTROL, b'C' as u16]);
        assert_eq!(shortcut(Action::Paste), vec![VK_CONTROL, b'V' as u16]);
        assert!(shortcut(Action::Default).is_empty());
    }
}
