use super::compatible_path;
use crate::engine::Input;
use serde_json::json;
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::*, WindowsAndMessaging::*},
};

unsafe fn name(handle: HANDLE) -> Option<String> {
    let mut count = 0;
    if GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, null_mut(), &mut count) == u32::MAX {
        return None;
    }
    let mut text = vec![0u16; count as usize + 1];
    if GetRawInputDeviceInfoW(
        handle,
        RIDI_DEVICENAME,
        text.as_mut_ptr().cast(),
        &mut count,
    ) == u32::MAX
    {
        return None;
    }
    Some(String::from_utf16_lossy(&text).to_ascii_uppercase())
}

fn hid_reports(body: &[u8]) -> Option<Vec<String>> {
    let size = u32::from_le_bytes(body.get(..4)?.try_into().ok()?) as usize;
    let count = u32::from_le_bytes(body.get(4..8)?.try_into().ok()?) as usize;
    if size == 0 || size > 64 || count > 32 {
        return None;
    }
    let data = body.get(8..8usize.checked_add(size.checked_mul(count)?)?)?;
    Some(
        data.chunks_exact(size)
            .map(|report| report.iter().map(|byte| format!("{byte:02x}")).collect())
            .collect(),
    )
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            let mut size = 0;
            let header_size = size_of::<RAWINPUTHEADER>() as u32;
            let input = lp as HRAWINPUT;
            if GetRawInputData(input, RID_INPUT, null_mut(), &mut size, header_size) != u32::MAX
                && size >= header_size
                && size <= 4096
            {
                let mut storage = vec![0u64; (size as usize).div_ceil(8)];
                let received = GetRawInputData(
                    input,
                    RID_INPUT,
                    storage.as_mut_ptr().cast(),
                    &mut size,
                    header_size,
                );
                if received != u32::MAX
                    && received as usize <= storage.len() * 8
                    && received >= header_size
                {
                    let header = &*(storage.as_ptr() as *const RAWINPUTHEADER);
                    if name(header.hDevice).is_some_and(|path| compatible_path(&path)) {
                        let bytes = std::slice::from_raw_parts(
                            storage.as_ptr().cast::<u8>(),
                            received as usize,
                        );
                        let body = &bytes[header_size as usize..];
                        if header.dwType == RIM_TYPEMOUSE && body.len() >= size_of::<RAWMOUSE>() {
                            let mouse = std::ptr::read_unaligned(body.as_ptr().cast::<RAWMOUSE>());
                            let buttons = mouse.Anonymous.Anonymous;
                            if buttons.usButtonFlags != 0 {
                                println!(
                                    "{}",
                                    json!({"kind":"mouse","flags":buttons.usButtonFlags,"data":buttons.usButtonData as i16,"raw_buttons":mouse.ulRawButtons})
                                );
                            }
                        } else if header.dwType == RIM_TYPEHID {
                            if let Some(reports) = hid_reports(body) {
                                for report in reports {
                                    let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                                    if state != 0 {
                                        let mut thumb_down = state & 2 != 0;
                                        if let (Some(shared), Some((input, down))) = (
                                            super::SHARED.get(),
                                            receiver_event(&report, &mut thumb_down),
                                        ) {
                                            shared.process(input, down, super::perform);
                                        }
                                        SetWindowLongPtrW(
                                            hwnd,
                                            GWLP_USERDATA,
                                            if thumb_down { 3 } else { 1 },
                                        );
                                    } else {
                                        println!(
                                            "{}",
                                            json!({"kind":"vendor_hid","report":report})
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        WM_TIMER => {
            DestroyWindow(hwnd);
            return 0;
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// shortcut: accept only complete frames captured from this receiver; extend with new hardware evidence.
fn side_wheel(report: &str) -> Option<Input> {
    match report {
        "bb04000000010017" | "bb04000000020014" => Some(Input::WheelRight),
        "bb04000000ff00e9" | "bb04000000fe00e8" => Some(Input::WheelLeft),
        _ => None,
    }
}

fn receiver_event(report: &str, thumb_down: &mut bool) -> Option<(Input, bool)> {
    match report {
        "bb04000400000012" if !*thumb_down => {
            *thumb_down = true;
            Some((Input::Thumb, true))
        }
        "bb04000000000016" if *thumb_down => {
            *thumb_down = false;
            Some((Input::Thumb, false))
        }
        _ => side_wheel(report).map(|input| (input, true)),
    }
}

pub(super) unsafe fn vendor_window() -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    let class: Vec<u16> = "QzFlowReceiverInput\0".encode_utf16().collect();
    let definition = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        ..Default::default()
    };
    if RegisterClassW(&definition) == 0 && windows_sys::Win32::Foundation::GetLastError() != 1410 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        class.as_ptr(),
        0,
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        null_mut(),
        instance,
        null(),
    );
    if hwnd.is_null() {
        return Err(std::io::Error::last_os_error().to_string());
    }
    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1);
    let registrations =
        [(0xffbb, 1), (0xffbc, 136), (0xffb5, 1)].map(|(page, usage)| RAWINPUTDEVICE {
            usUsagePage: page,
            usUsage: usage,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        });
    if RegisterRawInputDevices(
        registrations.as_ptr(),
        registrations.len() as u32,
        size_of::<RAWINPUTDEVICE>() as u32,
    ) == 0
    {
        DestroyWindow(hwnd);
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(hwnd)
}

pub fn trace_buttons() -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class: Vec<u16> = "QzFlowReadOnlyTrace\0".encode_utf16().collect();
        let definition = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&definition) == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut registrations = vec![RAWINPUTDEVICE {
            usUsagePage: 1,
            usUsage: 2,
            dwFlags: RIDEV_INPUTSINK,
            hwndTarget: hwnd,
        }];
        let mut count = 0;
        let entry_size = size_of::<RAWINPUTDEVICELIST>() as u32;
        if GetRawInputDeviceList(null_mut(), &mut count, entry_size) == u32::MAX {
            DestroyWindow(hwnd);
            return Err("Could not enumerate inputs.".into());
        }
        let mut entries = vec![RAWINPUTDEVICELIST::default(); count as usize];
        let found = GetRawInputDeviceList(entries.as_mut_ptr(), &mut count, entry_size);
        if found == u32::MAX {
            DestroyWindow(hwnd);
            return Err("Device list changed; try again.".into());
        }
        for device in entries.into_iter().take(found as usize) {
            if device.dwType != RIM_TYPEHID
                || !name(device.hDevice).is_some_and(|path| compatible_path(&path))
            {
                continue;
            }
            let mut info = RID_DEVICE_INFO {
                cbSize: size_of::<RID_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            let mut size = info.cbSize;
            if GetRawInputDeviceInfoW(
                device.hDevice,
                RIDI_DEVICEINFO,
                (&mut info as *mut RID_DEVICE_INFO).cast(),
                &mut size,
            ) == u32::MAX
            {
                continue;
            }
            let hid = info.Anonymous.hid;
            println!(
                "{}",
                json!({"kind":"collection","usage_page":hid.usUsagePage,"usage":hid.usUsage})
            );
            // Observe receiver consumer controls, but never register keyboard input.
            if (hid.usUsagePage >= 0xff00 || hid.usUsagePage == 12)
                && !registrations
                    .iter()
                    .any(|r| r.usUsagePage == hid.usUsagePage && r.usUsage == hid.usUsage)
            {
                registrations.push(RAWINPUTDEVICE {
                    usUsagePage: hid.usUsagePage,
                    usUsage: hid.usUsage,
                    dwFlags: RIDEV_INPUTSINK,
                    hwndTarget: hwnd,
                });
            }
        }
        if RegisterRawInputDevices(
            registrations.as_ptr(),
            registrations.len() as u32,
            size_of::<RAWINPUTDEVICE>() as u32,
        ) == 0
        {
            DestroyWindow(hwnd);
            return Err(std::io::Error::last_os_error().to_string());
        }
        if SetTimer(hwnd, 1, 45000, None) == 0 {
            DestroyWindow(hwnd);
            return Err("Could not start capture deadline.".into());
        }
        println!(
            "{}",
            json!({"trace":"ready","seconds":45,"scope":"276d:1902 mouse buttons, consumer controls and vendor HID; no keyboard or pointer coordinates"})
        );
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        println!("{}", json!({"trace":"finished"}));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumb_edges_are_paired_and_neutral_wheel_frames_are_not_thumb_presses() {
        let mut held = false;
        assert_eq!(receiver_event("bb04000000000016", &mut held), None);
        assert_eq!(
            receiver_event("bb04000400000012", &mut held),
            Some((Input::Thumb, true))
        );
        assert_eq!(receiver_event("bb04000400000012", &mut held), None);
        assert_eq!(receiver_event("bb04000400000013", &mut held), None);
        assert!(held);
        assert_eq!(
            receiver_event("bb04000000000016", &mut held),
            Some((Input::Thumb, false))
        );
        assert!(!held);
    }
    #[test]
    fn only_observed_side_wheel_frames_generate_actions() {
        assert_eq!(side_wheel("bb04000000010017"), Some(Input::WheelRight));
        assert_eq!(side_wheel("bb04000000ff00e9"), Some(Input::WheelLeft));
        assert_eq!(side_wheel("bb04000000000016"), None);
        assert_eq!(side_wheel("bb04000000010018"), None);
        assert_eq!(side_wheel(""), None);
    }
    #[test]
    fn hid_packet_bounds_are_checked_before_reading() {
        assert!(hid_reports(&[]).is_none());
        assert!(hid_reports(&[2, 0, 0, 0, 2, 0, 0, 0, 1, 2]).is_none());
        assert!(hid_reports(&[0, 0, 0, 0, 1, 0, 0, 0]).is_none());
        assert_eq!(
            hid_reports(&[2, 0, 0, 0, 2, 0, 0, 0, 1, 2, 3, 4]).unwrap(),
            vec!["0102", "0304"]
        );
    }
}
