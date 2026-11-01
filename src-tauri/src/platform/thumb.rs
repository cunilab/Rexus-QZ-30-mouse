//! Button 4 (thumb rest) input mode, shared by every OS: the OEM feature-report handshake that makes the
//! mouse send vendor input frames, the frames themselves, and the worker that keeps the mode in sync with
//! the saved mapping. Each OS supplies only `FeatureIo` (report 0xBB, 8 bytes) and a reader for the frames.
use crate::{
    engine::{Action, Input},
    Shared,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// Bluetooth LE reports this all-zero buffer while idle; it fails the checksum but is not busy.
const BLE_IDLE: [u8; 8] = [0xbb, 0, 0, 0, 0, 0, 0, 0];
const THUMB_PRESS: [u8; 8] = [0xbb, 4, 0, 4, 0, 0, 0, 0x12];
const THUMB_RELEASE: [u8; 8] = [0xbb, 4, 0, 0, 0, 0, 0, 0x16];
pub(super) const AMBIGUOUS: &str =
    "Connect exactly one QZ30 USB receiver or Bluetooth mouse to configure button 4.";

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

pub(super) trait FeatureIo {
    fn write(&mut self, report: [u8; 8]) -> Result<(), String>;
    fn read(&mut self) -> Result<[u8; 8], String>;
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

// The vendor input frame for button 4: press, then release (only complete frames seen on hardware).
pub(super) fn thumb_edge(report: &[u8], held: &mut bool) -> Option<(Input, bool)> {
    if report == THUMB_PRESS && !*held {
        *held = true;
        Some((Input::Thumb, true))
    } else if report == THUMB_RELEASE && *held {
        *held = false;
        Some((Input::Thumb, false))
    } else {
        None
    }
}

// True when a HID report descriptor opens the vendor collection ffbb:1 (long items are skipped).
#[cfg(not(windows))]
pub(super) fn has_vendor_collection(descriptor: &[u8]) -> bool {
    let (mut page, mut usage, mut index) = (0u32, 0u32, 0);
    while let Some(&prefix) = descriptor.get(index) {
        if prefix == 0xfe {
            index += 3 + descriptor.get(index + 1).map_or(0, |&n| n as usize);
            continue;
        }
        let size = [0, 1, 2, 4][(prefix & 3) as usize];
        let Some(data) = descriptor.get(index + 1..index + 1 + size) else {
            return false;
        };
        let value = data.iter().rev().fold(0, |v, &b| v << 8 | b as u32);
        match prefix & 0xfc {
            0x04 => page = value,
            0x08 => usage = value,
            0xa0 if (page, usage) == (0xffbb, 1) => return true,
            _ => {}
        }
        if prefix & 0x0c == 0 {
            usage = 0; // Local items end with each main item.
        }
        index += 1 + size;
    }
    false
}

pub(super) fn acknowledged_mode(report: &[u8; 8]) -> Result<bool, String> {
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
        if (valid(&report) || report == BLE_IDLE) && matches(&report) {
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

fn set_thumb_mode<I: FeatureIo>(
    open: impl FnOnce() -> Result<I, String>,
    enabled: bool,
    cancel: &AtomicBool,
) -> Result<(), ConfigurationError> {
    cancelled(cancel)?;
    let mut io = open()?;
    // OEM nonce checks the reply transformation; it is not a security secret.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .subsec_nanos()
        .to_le_bytes();
    configure(&mut io, enabled, cancel, [nonce[0], nonce[1]])
}

// Keeps the device mode in step with the mapping on its own thread: HID requests can wait for the
// radio and must never run under the engine or UI locks.
pub(super) fn spawn_worker<I: FeatureIo + 'static>(
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    open: fn() -> Result<I, String>,
) -> Result<JoinHandle<()>, String> {
    thread::Builder::new()
        .name("qz-receiver".into())
        .spawn(move || {
            run_worker(
                &shared,
                &stop,
                |enabled, cancel| {
                    if enabled {
                        super::ensure_no_vendor_utility()?;
                    }
                    set_thumb_mode(open, enabled, cancel)
                },
                |owned_mode| {
                    if super::devices()?.is_empty() {
                        Ok(None)
                    } else if !owned_mode {
                        // The last feature reply need not be a mask before initialization.
                        Ok(Some(false))
                    } else {
                        acknowledged_mode(&open()?.read()?).map(Some)
                    }
                },
                Duration::from_secs(2),
            );
        })
        .map_err(|error| format!("Could not start receiver setup: {error}"))
}

fn run_worker(
    shared: &Shared,
    stop: &AtomicBool,
    mut configure: impl FnMut(bool, &AtomicBool) -> Result<(), ConfigurationError>,
    mut probe: impl FnMut(bool) -> Result<Option<bool>, String>,
    probe_interval: Duration,
) {
    let mut requested = false;
    let mut active = false;
    let mut connected = true;
    let mut next_probe = std::time::Instant::now();
    while !stop.load(Ordering::Acquire) {
        let thumb_requested = {
            let engine = shared.engine.lock().unwrap_or_else(|e| e.into_inner());
            !engine.paused && engine.mappings.thumb != Action::Default
        } && !shared.stopping.load(Ordering::Acquire);
        if !thumb_requested && !active {
            connected = true;
            next_probe = std::time::Instant::now();
        }
        if (thumb_requested || active) && std::time::Instant::now() >= next_probe {
            next_probe = std::time::Instant::now() + probe_interval;
            match probe(active) {
                Ok(None) => {
                    connected = false;
                    active = false;
                    requested = false;
                    shared.thumb_ready.store(false, Ordering::Release);
                    shared
                        .engine
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .enabled = false;
                }
                Ok(Some(mode_active)) => {
                    connected = true;
                    if active && !mode_active {
                        active = false;
                        requested = false;
                        shared.thumb_ready.store(false, Ordering::Release);
                    }
                }
                Err(error) => {
                    let mut engine = shared.engine.lock().unwrap_or_else(|e| e.into_inner());
                    engine.enabled = false;
                    engine.paused = true;
                    engine.error = Some(format!("Could not check button 4 receiver mode: {error}"));
                }
            }
        }
        if !connected {
            thread::park_timeout(Duration::from_millis(100));
            continue;
        }
        let desired = {
            let engine = shared.engine.lock().unwrap_or_else(|e| e.into_inner());
            engine.enabled && !engine.paused && engine.mappings.thumb != Action::Default
        } && !shared.stopping.load(Ordering::Acquire);
        if desired != requested {
            requested = desired;
            shared.thumb_ready.store(false, Ordering::Release);
            if desired || active {
                // HID requests can wait for the radio; never hold shared state across them.
                let result = configure(desired, stop);
                match result {
                    Ok(()) => {
                        active = desired;
                        shared.thumb_ready.store(active, Ordering::Release);
                    }
                    Err(error) => {
                        active |= error.reset_needed;
                        let disconnected =
                            !stop.load(Ordering::Acquire) && matches!(probe(false), Ok(None));
                        if disconnected {
                            connected = false;
                            active = false;
                            requested = false;
                            shared
                                .engine
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .enabled = false;
                        } else {
                            let mut engine =
                                shared.engine.lock().unwrap_or_else(|e| e.into_inner());
                            engine.enabled = false;
                            engine.paused = true;
                            engine.error = Some(format!("Button 4 receiver setup failed: {error}"));
                        }
                    }
                }
            }
        }
        thread::park_timeout(Duration::from_millis(100));
    }
    shared.thumb_ready.store(false, Ordering::Release);
    if active {
        if let Err(error) = configure(false, &AtomicBool::new(false)) {
            shared
                .engine
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .error = Some(format!("Could not restore button 4 default mode: {error}"));
        }
    }
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
    fn bluetooth_idle_report_does_not_block_the_exchange() {
        // Exchange measured over Bluetooth LE on macOS; the idle feature report is all zero after the ID.
        let mut io = Fake {
            reads: [
                BLE_IDLE,
                [0xbb, 0, 1, 2, 3, 0xf3, 0x1f, 0xfe],
                [0xbb, 1, 2, 1, 0x0f, 0, 0, 0x1f],
                mask_report(true),
            ]
            .into(),
            ..Default::default()
        };
        configure(&mut io, true, &AtomicBool::new(false), [0x8b, 0x1b]).unwrap();
        assert_eq!(
            io.writes,
            vec![
                [0xbb, 0, 1, 2, 3, 0x8b, 0x1b, 0x82],
                [0xbb, 1, 0, 0, 0, 0, 0, 0x13],
                [0xbb, 2, 0, 0, 0, 4, 0, 0x14]
            ]
        );
        // The idle report is accepted only as the starting state, never as an acknowledgement.
        let mut io = Fake {
            reads: [BLE_IDLE].into(),
            ..Default::default()
        };
        assert!(configure(&mut io, false, &AtomicBool::new(false), [0, 0]).is_err());
    }

    #[test]
    fn thumb_edges_are_paired_and_other_frames_are_ignored() {
        let mut held = false;
        assert_eq!(thumb_edge(&THUMB_RELEASE, &mut held), None);
        assert_eq!(
            thumb_edge(&THUMB_PRESS, &mut held),
            Some((Input::Thumb, true))
        );
        assert_eq!(thumb_edge(&THUMB_PRESS, &mut held), None);
        assert_eq!(thumb_edge(&[0xbb, 4, 0, 4, 0, 0, 0, 0x13], &mut held), None);
        assert_eq!(thumb_edge(&THUMB_PRESS[..7], &mut held), None);
        assert!(held);
        assert_eq!(
            thumb_edge(&THUMB_RELEASE, &mut held),
            Some((Input::Thumb, false))
        );
        assert!(!held);
        assert_eq!(THUMB_PRESS, packet([4, 0, 4, 0, 0, 0]));
        assert_eq!(THUMB_RELEASE, packet([4, 0, 0, 0, 0, 0]));
    }

    #[cfg(not(windows))]
    #[test]
    fn descriptor_check_finds_only_the_vendor_collection() {
        let vendor = [
            0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0xc0, // mouse
            0x06, 0xbb, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0xbb, 0x09, 0x01, 0xc0,
        ];
        assert!(has_vendor_collection(&vendor));
        // Page ffbb with another usage, another page with usage 1, and a usage consumed by a main item.
        assert!(!has_vendor_collection(&[
            0x06, 0xbb, 0xff, 0x09, 0x02, 0xa1, 0x01
        ]));
        assert!(!has_vendor_collection(&[
            0x06, 0xbc, 0xff, 0x09, 0x01, 0xa1, 0x01
        ]));
        assert!(!has_vendor_collection(&[
            0x06, 0xbb, 0xff, 0x09, 0x01, 0x81, 0x02, 0xa1, 0x01
        ]));
        assert!(!has_vendor_collection(&[0x06, 0xbb])); // truncated
        assert!(!has_vendor_collection(&[]));
        // A long item is skipped without being read as short items.
        assert!(has_vendor_collection(&[
            0xfe, 0x02, 0x10, 0x06, 0xbb, 0x06, 0xbb, 0xff, 0x09, 0x01, 0xa1, 0x01
        ]));
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

    #[test]
    fn receiver_bootstraps_before_requiring_an_owned_mask_acknowledgement() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let recorded_reply = [0xbb, 0, 1, 2, 3, 0xb7, 0x2e, 0x8b];
        let mut writes = Vec::new();
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                if enabled {
                    stop.store(true, Ordering::Release);
                }
                Ok(())
            },
            |owned_mode| {
                if owned_mode && recorded_reply[1] != 2 {
                    Err("last reply is not a mask".into())
                } else {
                    Ok(Some(false))
                }
            },
            Duration::ZERO,
        );
        assert_eq!(writes, [true, false]);
        assert!(shared.engine.lock().unwrap().error.is_none());
    }

    #[test]
    fn failed_enable_cleanup_is_retried_during_shutdown() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                if enabled {
                    stop.store(true, Ordering::Release);
                    Err(ConfigurationError {
                        message: "reset failed".into(),
                        reset_needed: true,
                    })
                } else {
                    Ok(())
                }
            },
            |_| Ok(Some(true)),
            Duration::from_secs(2),
        );
        assert_eq!(writes, [true, false]);
        assert!(shared
            .engine
            .lock()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("reset failed"));
    }

    #[test]
    fn unknown_probe_retains_reset_responsibility() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        let mut probes = 0;
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                if !enabled {
                    stop.store(true, Ordering::Release);
                }
                Ok(())
            },
            |_| {
                probes += 1;
                if probes == 1 {
                    Ok(Some(false))
                } else {
                    Err("busy or invalid response".into())
                }
            },
            Duration::ZERO,
        );
        assert_eq!(writes, [true, false]);
        assert!(shared.engine.lock().unwrap().paused);
    }

    #[test]
    fn default_thumb_never_requires_feature_interface_for_other_mappings() {
        let shared = Shared::default();
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.back = Action::Copy;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let delayed_stop = stop.clone();
        let current = thread::current();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            delayed_stop.store(true, Ordering::Release);
            current.unpark();
        });
        run_worker(
            &shared,
            &stop,
            |_, _| panic!("unexpected thumb configuration"),
            |_| panic!("unexpected feature probe"),
            Duration::ZERO,
        );
        stopper.join().unwrap();
        assert!(!shared.engine.lock().unwrap().paused);
    }

    #[test]
    fn receiver_reauthenticates_after_disconnect_without_pausing_preference() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        let mut probes = 0;
        let mut enables = 0;
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                if enabled {
                    enables += 1;
                    if enables == 2 {
                        stop.store(true, Ordering::Release);
                    }
                }
                Ok(())
            },
            |_| {
                probes += 1;
                if probes == 2 {
                    return Ok(None);
                }
                if probes == 3 {
                    shared.engine.lock().unwrap().enabled = true;
                }
                Ok(Some(false))
            },
            Duration::ZERO,
        );
        assert_eq!(writes, [true, true, false]);
        assert!(!shared.engine.lock().unwrap().paused);
    }

    #[test]
    fn receiver_reauthenticates_when_acknowledged_mode_is_lost() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        let mut enables = 0;
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                if enabled {
                    enables += 1;
                    if enables == 2 {
                        stop.store(true, Ordering::Release);
                    }
                }
                Ok(())
            },
            |_| Ok(Some(false)),
            Duration::ZERO,
        );
        assert_eq!(writes, [true, true, false]);
    }

    #[test]
    fn receiver_retains_cleanup_failure_during_shutdown() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        run_worker(
            &shared,
            &stop,
            |_, _| {
                stop.store(true, Ordering::Release);
                Err(String::from("reset failed during cancellation").into())
            },
            |_| Ok(Some(true)),
            Duration::from_secs(2),
        );
        assert!(shared
            .engine
            .lock()
            .unwrap()
            .error
            .as_deref()
            .unwrap()
            .contains("reset failed"));
    }

    #[test]
    fn receiver_releases_engine_during_io_and_restores_after_pause() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        run_worker(
            &shared,
            &stop,
            |enabled, _| {
                writes.push(enabled);
                let mut engine = shared.engine.try_lock().expect("HID held engine lock");
                if enabled {
                    engine.paused = true;
                } else {
                    stop.store(true, Ordering::Release);
                }
                Ok(())
            },
            |_| Ok(Some(true)),
            Duration::from_secs(2),
        );
        assert_eq!(writes, [true, false]);
    }

    #[test]
    fn receiver_shutdown_restores_with_cancellation_disabled() {
        let shared = Shared::default();
        let stop = AtomicBool::new(false);
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let mut writes = Vec::new();
        run_worker(
            &shared,
            &stop,
            |enabled, cancel| {
                assert!(!cancel.load(Ordering::Acquire));
                writes.push(enabled);
                if enabled {
                    stop.store(true, Ordering::Release);
                }
                Ok(())
            },
            |_| Ok(Some(true)),
            Duration::from_secs(2),
        );
        assert_eq!(writes, [true, false]);
    }

    #[test]
    fn failed_receiver_setup_pauses_without_repeating_device_writes() {
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut engine = shared.engine.lock().unwrap();
            engine.enabled = true;
            engine.mappings.thumb = Action::VolumeUp;
        }
        let worker_shared = shared.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let mut attempts = 0;
            run_worker(
                &worker_shared,
                &worker_stop,
                |enabled, _| {
                    assert!(enabled);
                    attempts += 1;
                    Err(String::from("fake acknowledgement timeout").into())
                },
                |_| Ok(Some(true)),
                Duration::from_secs(2),
            );
            attempts
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !shared.engine.lock().unwrap().paused && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        let paused = shared.engine.lock().unwrap().paused;
        stop.store(true, Ordering::Release);
        worker.thread().unpark();
        assert_eq!(worker.join().unwrap(), 1);
        assert!(paused);
        let engine = shared.engine.lock().unwrap();
        assert!(!engine.enabled);
        assert!(engine
            .error
            .as_deref()
            .unwrap()
            .contains("fake acknowledgement timeout"));
    }
}
