//! OpenBikeControl BLE peripheral, with a per-OS backend.

#[cfg(target_os = "linux")] mod linux;
#[cfg(not(target_os = "linux"))] mod stub;

#[cfg(target_os = "linux")] pub use linux::{ObcHandle, ObcPeripheral};
#[cfg(not(target_os = "linux"))] pub use stub::{ObcHandle, ObcPeripheral};
