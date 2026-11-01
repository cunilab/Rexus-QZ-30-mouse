#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod thumb;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!("QZ Flow supports Windows, macOS and Linux only.");

// USB IDs of the receiver all platform inventories look for.
pub(crate) const RECEIVER_VENDOR: u16 = 0x276d;
pub(crate) const RECEIVER_PRODUCT: u16 = 0x1902;
// Bluetooth LE identity observed on a QZ30 ("Rexus QZ30-1" on macOS); the same on every OS.
pub(crate) const BLUETOOTH_VENDOR: u16 = 0x32c2;
pub(crate) const BLUETOOTH_PRODUCT: u16 = 0x0012;
// Both transports of the same mouse; button 4's vendor collection exists on each.
pub(crate) const COMPATIBLE_IDS: [(u16, u16); 2] = [
    (RECEIVER_VENDOR, RECEIVER_PRODUCT),
    (BLUETOOTH_VENDOR, BLUETOOTH_PRODUCT),
];
