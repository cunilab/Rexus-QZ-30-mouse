#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("QZ Flow currently supports Windows and macOS only.");

// USB IDs of the receiver all platform inventories look for.
const RECEIVER_VENDOR: u16 = 0x276d;
const RECEIVER_PRODUCT: u16 = 0x1902;
