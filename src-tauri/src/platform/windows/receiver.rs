use std::{
    mem::size_of,
    ptr::{null, null_mut},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
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

struct Receiver(HANDLE);

#[derive(Debug)]
pub(super) struct ConfigurationError {
    pub message: String,
    pub reset_needed: bool,
}

impl From<String> for ConfigurationError {
    fn from(message: String) -> Self {
        Self {
            message,
            reset_needed: false,
        }
    }
}

impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

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
    fn open() -> Result<Self, String> {
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
                if (
                    hid.dwVendorId,
                    hid.dwProductId,
                    hid.usUsagePage,
                    hid.usUsage,
                ) != (0x276d, 0x1902, 0xffbb, 1)
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
                return Err("Connect exactly one QZ30 USB receiver to configure button 4.".into());
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

trait FeatureIo {
    fn write(&mut self, report: [u8; 8]) -> Result<(), String>;
    fn read(&mut self) -> Result<[u8; 8], String>;
    fn wait(&mut self, milliseconds: u64);
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

    fn wait(&mut self, milliseconds: u64) {
        thread::sleep(Duration::from_millis(milliseconds));
    }
}

fn packet(payload: [u8; 6]) -> [u8; 8] {
    let mut report = [0xbb, 0, 0, 0, 0, 0, 0, 0x12];
    report[1..7].copy_from_slice(&payload);
    report[7] = payload
        .into_iter()
        .fold(0x12, |checksum, byte| checksum ^ byte);
    report
}

fn valid(report: &[u8; 8]) -> bool {
    report[0] == 0xbb
        && report[1] < 0x80
        && report[1..7].iter().fold(0x12, |sum, byte| sum ^ byte) == report[7]
}

pub(super) fn thumb_mode_active() -> Result<bool, String> {
    let report = Receiver::open()?.read()?;
    acknowledged_mode(&report)
}

fn acknowledged_mode(report: &[u8; 8]) -> Result<bool, String> {
    if valid(report) && mask_matches(report, true) {
        Ok(true)
    } else if valid(report) && mask_matches(report, false) {
        Ok(false)
    } else {
        Err("Receiver mode acknowledgement unavailable; reconnect the receiver before starting button 4 mappings.".into())
    }
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("QZ30 receiver configuration cancelled.".into())
    } else {
        Ok(())
    }
}

fn poll(
    io: &mut impl FeatureIo,
    cancel: &AtomicBool,
    matches: impl Fn(&[u8; 8]) -> bool,
) -> Result<(), String> {
    for _ in 0..40 {
        cancelled(cancel)?;
        let report = io.read()?;
        if valid(&report) && matches(&report) {
            return Ok(());
        }
        io.wait(50);
    }
    Err("QZ30 receiver acknowledgement timed out.".into())
}

fn exchange(
    io: &mut impl FeatureIo,
    cancel: &AtomicBool,
    report: [u8; 8],
    matches: impl Fn(&[u8; 8]) -> bool,
) -> Result<(), String> {
    cancelled(cancel)?;
    io.write(report)?;
    io.wait(100);
    poll(io, cancel, matches)
}

fn mask_report(enabled: bool) -> [u8; 8] {
    packet([2, 0, 0, 0, if enabled { 4 } else { 0 }, 0])
}

fn mask_matches(report: &[u8; 8], enabled: bool) -> bool {
    report == &mask_report(enabled)
}

fn configure(
    io: &mut impl FeatureIo,
    enabled: bool,
    cancel: &AtomicBool,
    nonce: [u8; 2],
) -> Result<(), ConfigurationError> {
    poll(io, cancel, |_| true)?;
    if !enabled {
        cancelled(cancel)?;
        return exchange(io, &AtomicBool::new(false), mask_report(false), |r| {
            mask_matches(r, false)
        })
        .map_err(Into::into);
    }
    let a = nonce[0].wrapping_add(nonce[1]) ^ 0x55;
    let b = nonce[1].wrapping_add(a) ^ 0x11;
    exchange(io, cancel, packet([0, 1, 2, 3, nonce[0], nonce[1]]), |r| {
        r[1] == 0 && r[2] == 1 && r[5] == a && r[6] == b
    })?;
    exchange(io, cancel, packet([1, 0, 0, 0, 0, 0]), |r| r[1] == 1)?;
    cancelled(cancel)?;
    // A failed write may still reach the device; attempt restoration even after cancellation.
    let result = io.write(mask_report(true)).and_then(|()| {
        io.wait(100);
        poll(io, cancel, |r| mask_matches(r, true))
    });
    if let Err(error) = result {
        let cleanup = exchange(io, &AtomicBool::new(false), mask_report(false), |r| {
            mask_matches(r, false)
        });
        return Err(match cleanup {
            Ok(()) => error.into(),
            Err(reset) => ConfigurationError {
                message: format!(
                    "{error} Receiver reset also failed: {reset}. Reconnect the receiver."
                ),
                reset_needed: true,
            },
        });
    }
    Ok(())
}

pub(super) fn set_thumb_mode(enabled: bool, cancel: &AtomicBool) -> Result<(), ConfigurationError> {
    cancelled(cancel)?;
    let mut receiver = Receiver::open()?;
    // OEM nonce checks the reply transformation; it is not a security secret.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .subsec_nanos()
        .to_le_bytes();
    configure(&mut receiver, enabled, cancel, [nonce[0], nonce[1]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Fake {
        reads: VecDeque<[u8; 8]>,
        writes: Vec<[u8; 8]>,
        fail_enable: bool,
        cancel_on_enable: Option<std::sync::Arc<AtomicBool>>,
    }

    impl FeatureIo for Fake {
        fn read(&mut self) -> Result<[u8; 8], String> {
            Ok(self.reads.pop_front().unwrap_or([0; 8]))
        }
        fn write(&mut self, report: [u8; 8]) -> Result<(), String> {
            self.writes.push(report);
            if report == mask_report(true) {
                if let Some(cancel) = &self.cancel_on_enable {
                    cancel.store(true, Ordering::Release);
                }
            }
            if self.fail_enable && report == mask_report(true) {
                Err("simulated write failure".into())
            } else {
                Ok(())
            }
        }
        fn wait(&mut self, _: u64) {}
    }

    fn initialized() -> Fake {
        Fake {
            reads: [
                [0xbb, 0, 1, 2, 3, 0xb7, 0x2e, 0x8b],
                packet([0, 1, 2, 3, 0xf8, 0xc2]),
                packet([1, 1, 1, 15, 0, 0]),
            ]
            .into(),
            ..Default::default()
        }
    }

    #[test]
    fn verified_oem_packets_and_challenge() {
        assert_eq!(mask_report(true), [0xbb, 2, 0, 0, 0, 4, 0, 0x14]);
        assert_eq!(mask_report(false), [0xbb, 2, 0, 0, 0, 0, 0, 0x10]);
        let mut io = initialized();
        io.reads.push_back(mask_report(true));
        configure(&mut io, true, &AtomicBool::new(false), [0xd2, 0xdb]).unwrap();
        assert_eq!(
            io.writes,
            vec![
                packet([0, 1, 2, 3, 0xd2, 0xdb]),
                packet([1, 0, 0, 0, 0, 0]),
                mask_report(true)
            ]
        );
    }

    #[test]
    fn retries_busy_bad_checksum_and_stale_ack() {
        let mut io = Fake::default();
        let mut bad = mask_report(false);
        bad[7] ^= 1;
        io.reads.extend([
            [0xbb, 0x82, 0, 0, 0, 0, 0, 0],
            bad,
            mask_report(false),
            mask_report(true),
            mask_report(false),
        ]);
        configure(&mut io, false, &AtomicBool::new(false), [0, 0]).unwrap();
        assert_eq!(io.writes, vec![mask_report(false)]);
    }

    #[test]
    fn bad_challenge_never_changes_mask() {
        let mut io = Fake {
            reads: [mask_report(false)].into(),
            ..Default::default()
        };
        assert!(configure(&mut io, true, &AtomicBool::new(false), [0xd2, 0xdb]).is_err());
        assert_eq!(io.writes.len(), 1);
    }

    #[test]
    fn failed_enable_write_still_restores_default() {
        let mut io = initialized();
        io.fail_enable = true;
        io.reads.push_back(mask_report(false));
        let error = configure(&mut io, true, &AtomicBool::new(false), [0xd2, 0xdb]).unwrap_err();
        assert!(!error.reset_needed);
        assert_eq!(io.writes.last(), Some(&mask_report(false)));
    }

    #[test]
    fn cancelled_before_configuration_never_writes() {
        let mut io = initialized();
        assert!(configure(&mut io, true, &AtomicBool::new(true), [0xd2, 0xdb]).is_err());
        assert!(io.writes.is_empty());
    }

    #[test]
    fn cancellation_after_enable_still_restores_default() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let mut io = initialized();
        io.cancel_on_enable = Some(cancel.clone());
        io.reads.push_back(mask_report(false));
        assert!(configure(&mut io, true, &cancel, [0xd2, 0xdb]).is_err());
        assert_eq!(io.writes.last(), Some(&mask_report(false)));
    }

    #[test]
    fn failed_cleanup_is_reported() {
        let mut io = initialized();
        io.fail_enable = true;
        let error = configure(&mut io, true, &AtomicBool::new(false), [0xd2, 0xdb]).unwrap_err();
        assert!(error.reset_needed);
        assert!(error.message.contains("Receiver reset also failed"));
        assert!(error.message.contains("Reconnect the receiver"));
    }

    #[test]
    fn only_exact_mode_acknowledgements_establish_mode() {
        assert_eq!(acknowledged_mode(&mask_report(true)), Ok(true));
        assert_eq!(acknowledged_mode(&mask_report(false)), Ok(false));
        assert!(acknowledged_mode(&packet([1, 1, 1, 15, 0, 0])).is_err());
        assert!(acknowledged_mode(&packet([0x82, 0, 0, 0, 0, 0])).is_err());
        let mut bad = mask_report(false);
        bad[7] ^= 1;
        assert!(acknowledged_mode(&bad).is_err());
    }
}
