use super::thumb::{self, FeatureIo};
use crate::{
    engine::{Action, Input},
    Device, Shared,
};
use evdev::{
    uinput::VirtualDevice, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    RelativeAxisCode,
};
use std::{
    fs,
    os::{fd::AsRawFd, unix::fs::PermissionsExt, unix::process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static KEYBOARD: Mutex<Option<VirtualDevice>> = Mutex::new(None);
const SYSFS_INPUT: &str = "/sys/class/input";
const UINPUT: &str = "/dev/uinput";
const SYSFS_HIDRAW: &str = "/sys/class/hidraw";
// _IOC(_IOC_READ | _IOC_WRITE, 'H', nr, len) as on x86 and Arm; other architectures encode it differently.
const fn hid_ioc(nr: u32, len: u32) -> libc::Ioctl {
    ((3 << 30) | (len << 16) | ((b'H' as u32) << 8) | nr) as libc::Ioctl
}
const HIDIOCSFEATURE_8: libc::Ioctl = hid_ioc(0x06, 8);
const HIDIOCGFEATURE_8: libc::Ioctl = hid_ioc(0x07, 8);
// Discovery skips devices with this prefix, so our own virtual devices are never re-processed.
const VIRTUAL_PREFIX: &str = "QZ Flow virtual";
const MOUSE_NAME: &str = "QZ Flow virtual mouse";
const KEYBOARD_NAME: &str = "QZ Flow virtual keyboard";
// Variables an AppImage injects for itself; launched programs must not inherit them.
const APPIMAGE_ENV: [&str; 12] = [
    "APPIMAGE",
    "APPDIR",
    "ARGV0",
    "OWD",
    "LD_LIBRARY_PATH",
    "GTK_PATH",
    "GIO_MODULE_DIR",
    "GDK_PIXBUF_MODULE_FILE",
    "GSETTINGS_SCHEMA_DIR",
    "PYTHONHOME",
    "PYTHONPATH",
    "QT_PLUGIN_PATH",
];
const SETUP_HINT: &str = "Install packaging/linux/70-qz-flow.rules as described in the README, replug the receiver or reconnect Bluetooth, then sign out and in.";

struct Node {
    path: PathBuf,
    name: String,
    pointer: bool,
    bluetooth: bool,
}

// sysfs files hold hexadecimal words, most significant first; the last word has the low bits.
fn parse_hex(text: &str) -> Option<u64> {
    u64::from_str_radix(text.split_whitespace().last()?, 16).ok()
}

fn read_hex(path: &Path) -> Option<u64> {
    parse_hex(&fs::read_to_string(path).ok()?)
}

fn has_relative_axes(event_types: u64) -> bool {
    event_types & (1 << EventType::RELATIVE.0) != 0
}

fn is_bluetooth(vendor: Option<u64>, product: Option<u64>) -> bool {
    (vendor, product)
        == (
            Some(super::BLUETOOTH_VENDOR.into()),
            Some(super::BLUETOOTH_PRODUCT.into()),
        )
}

// BlueZ exposes the BLE mouse through uhid with bustype 0x05 and the same vendor/product IDs.
fn is_compatible(bus: Option<u64>, vendor: Option<u64>, product: Option<u64>, name: &str) -> bool {
    (is_bluetooth(vendor, product)
        || (vendor, product)
            == (
                Some(super::RECEIVER_VENDOR.into()),
                Some(super::RECEIVER_PRODUCT.into()),
            ))
        && bus != Some(BusType::BUS_VIRTUAL.0.into())
        && !name.starts_with(VIRTUAL_PREFIX)
}

// sysfs is world-readable, so the mouse is found even before /dev/input access is granted.
fn compatible_nodes() -> Result<Vec<Node>, String> {
    let entries = fs::read_dir(SYSFS_INPUT)
        .map_err(|error| format!("Linux could not list input devices: {error}"))?;
    let mut nodes = Vec::new();
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str().filter(|name| name.starts_with("event")) else {
            continue;
        };
        let device = entry.path().join("device");
        let id = |name: &str| read_hex(&device.join("id").join(name));
        let name = fs::read_to_string(device.join("name"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        if is_compatible(id("bustype"), id("vendor"), id("product"), &name) {
            nodes.push(Node {
                path: Path::new("/dev/input").join(file),
                name,
                pointer: read_hex(&device.join("capabilities/ev")).is_some_and(has_relative_axes),
                bluetooth: is_bluetooth(id("vendor"), id("product")),
            });
        }
    }
    nodes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(nodes)
}

// One entry per link (receiver, Bluetooth), each from its pointer node when it has one.
pub fn devices() -> Result<Vec<Device>, String> {
    let nodes = compatible_nodes()?;
    Ok([false, true]
        .into_iter()
        .filter_map(|bluetooth| {
            let kind: Vec<_> = nodes.iter().filter(|n| n.bluetooth == bluetooth).collect();
            let node = kind.iter().find(|n| n.pointer).or(kind.first())?;
            Some(Device {
                vendor_id: if bluetooth {
                    super::BLUETOOTH_VENDOR
                } else {
                    super::RECEIVER_VENDOR
                },
                product_id: if bluetooth {
                    super::BLUETOOTH_PRODUCT
                } else {
                    super::RECEIVER_PRODUCT
                },
                name: if !node.name.is_empty() {
                    node.name.clone()
                } else if bluetooth {
                    "Rexus QZ30".into()
                } else {
                    "QZ30-compatible USB receiver".into()
                },
                transport: if bluetooth {
                    "Bluetooth LE".into()
                } else {
                    "USB / 2.4 GHz receiver".into()
                },
            })
        })
        .collect())
}

fn access_granted(nodes: &[Node], readable: impl Fn(&Path) -> bool, uinput_writable: bool) -> bool {
    uinput_writable
        && nodes
            .iter()
            .filter(|node| node.pointer)
            .all(|node| readable(&node.path))
}

pub fn permission() -> &'static str {
    let uinput = fs::OpenOptions::new().write(true).open(UINPUT).is_ok();
    let nodes = compatible_nodes().unwrap_or_default();
    if access_granted(&nodes, |path| fs::File::open(path).is_ok(), uinput) {
        "granted"
    } else {
        "required"
    }
}

pub fn ensure_no_vendor_utility() -> Result<(), String> {
    Ok(()) // The original utility is Windows/macOS only.
}

pub fn open_input_settings() -> Result<(), String> {
    Err(format!(
        "Linux has no input permission dialog. {SETUP_HINT}"
    ))
}

// uevent line `HID_ID=<bus>:<vendor>:<product>` in hexadecimal; USB is bus 0003, Bluetooth (uhid) bus 0005.
fn parse_hid_id(uevent: &str) -> Option<(u32, u16, u16)> {
    let mut parts = uevent
        .lines()
        .find_map(|line| line.strip_prefix("HID_ID="))?
        .split(':')
        .map(|part| u32::from_str_radix(part, 16).ok());
    let (bus, vendor, product) = (parts.next()??, parts.next()??, parts.next()??);
    Some((bus, vendor.try_into().ok()?, product.try_into().ok()?))
}

// The hidraw node of the vendor collection ffbb:1; a receiver has several, a Bluetooth mouse one.
fn is_vendor_node(uevent: &str, descriptor: &[u8]) -> bool {
    let id = parse_hid_id(uevent).unwrap_or_default();
    // Buses in COMPATIBLE_IDS order: the receiver is USB, the mouse's Bluetooth LE input comes from uhid.
    super::COMPATIBLE_IDS
        .into_iter()
        .zip([0x0003, 0x0005])
        .any(|((vendor, product), bus)| (bus, vendor, product) == id)
        && thumb::has_vendor_collection(descriptor)
}

// sysfs is world-readable, so the node is found before /dev/hidraw access is granted.
fn vendor_nodes() -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(SYSFS_HIDRAW)
        .map_err(|error| format!("Linux could not list HID devices: {error}"))?;
    let mut nodes: Vec<_> = entries
        .flatten()
        .filter(|entry| {
            let device = entry.path().join("device");
            is_vendor_node(
                &fs::read_to_string(device.join("uevent")).unwrap_or_default(),
                &fs::read(device.join("report_descriptor")).unwrap_or_default(),
            )
        })
        .map(|entry| Path::new("/dev").join(entry.file_name()))
        .collect();
    nodes.sort();
    Ok(nodes)
}

fn open_vendor_node(write: bool) -> Result<fs::File, String> {
    let nodes = vendor_nodes()?;
    let [path] = &nodes[..] else {
        return Err(thumb::AMBIGUOUS.into());
    };
    fs::OpenOptions::new()
        .read(true)
        .write(write)
        .open(path)
        .map_err(|error| format!("Could not open {} ({error}). {SETUP_HINT}", path.display()))
}

// Feature reports 0xBB through hidraw; the buffer carries the report ID first.
struct Hidraw(fs::File);

impl Hidraw {
    fn open() -> Result<Self, String> {
        open_vendor_node(true).map(Self)
    }

    fn feature(&self, request: libc::Ioctl, report: &mut [u8; 8]) -> Result<(), String> {
        let code = unsafe { libc::ioctl(self.0.as_raw_fd(), request, report.as_mut_ptr()) };
        if code < 0 {
            Err(format!(
                "QZ30 feature report failed: {}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(())
        }
    }
}

impl FeatureIo for Hidraw {
    fn write(&mut self, mut report: [u8; 8]) -> Result<(), String> {
        self.feature(HIDIOCSFEATURE_8, &mut report)
    }

    fn read(&mut self) -> Result<[u8; 8], String> {
        let mut report = [0xbb, 0, 0, 0, 0, 0, 0, 0];
        self.feature(HIDIOCGFEATURE_8, &mut report)?;
        Ok(report)
    }
}

// Button 4 frames arrive as input reports on the vendor node, read only while the mode is active.
fn read_thumb(shared: &Shared, stop: &AtomicBool) {
    let (mut file, mut held, mut frame) = (None, false, [0u8; 64]);
    while !stop.load(Ordering::Acquire) {
        if !shared.thumb_ready.load(Ordering::Acquire) {
            file = None;
        } else if file.is_none() {
            file = open_vendor_node(false).ok();
            held = false;
        }
        let Some(node) = &file else {
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        let mut poll = libc::pollfd {
            fd: node.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut poll, 1, 100) } <= 0 {
            continue;
        }
        let length = unsafe { libc::read(poll.fd, frame.as_mut_ptr().cast(), frame.len()) };
        if length <= 0 || poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            file = None; // The node vanished; reopen once the mode is ready again.
            continue;
        }
        if let Some((input, down)) = thumb::thumb_edge(&frame[..length as usize], &mut held) {
            shared.process(input, down, perform);
        }
    }
}

pub struct Hook {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    // Button 4 worker and frame reader.
    thumb: Vec<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for worker in &self.thumb {
            worker.thread().unpark();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        for join in self.thumb.drain(..) {
            let _ = join.join();
        }
        *KEYBOARD.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

struct Pair {
    device: evdev::Device,
    mouse: VirtualDevice,
    wheel: Wheel,
}

// Horizontal wheel state: devices with the legacy axis trigger on it; hi-res-only devices
// accumulate toward one 120-unit notch, since a multiplier can split a notch across reports.
struct Wheel {
    legacy: bool,
    hi_res: i32,
}

// Cleanup that must also run if the reader thread panics, or status would stay "active".
struct Reader<'a> {
    shared: &'a Shared,
    pairs: Vec<Pair>,
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        for pair in &mut self.pairs {
            let _ = pair.device.ungrab();
        }
        self.shared.hook_running.store(false, Ordering::Release);
        self.shared
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .enabled = false;
    }
}

fn uinput_error(error: std::io::Error) -> String {
    format!("Could not create a virtual input device ({error}). {SETUP_HINT}")
}

// The virtual mouse mirrors the physical one so every unmapped event can be forwarded unchanged.
fn open_pair(path: &Path) -> Result<Pair, String> {
    let device = evdev::Device::open(path)
        .map_err(|error| format!("Could not open {} ({error}). {SETUP_HINT}", path.display()))?;
    let id = device.input_id();
    let mut builder = VirtualDevice::builder()
        .map_err(uinput_error)?
        .name(MOUSE_NAME)
        .input_id(InputId::new(
            BusType::BUS_VIRTUAL,
            id.vendor(),
            id.product(),
            id.version(),
        ))
        .with_properties(device.properties())
        .map_err(uinput_error)?;
    if let Some(keys) = device.supported_keys() {
        builder = builder.with_keys(keys).map_err(uinput_error)?;
    }
    if let Some(axes) = device.supported_relative_axes() {
        builder = builder.with_relative_axes(axes).map_err(uinput_error)?;
    }
    if let Some(misc) = device.misc_properties() {
        builder = builder.with_msc(misc).map_err(uinput_error)?;
    }
    let mouse = builder.build().map_err(uinput_error)?;
    let legacy = device
        .supported_relative_axes()
        .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_HWHEEL));
    Ok(Pair {
        device,
        mouse,
        wheel: Wheel { legacy, hi_res: 0 },
    })
}

fn virtual_keyboard() -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..=248 {
        keys.insert(KeyCode::new(code));
    }
    VirtualDevice::builder()
        .map_err(uinput_error)?
        .name(KEYBOARD_NAME)
        .input_id(InputId::new(BusType::BUS_VIRTUAL, 0, 0, 1))
        .with_keys(&keys)
        .map_err(uinput_error)?
        .build()
        .map_err(uinput_error)
}

pub fn start(shared: Arc<Shared>) -> Result<Hook, String> {
    if permission() != "granted" {
        return Err(format!(
            "QZ Flow cannot open the mouse or /dev/uinput. {SETUP_HINT}"
        ));
    }
    let _ = SHARED.set(shared.clone());
    let nodes: Vec<_> = compatible_nodes()?
        .into_iter()
        .filter(|node| node.pointer)
        .collect();
    if nodes.is_empty() {
        return Err(
            "The mouse input was not found. Replug the receiver or reconnect Bluetooth.".into(),
        );
    }
    let mut pairs = nodes
        .iter()
        .map(|node| open_pair(&node.path))
        .collect::<Result<Vec<_>, _>>()?;
    let keyboard = virtual_keyboard()?;
    // Grab last: closing the descriptors on an error releases any grab already taken.
    for pair in &mut pairs {
        pair.device
            .set_nonblocking(true)
            .map_err(|e| e.to_string())?;
        pair.device
            .grab()
            .map_err(|error| format!("Could not take exclusive control of the mouse ({error}). Another program may be using it."))?;
    }
    *KEYBOARD.lock().unwrap_or_else(|e| e.into_inner()) = Some(keyboard);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let thread_shared = shared.clone();
    let (go, wait) = mpsc::channel::<()>();
    let join = thread::Builder::new()
        .name("qz-evdev".into())
        .spawn(move || {
            let mut reader = Reader {
                shared: &thread_shared,
                pairs,
            };
            // Started only once hook_running is set, so this thread's exit cannot be overwritten.
            if wait.recv().is_ok() {
                run(reader.shared, &thread_stop, &mut reader.pairs);
            }
        })
        .map_err(|error| {
            *KEYBOARD.lock().unwrap_or_else(|e| e.into_inner()) = None;
            format!("Could not start the input listener: {error}")
        })?;
    shared.hook_running.store(true, Ordering::Release);
    let _ = go.send(());
    // Dropping the hook on a failed spawn stops the input thread again.
    let mut hook = Hook {
        stop: stop.clone(),
        join: Some(join),
        thumb: Vec::new(),
    };
    hook.thumb.push(thumb::spawn_worker(
        shared.clone(),
        stop.clone(),
        Hidraw::open,
    )?);
    let reader_shared = shared.clone();
    hook.thumb.push(
        thread::Builder::new()
            .name("qz-hidraw".into())
            .spawn(move || read_thumb(&reader_shared, &stop))
            .map_err(|error| format!("Could not start the button 4 reader: {error}"))?,
    );
    Ok(hook)
}

// Returns when asked to stop or when a device disappears; polling keeps shutdown prompt.
fn run(shared: &Shared, stop: &AtomicBool, pairs: &mut [Pair]) {
    let mut fds: Vec<_> = pairs
        .iter()
        .map(|pair| libc::pollfd {
            fd: pair.device.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let mut batch = Vec::new();
    while !stop.load(Ordering::Acquire) {
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 100) };
        if ready < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        for (pair, fd) in pairs.iter_mut().zip(&fds) {
            if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                return;
            }
            if fd.revents & libc::POLLIN == 0 {
                continue;
            }
            let Pair {
                device,
                mouse,
                wheel,
            } = pair;
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(_) => return,
            };
            for event in events {
                if event.event_type() != EventType::SYNCHRONIZATION {
                    batch.push(event);
                    continue;
                }
                let output = filter_batch(&batch, wheel, |input, down| {
                    shared.process(input, down, perform)
                });
                batch.clear();
                if !output.is_empty() && mouse.emit(&output).is_err() {
                    return;
                }
            }
        }
    }
}

fn button_input(event: &InputEvent) -> Option<(Input, bool)> {
    if event.event_type() != EventType::KEY || !(0..=1).contains(&event.value()) {
        return None;
    }
    let input = match KeyCode::new(event.code()) {
        KeyCode::BTN_SIDE => Input::Back,
        KeyCode::BTN_EXTRA => Input::Forward,
        KeyCode::BTN_MIDDLE => Input::Middle,
        _ => return None,
    };
    Some((input, event.value() == 1))
}

fn is_horizontal_wheel(event: &InputEvent) -> bool {
    event.event_type() == EventType::RELATIVE
        && [
            RelativeAxisCode::REL_HWHEEL.0,
            RelativeAxisCode::REL_HWHEEL_HI_RES.0,
        ]
        .contains(&event.code())
}

fn wheel_input(batch: &[InputEvent], wheel: &mut Wheel) -> Option<Input> {
    let sum = |code: u16| {
        batch
            .iter()
            .filter(|event| event.event_type() == EventType::RELATIVE && event.code() == code)
            .map(InputEvent::value)
            .sum::<i32>()
    };
    let delta = if wheel.legacy {
        sum(RelativeAxisCode::REL_HWHEEL.0)
    } else {
        let hi_res = sum(RelativeAxisCode::REL_HWHEEL_HI_RES.0);
        if hi_res.signum() == -wheel.hi_res.signum() {
            wheel.hi_res = 0; // A reversal starts a new notch.
        }
        wheel.hi_res += hi_res;
        if wheel.hi_res.abs() < 120 {
            return None;
        }
        std::mem::take(&mut wheel.hi_res)
    };
    match delta.signum() {
        1 => Some(Input::WheelRight),
        -1 => Some(Input::WheelLeft),
        _ => None,
    }
}

// Everything is forwarded unchanged except events consumed by a mapped input.
fn filter_batch(
    batch: &[InputEvent],
    wheel: &mut Wheel,
    mut process: impl FnMut(Input, bool) -> bool,
) -> Vec<InputEvent> {
    let triggered = wheel_input(batch, wheel);
    let drop_wheel = triggered.is_some_and(|input| process(input, true));
    batch
        .iter()
        .filter(|event| {
            // Hi-res reports that did not complete a notch are forwarded unchanged.
            if is_horizontal_wheel(event) {
                return !drop_wheel;
            }
            button_input(event).is_none_or(|(input, down)| !process(input, down))
        })
        .copied()
        .collect()
}

fn shortcut(action: &Action) -> Result<Vec<u16>, String> {
    use KeyCode as K;
    let keys = match action {
        Action::Default | Action::Disable => vec![],
        Action::Copy => vec![K::KEY_LEFTCTRL, K::KEY_C],
        Action::Cut => vec![K::KEY_LEFTCTRL, K::KEY_X],
        Action::Paste => vec![K::KEY_LEFTCTRL, K::KEY_V],
        Action::Undo => vec![K::KEY_LEFTCTRL, K::KEY_Z],
        Action::Redo => vec![K::KEY_LEFTCTRL, K::KEY_LEFTSHIFT, K::KEY_Z],
        Action::SelectAll => vec![K::KEY_LEFTCTRL, K::KEY_A],
        Action::BrowserBack => vec![K::KEY_LEFTALT, K::KEY_LEFT],
        Action::BrowserForward => vec![K::KEY_LEFTALT, K::KEY_RIGHT],
        Action::TaskView => vec![K::KEY_LEFTMETA], // GNOME Activities overview.
        Action::PreviousDesktop => vec![K::KEY_LEFTCTRL, K::KEY_LEFTALT, K::KEY_LEFT],
        Action::NextDesktop => vec![K::KEY_LEFTCTRL, K::KEY_LEFTALT, K::KEY_RIGHT],
        Action::SwitchApps => vec![K::KEY_LEFTALT, K::KEY_TAB],
        Action::PreviousTab => vec![K::KEY_LEFTCTRL, K::KEY_LEFTSHIFT, K::KEY_TAB],
        Action::NextTab => vec![K::KEY_LEFTCTRL, K::KEY_TAB],
        Action::NewTab => vec![K::KEY_LEFTCTRL, K::KEY_T],
        Action::CloseTab => vec![K::KEY_LEFTCTRL, K::KEY_W],
        Action::ReopenTab => vec![K::KEY_LEFTCTRL, K::KEY_LEFTSHIFT, K::KEY_T],
        Action::Refresh => vec![K::KEY_LEFTCTRL, K::KEY_R],
        Action::ZoomIn => vec![K::KEY_LEFTCTRL, K::KEY_EQUAL],
        Action::ZoomOut => vec![K::KEY_LEFTCTRL, K::KEY_MINUS],
        Action::ZoomReset => vec![K::KEY_LEFTCTRL, K::KEY_0],
        Action::Screenshot => vec![K::KEY_SYSRQ], // The Print Screen key.
        Action::VolumeUp => vec![K::KEY_VOLUMEUP],
        Action::VolumeDown => vec![K::KEY_VOLUMEDOWN],
        Action::Mute => vec![K::KEY_MUTE],
        Action::PlayPause => vec![K::KEY_PLAYPAUSE],
        Action::PreviousTrack => vec![K::KEY_PREVIOUSSONG],
        Action::NextTrack => vec![K::KEY_NEXTSONG],
        Action::CustomShortcut { key, modifiers } => {
            let mut keys = Vec::new();
            for modifier in modifiers {
                let code = match modifier.as_str() {
                    "control" => K::KEY_LEFTCTRL,
                    "alt" => K::KEY_LEFTALT,
                    "shift" => K::KEY_LEFTSHIFT,
                    "meta" => K::KEY_LEFTMETA,
                    _ => return Err(format!("Unknown shortcut modifier: {modifier}")),
                };
                if keys.contains(&code) {
                    return Err("Shortcut modifiers must be unique.".into());
                }
                keys.push(code);
            }
            keys.push(
                custom_key(key)
                    .ok_or_else(|| format!("Unsupported shortcut key on Linux: {key}"))?,
            );
            keys
        }
        Action::Desktop | Action::Minimize | Action::Maximize => {
            return Err("This window action has no shortcut shared by GNOME and KDE, so it is not supported on Linux. Choose another action or a custom shortcut.".into());
        }
        Action::LaunchApplication { .. } => {
            return Err("This action does not use a keyboard shortcut.".into());
        }
    };
    Ok(keys.into_iter().map(|key| key.0).collect())
}

// Key names are US physical positions, as in the other platforms.
fn custom_key(key: &str) -> Option<KeyCode> {
    use KeyCode as K;
    const LETTERS: [KeyCode; 26] = [
        K::KEY_A,
        K::KEY_B,
        K::KEY_C,
        K::KEY_D,
        K::KEY_E,
        K::KEY_F,
        K::KEY_G,
        K::KEY_H,
        K::KEY_I,
        K::KEY_J,
        K::KEY_K,
        K::KEY_L,
        K::KEY_M,
        K::KEY_N,
        K::KEY_O,
        K::KEY_P,
        K::KEY_Q,
        K::KEY_R,
        K::KEY_S,
        K::KEY_T,
        K::KEY_U,
        K::KEY_V,
        K::KEY_W,
        K::KEY_X,
        K::KEY_Y,
        K::KEY_Z,
    ];
    const DIGITS: [KeyCode; 10] = [
        K::KEY_0,
        K::KEY_1,
        K::KEY_2,
        K::KEY_3,
        K::KEY_4,
        K::KEY_5,
        K::KEY_6,
        K::KEY_7,
        K::KEY_8,
        K::KEY_9,
    ];
    const FUNCTION: [KeyCode; 12] = [
        K::KEY_F1,
        K::KEY_F2,
        K::KEY_F3,
        K::KEY_F4,
        K::KEY_F5,
        K::KEY_F6,
        K::KEY_F7,
        K::KEY_F8,
        K::KEY_F9,
        K::KEY_F10,
        K::KEY_F11,
        K::KEY_F12,
    ];
    if let Some(letter) = key.strip_prefix("Key").filter(|s| s.len() == 1) {
        let letter = letter.as_bytes()[0];
        return letter
            .is_ascii_uppercase()
            .then(|| LETTERS[(letter - b'A') as usize]);
    }
    if let Some(digit) = key.strip_prefix("Digit").filter(|s| s.len() == 1) {
        let digit = digit.as_bytes()[0];
        return digit
            .is_ascii_digit()
            .then(|| DIGITS[(digit - b'0') as usize]);
    }
    if let Some(number) = key.strip_prefix('F').and_then(|s| s.parse::<usize>().ok()) {
        if (1..=12).contains(&number) && key == format!("F{number}") {
            return Some(FUNCTION[number - 1]);
        }
    }
    Some(match key {
        "ArrowLeft" => K::KEY_LEFT,
        "ArrowRight" => K::KEY_RIGHT,
        "ArrowUp" => K::KEY_UP,
        "ArrowDown" => K::KEY_DOWN,
        "Space" => K::KEY_SPACE,
        "Enter" => K::KEY_ENTER,
        "Escape" => K::KEY_ESC,
        "Tab" => K::KEY_TAB,
        "Backspace" => K::KEY_BACKSPACE,
        "Delete" => K::KEY_DELETE,
        "Home" => K::KEY_HOME,
        "End" => K::KEY_END,
        "PageUp" => K::KEY_PAGEUP,
        "PageDown" => K::KEY_PAGEDOWN,
        "Insert" => K::KEY_INSERT,
        "Minus" => K::KEY_MINUS,
        "Equal" => K::KEY_EQUAL,
        "BracketLeft" => K::KEY_LEFTBRACE,
        "BracketRight" => K::KEY_RIGHTBRACE,
        "Backslash" => K::KEY_BACKSLASH,
        "Semicolon" => K::KEY_SEMICOLON,
        "Quote" => K::KEY_APOSTROPHE,
        "Comma" => K::KEY_COMMA,
        "Period" => K::KEY_DOT,
        "Slash" => K::KEY_SLASH,
        "Backquote" => K::KEY_GRAVE,
        _ => return None,
    })
}

fn validate_application_shape(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("Choose an application with an absolute path to an executable file.".into());
    }
    Ok(())
}

fn validate_application(path: &str) -> Result<(), String> {
    validate_application_shape(path)?;
    let executable = fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
    if !executable {
        return Err(
            "The selected application no longer exists or is not executable. Choose it again."
                .into(),
        );
    }
    Ok(())
}

pub fn validate_action(action: &Action) -> Result<(), String> {
    match action {
        // Existence is checked when picking and pressing, so a missing app never disables other mappings.
        Action::LaunchApplication { path } => validate_application_shape(path),
        _ => shortcut(action).map(|_| ()),
    }
}

fn picker_result(code: Option<i32>, stdout: &[u8]) -> Result<Option<String>, String> {
    match code {
        Some(0) => {
            let text = std::str::from_utf8(stdout)
                .map_err(|_| "Application picker returned invalid text.")?;
            let path = text.strip_suffix('\n').unwrap_or(text);
            if path.is_empty() {
                return Ok(None);
            }
            validate_application(path)?;
            Ok(Some(path.into()))
        }
        Some(1) => Ok(None), // Both pickers exit with 1 when cancelled.
        _ => Err("Application picker failed.".into()),
    }
}

pub fn pick_application() -> Result<Option<String>, String> {
    // Fixed arguments; the selected path comes back on stdout as data and is never run through a shell.
    let pickers: [(&str, &[&str]); 2] = [
        (
            "zenity",
            &["--file-selection", "--title=Choose an application"],
        ),
        (
            "kdialog",
            &[
                "--getopenfilename",
                "/usr/bin",
                "--title",
                "Choose an application",
            ],
        ),
    ];
    for (program, args) in pickers {
        match Command::new(program).args(args).output() {
            Ok(output) => return picker_result(output.status.code(), &output.stdout),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not open application picker: {error}")),
        }
    }
    Err("Install zenity or kdialog to choose an application, or run QZ Flow from a desktop session that provides one.".into())
}

fn launch(path: String) -> Result<(), String> {
    // Filesystem checks and process creation can block; keep them off the input thread.
    thread::Builder::new()
        .name("qz-launch".into())
        .spawn(move || {
            let result = validate_application(&path).and_then(|_| {
                let mut command = Command::new(&path);
                if std::env::var_os("APPIMAGE").is_some() {
                    for name in APPIMAGE_ENV {
                        command.env_remove(name);
                    }
                }
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .spawn()
                    .map_err(|e| format!("Could not launch application: {e}"))
            });
            match (result, SHARED.get()) {
                // Waiting reaps the child so it never lingers as a zombie.
                (Ok(mut child), _) => {
                    let _ = child.wait();
                }
                (Err(error), Some(shared)) => {
                    shared
                        .engine
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .error = Some(error);
                }
                _ => {}
            }
        })
        .map(|_| ())
        .map_err(|e| format!("Could not schedule application launch: {e}"))
}

fn press(keys: &[u16]) -> Result<(), String> {
    let mut guard = KEYBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let keyboard = guard
        .as_mut()
        .ok_or("The virtual keyboard is unavailable. QZ Flow will retry automatically.")?;
    let events = |keys: &mut dyn Iterator<Item = &u16>, value| {
        keys.map(|key| InputEvent::new(EventType::KEY.0, *key, value))
            .collect::<Vec<_>>()
    };
    let pressed = keyboard.emit(&events(&mut keys.iter(), 1));
    // Always attempt the releases so a failed press cannot leave a key stuck down.
    let released = keyboard.emit(&events(&mut keys.iter().rev(), 0));
    let result = pressed.and(released);
    if result.is_err() {
        // Destroying the uinput device makes the kernel release any key still held on it.
        *guard = None;
        *guard = virtual_keyboard().ok();
    }
    result.map_err(|error| format!("Linux could not inject the shortcut: {error}"))
}

fn perform(action: Action) -> Result<(), String> {
    if let Action::LaunchApplication { path } = action {
        return launch(path);
    }
    validate_action(&action)?;
    match action {
        Action::Disable => Ok(()),
        Action::Default => Err("Default actions must pass through unchanged.".into()),
        _ => press(&shortcut(&action)?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hid_feature_ioctls_match_the_kernel_values() {
        assert_eq!(HIDIOCSFEATURE_8 as u32, 0xc008_4806);
        assert_eq!(HIDIOCGFEATURE_8 as u32, 0xc008_4807);
    }

    #[test]
    fn hid_id_and_descriptor_select_the_vendor_node_on_both_transports() {
        let vendor = [0x06, 0xbb, 0xff, 0x09, 0x01, 0xa1, 0x01];
        let uevent = |id: &str| format!("DRIVER=hid-generic\nHID_ID={id}\nHID_NAME=QZ30\n");
        let usb = uevent("0003:0000276D:00001902");
        let ble = uevent("0005:000032C2:00000012");
        assert_eq!(parse_hid_id(&usb), Some((3, 0x276d, 0x1902)));
        assert!(is_vendor_node(&usb, &vendor));
        assert!(is_vendor_node(&ble, &vendor));
        // The mouse and keyboard interfaces of the same receiver have no vendor collection.
        assert!(!is_vendor_node(&usb, &[0x05, 0x01, 0x09, 0x02, 0xa1, 0x01]));
        // IDs must come with their own bus.
        assert!(!is_vendor_node(&uevent("0005:0000276D:00001902"), &vendor));
        assert!(!is_vendor_node(&uevent("0003:000032C2:00000012"), &vendor));
        assert!(!is_vendor_node(&uevent("0003:000046D:0000C52B"), &vendor));
        assert_eq!(parse_hid_id("HID_ID=0003:0001276D:00001902"), None);
        assert_eq!(parse_hid_id("HID_ID=zz:1:2"), None);
        assert_eq!(parse_hid_id("HID_NAME=QZ30"), None);
    }

    fn key(code: KeyCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::KEY.0, code.0, value)
    }
    fn rel(code: RelativeAxisCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::RELATIVE.0, code.0, value)
    }
    fn legacy() -> Wheel {
        Wheel {
            legacy: true,
            hi_res: 0,
        }
    }
    fn codes(events: &[InputEvent]) -> Vec<(u16, u16)> {
        events
            .iter()
            .map(|event| (event.event_type().0, event.code()))
            .collect()
    }

    #[test]
    fn sysfs_words_and_receiver_identity_are_parsed_strictly() {
        assert_eq!(parse_hex("276d\n"), Some(0x276d));
        assert_eq!(parse_hex("0 17\n"), Some(0x17));
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("zz"), None);
        assert!(has_relative_axes(0x17));
        assert!(!has_relative_axes(0x13));
        let (v, p) = (Some(0x276d), Some(0x1902));
        assert!(is_compatible(Some(3), v, p, "Rexus Mouse"));
        assert!(!is_compatible(Some(3), v, Some(0x1903), "Rexus Mouse"));
        assert!(!is_compatible(Some(3), Some(0x276e), p, "Rexus Mouse"));
        assert!(!is_compatible(Some(3), None, None, "Rexus Mouse"));
        // Our own virtual mouse mirrors the receiver's IDs but must never be rediscovered.
        assert!(!is_compatible(Some(6), v, p, "Rexus Mouse"));
        assert!(!is_compatible(Some(3), v, p, MOUSE_NAME));
    }

    #[test]
    fn bluetooth_identity_is_accepted_over_the_bluetooth_bus_only_as_exact_ids() {
        let (v, p) = (Some(0x32c2), Some(0x0012));
        assert!(is_compatible(Some(5), v, p, "Rexus QZ30"));
        assert!(is_bluetooth(v, p));
        assert!(!is_bluetooth(Some(0x276d), Some(0x1902)));
        assert!(!is_compatible(Some(5), v, Some(0x0013), "Rexus QZ30"));
        assert!(!is_compatible(Some(5), Some(0x32c3), p, "Rexus QZ30"));
        assert!(!is_compatible(Some(5), v, Some(0x1902), "Rexus QZ30"));
        // Mixed pairs of the two identities must not match.
        assert!(!is_compatible(Some(5), Some(0x276d), p, "Rexus QZ30"));
        assert!(!is_compatible(Some(6), v, p, "Rexus QZ30"));
        assert!(!is_compatible(Some(5), v, p, MOUSE_NAME));
    }

    #[test]
    fn permission_needs_readable_pointer_nodes_and_writable_uinput() {
        let node = |name: &str, pointer| Node {
            path: name.into(),
            name: String::new(),
            pointer,
            bluetooth: false,
        };
        let nodes = [
            node("/dev/input/event1", true),
            node("/dev/input/event2", false),
        ];
        let only_second_unreadable = |path: &Path| path != Path::new("/dev/input/event2");
        assert!(access_granted(&nodes, only_second_unreadable, true));
        assert!(!access_granted(&nodes, |_| false, true));
        assert!(!access_granted(&nodes, |_| true, false));
        assert!(access_granted(&[], |_| false, true));
    }

    #[test]
    fn side_buttons_and_wheel_map_to_inputs_and_other_events_pass_through() {
        assert_eq!(
            button_input(&key(KeyCode::BTN_SIDE, 1)),
            Some((Input::Back, true))
        );
        assert_eq!(
            button_input(&key(KeyCode::BTN_EXTRA, 0)),
            Some((Input::Forward, false))
        );
        assert_eq!(
            button_input(&key(KeyCode::BTN_MIDDLE, 1)),
            Some((Input::Middle, true))
        );
        assert_eq!(button_input(&key(KeyCode::BTN_LEFT, 1)), None);
        assert_eq!(button_input(&key(KeyCode::BTN_SIDE, 2)), None);
        let both = [
            rel(RelativeAxisCode::REL_HWHEEL_HI_RES, -120),
            rel(RelativeAxisCode::REL_HWHEEL, -1),
        ];
        assert_eq!(wheel_input(&both, &mut legacy()), Some(Input::WheelLeft));
        assert_eq!(
            wheel_input(&[rel(RelativeAxisCode::REL_HWHEEL, 1)], &mut legacy()),
            Some(Input::WheelRight)
        );
        assert_eq!(
            wheel_input(&[rel(RelativeAxisCode::REL_WHEEL, 1)], &mut legacy()),
            None
        );
    }

    #[test]
    fn consumed_inputs_are_dropped_and_everything_else_is_forwarded() {
        let batch = [
            rel(RelativeAxisCode::REL_X, 5),
            key(KeyCode::BTN_SIDE, 1),
            key(KeyCode::BTN_LEFT, 1),
        ];
        let mut seen = Vec::new();
        let output = filter_batch(&batch, &mut legacy(), |input, down| {
            seen.push((input, down));
            input == Input::Back
        });
        assert_eq!(seen, [(Input::Back, true)]);
        assert_eq!(
            codes(&output),
            [
                (EventType::RELATIVE.0, RelativeAxisCode::REL_X.0),
                (EventType::KEY.0, KeyCode::BTN_LEFT.0)
            ]
        );
        assert_eq!(filter_batch(&batch, &mut legacy(), |_, _| false).len(), 3);
    }

    #[test]
    fn consumed_wheel_drops_legacy_and_hi_res_events_but_keeps_vertical_scroll() {
        let batch = [
            rel(RelativeAxisCode::REL_HWHEEL_HI_RES, 120),
            rel(RelativeAxisCode::REL_HWHEEL, 1),
            rel(RelativeAxisCode::REL_WHEEL, 1),
        ];
        let mut calls = 0;
        let output = filter_batch(&batch, &mut legacy(), |input, _| {
            calls += 1;
            input == Input::WheelRight
        });
        assert_eq!(calls, 1);
        assert_eq!(
            codes(&output),
            [(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0)]
        );
        assert_eq!(filter_batch(&batch, &mut legacy(), |_, _| false).len(), 3);
    }

    #[test]
    fn hi_res_only_wheel_triggers_once_per_accumulated_notch() {
        let hi = |value| [rel(RelativeAxisCode::REL_HWHEEL_HI_RES, value)];
        let mut wheel = Wheel {
            legacy: false,
            hi_res: 0,
        };
        // One 120-unit notch split across reports fires once, on the report that completes it.
        let fired: Vec<_> = [30, 30, 30, 30, 30, 30, 30, 30]
            .into_iter()
            .map(|value| wheel_input(&hi(value), &mut wheel))
            .collect();
        assert_eq!(fired.iter().filter(|input| input.is_some()).count(), 2);
        assert_eq!(fired[3], Some(Input::WheelRight));
        assert_eq!(fired[7], Some(Input::WheelRight));
        // A reversal discards the partial notch instead of cancelling against it.
        assert_eq!(wheel_input(&hi(90), &mut wheel), None);
        assert_eq!(wheel_input(&hi(-120), &mut wheel), Some(Input::WheelLeft));
        // A report that does not complete a notch is forwarded, not consumed.
        let batch = hi(40);
        let output = filter_batch(&batch, &mut wheel, |_, _| true);
        assert_eq!(output.len(), 1);
        // The legacy axis, when present, fires once per legacy event however many hi-res events arrive.
        let mut with_legacy = legacy();
        let batch = [
            rel(RelativeAxisCode::REL_HWHEEL_HI_RES, 40),
            rel(RelativeAxisCode::REL_HWHEEL, 1),
        ];
        assert_eq!(
            wheel_input(&batch, &mut with_legacy),
            Some(Input::WheelRight)
        );
        assert_eq!(wheel_input(&batch[..1], &mut with_legacy), None);
    }

    #[test]
    fn appimage_environment_variables_are_stripped_for_launched_programs() {
        for name in [
            "LD_LIBRARY_PATH",
            "APPDIR",
            "APPIMAGE",
            "GTK_PATH",
            "PYTHONHOME",
            "QT_PLUGIN_PATH",
        ] {
            assert!(APPIMAGE_ENV.contains(&name), "{name}");
        }
    }

    #[test]
    fn actions_map_to_distinct_linux_shortcuts() {
        let codes = |action: &Action| shortcut(action).unwrap();
        assert_eq!(
            codes(&Action::Copy),
            [KeyCode::KEY_LEFTCTRL.0, KeyCode::KEY_C.0]
        );
        assert_eq!(
            codes(&Action::Redo),
            [
                KeyCode::KEY_LEFTCTRL.0,
                KeyCode::KEY_LEFTSHIFT.0,
                KeyCode::KEY_Z.0
            ]
        );
        assert_eq!(codes(&Action::TaskView), [KeyCode::KEY_LEFTMETA.0]);
        assert_eq!(codes(&Action::Mute), [KeyCode::KEY_MUTE.0]);
        assert_eq!(codes(&Action::PlayPause), [KeyCode::KEY_PLAYPAUSE.0]);
        assert_eq!(
            codes(&Action::NextDesktop),
            [
                KeyCode::KEY_LEFTCTRL.0,
                KeyCode::KEY_LEFTALT.0,
                KeyCode::KEY_RIGHT.0
            ]
        );
        assert!(codes(&Action::Default).is_empty());
        assert!(codes(&Action::Disable).is_empty());
        assert!(validate_action(&Action::Disable).is_ok());
        for action in [Action::Desktop, Action::Minimize, Action::Maximize] {
            assert!(validate_action(&action).is_err());
        }
    }

    #[test]
    fn custom_shortcuts_validate_keys_and_modifiers() {
        let custom = |key: &str, modifiers: &[&str]| Action::CustomShortcut {
            key: key.into(),
            modifiers: modifiers.iter().map(|m| m.to_string()).collect(),
        };
        assert_eq!(
            shortcut(&custom("KeyK", &["control", "shift"])).unwrap(),
            [
                KeyCode::KEY_LEFTCTRL.0,
                KeyCode::KEY_LEFTSHIFT.0,
                KeyCode::KEY_K.0
            ]
        );
        for key in ["Keya", "F13", "F01", "Shell", "NumpadEnter", ""] {
            assert!(custom_key(key).is_none());
        }
        for key in ["KeyZ", "Digit0", "F12", "Insert", "Slash", "ArrowLeft"] {
            assert!(custom_key(key).is_some());
        }
        for letter in b'A'..=b'Z' {
            assert!(custom_key(&format!("Key{}", letter as char)).is_some());
        }
        assert!(validate_action(&custom("KeyK", &["meta", "meta"])).is_err());
        assert!(validate_action(&custom("KeyK", &["shell"])).is_err());
    }

    #[test]
    fn applications_are_validated_by_shape_and_by_existence_separately() {
        assert!(validate_application_shape("firefox").is_err());
        assert!(validate_application_shape("/").is_err());
        assert!(validate_application_shape("/opt/missing/app").is_ok());
        assert!(validate_application("/opt/missing/app").is_err());
        assert!(validate_application("/etc/passwd").is_err());
        let current = std::env::current_exe().unwrap();
        assert!(validate_application(current.to_str().unwrap()).is_ok());
    }

    #[test]
    fn picker_output_is_data_and_cancellation_is_not_an_error() {
        let current = std::env::current_exe().unwrap();
        let chosen = format!("{}\n", current.display());
        assert_eq!(
            picker_result(Some(0), chosen.as_bytes()).unwrap(),
            Some(current.display().to_string())
        );
        assert_eq!(picker_result(Some(1), b"").unwrap(), None);
        assert_eq!(picker_result(Some(0), b"\n").unwrap(), None);
        assert!(picker_result(Some(0), b"relative/app\n").is_err());
        assert!(picker_result(Some(2), b"").is_err());
        assert!(picker_result(None, b"").is_err());
    }
}
