use crate::platform::{
    thumb::{FeatureIo, AMBIGUOUS},
    COMPATIBLE_IDS,
};
use std::{
    mem::size_of,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Devices::HumanInterfaceDevice::{
        HidD_FreePreparsedData, HidD_GetFeature, HidD_GetPreparsedData, HidD_SetFeature,
        HidP_GetCaps, HIDP_CAPS, HIDP_STATUS_SUCCESS,
    },
    Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    UI::Input::{
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RIDI_DEVICEINFO,
        RIDI_DEVICENAME, RID_DEVICE_INFO, RIM_TYPEHID,
    },
};

pub(super) struct Receiver(HANDLE);

impl Drop for Receiver {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn os_error(operation: &str) -> String {
    format!(
        "QZ30 receiver {operation}: {}",
        std::io::Error::last_os_error()
    )
}

impl Receiver {
    pub(super) fn open() -> Result<Self, String> {
        unsafe {
            let mut count = 0;
            let size = size_of::<RAWINPUTDEVICELIST>() as u32;
            if GetRawInputDeviceList(null_mut(), &mut count, size) == u32::MAX {
                return Err(os_error("inventory failed"));
            }
            let mut list = vec![RAWINPUTDEVICELIST::default(); count as usize];
            let found = GetRawInputDeviceList(list.as_mut_ptr(), &mut count, size);
            if found == u32::MAX {
                return Err(os_error("inventory changed"));
            }
            let mut paths = Vec::new();
            for item in list.into_iter().take(found as usize) {
                if item.dwType != RIM_TYPEHID {
                    continue;
                }
                let mut info = RID_DEVICE_INFO {
                    cbSize: size_of::<RID_DEVICE_INFO>() as u32,
                    ..Default::default()
                };
                let mut length = info.cbSize;
                if GetRawInputDeviceInfoW(
                    item.hDevice,
                    RIDI_DEVICEINFO,
                    (&mut info as *mut RID_DEVICE_INFO).cast(),
                    &mut length,
                ) == u32::MAX
                    || info.dwType != RIM_TYPEHID
                {
                    continue;
                }
                let hid = info.Anonymous.hid;
                // The vendor collection ffbb:1 of the USB receiver or of the Bluetooth LE mouse.
                if !COMPATIBLE_IDS.iter().any(|&(vendor, product)| {
                    (vendor as u32, product as u32) == (hid.dwVendorId, hid.dwProductId)
                }) || (hid.usUsagePage, hid.usUsage) != (0xffbb, 1)
                {
                    continue;
                }
                length = 0;
                if GetRawInputDeviceInfoW(item.hDevice, RIDI_DEVICENAME, null_mut(), &mut length)
                    == u32::MAX
                {
                    return Err(os_error("path lookup failed"));
                }
                let mut path = vec![0u16; length as usize + 1];
                if GetRawInputDeviceInfoW(
                    item.hDevice,
                    RIDI_DEVICENAME,
                    path.as_mut_ptr().cast(),
                    &mut length,
                ) == u32::MAX
                {
                    return Err(os_error("path lookup failed"));
                }
                paths.push(path);
            }
            if paths.len() != 1 {
                return Err(AMBIGUOUS.into());
            }
            let handle = CreateFileW(
                paths[0].as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            );
            if handle == INVALID_HANDLE_VALUE {
                return Err(os_error("open failed"));
            }
            let receiver = Self(handle);
            let mut preparsed = 0;
            if !HidD_GetPreparsedData(handle, &mut preparsed) {
                return Err(os_error("descriptor lookup failed"));
            }
            let mut caps = HIDP_CAPS::default();
            let status = HidP_GetCaps(preparsed, &mut caps);
            HidD_FreePreparsedData(preparsed);
            if status != HIDP_STATUS_SUCCESS
                || caps.UsagePage != 0xffbb
                || caps.Usage != 1
                || caps.FeatureReportByteLength != 8
            {
                return Err(
                    "Unsupported QZ30 receiver feature descriptor; no command sent.".into(),
                );
            }
            Ok(receiver)
        }
    }
}

impl FeatureIo for Receiver {
    fn write(&mut self, report: [u8; 8]) -> Result<(), String> {
        if unsafe { HidD_SetFeature(self.0, report.as_ptr().cast(), 8) } {
            Ok(())
        } else {
            Err(os_error("feature write failed"))
        }
    }

    fn read(&mut self) -> Result<[u8; 8], String> {
        let mut report = [0; 8];
        report[0] = 0xbb;
        if unsafe { HidD_GetFeature(self.0, report.as_mut_ptr().cast(), 8) } {
            Ok(report)
        } else {
            Err(os_error("feature read failed"))
        }
    }
}
