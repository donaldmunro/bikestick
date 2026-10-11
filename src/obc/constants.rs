//! OpenBikeControl BLE constants
use uuid::{Uuid, uuid};

pub const OBC_SERVICE: Uuid = uuid!("d273f680-d548-419d-b9d1-fa0472345229");
/// Notify: button state, us -> trainer app.
pub const BUTTON_STATE_CHARACTERISTIC: Uuid = uuid!("d273f681-d548-419d-b9d1-fa0472345229");
/// Write: app info, trainer app -> us.
pub const APP_INFO_CHARACTERISTIC: Uuid = uuid!("d273f683-d548-419d-b9d1-fa0472345229");

// Standard services.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const DEVICE_INFORMATION_SERVICE: Uuid = uuid!("0000180a-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CHAR_MANUFACTURER_NAME: Uuid = uuid!("00002a29-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CHAR_SERIAL_NUMBER: Uuid = uuid!("00002a25-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CHAR_HARDWARE_REVISION: Uuid = uuid!("00002a27-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CHAR_FIRMWARE_REVISION: Uuid = uuid!("00002a26-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const BATTERY_SERVICE: Uuid = uuid!("0000180f-0000-1000-8000-00805f9b34fb");
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const CHAR_BATTERY_LEVEL: Uuid = uuid!("00002a19-0000-1000-8000-00805f9b34fb");

/// Message type bytes (first byte of every OBC message). Device status and
/// haptic feedback are part of the protocol but unused by this daemon.
pub const MSG_BUTTON_STATE: u8 = 0x01;
#[allow(dead_code)]
pub const MSG_DEVICE_STATUS: u8 = 0x02;
#[allow(dead_code)]
pub const MSG_HAPTIC_FEEDBACK: u8 = 0x03;
pub const MSG_APP_INFO: u8 = 0x04;

pub const APP_INFO_VERSION: u8 = 0x01;

pub const STATE_RELEASED: u8 = 0x00;
pub const STATE_PRESSED: u8 = 0x01;

/// App-info fragments are buffered up to this size before giving up
/// (a valid app info is ~100 bytes; mirrors upstream `AppInfoReassembler`).
pub const APP_INFO_MAX_BUFFER: usize = 512;
