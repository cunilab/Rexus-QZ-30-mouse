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
pub(crate) const RECEIVER_VENDOR: u16 = 0x276d;
pub(crate) const RECEIVER_PRODUCT: u16 = 0x1902;
// Bluetooth LE identity observed on a QZ30 ("Rexus QZ30-1" on macOS); the same on every OS.
pub(crate) const BLUETOOTH_VENDOR: u16 = 0x32c2;
pub(crate) const BLUETOOTH_PRODUCT: u16 = 0x0012;

// Receiver-only I/O (button 4 handshake, side-wheel frames) must never target a Bluetooth device.
pub(crate) fn is_receiver(device: &crate::Device) -> bool {
    (device.vendor_id, device.product_id) == (RECEIVER_VENDOR, RECEIVER_PRODUCT)
}

#[cfg(test)]
mod tests {
    use crate::Device;

    fn device(vendor_id: u16, product_id: u16) -> Device {
        Device {
            vendor_id,
            product_id,
            name: String::new(),
            transport: String::new(),
        }
    }

    #[test]
    fn only_the_usb_receiver_counts_as_receiver() {
        assert!(super::is_receiver(&device(0x276d, 0x1902)));
        assert!(!super::is_receiver(&device(
            super::BLUETOOTH_VENDOR,
            super::BLUETOOTH_PRODUCT
        )));
    }
}
