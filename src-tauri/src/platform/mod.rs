#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
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
const RECEIVER_VENDOR: u16 = 0x276d;
const RECEIVER_PRODUCT: u16 = 0x1902;
