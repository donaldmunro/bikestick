use uuid::{Uuid, uuid};

/// Bluetooth SIG company identifier for Zwift, Inc.
pub const ZWIFT_MANUFACTURER_ID: u16 = 2378; // 0x094A

/// First byte of the Zwift manufacturer-data payload identifying the device.
pub const RC1_RIGHT_SIDE: u8 = 0x02;
pub const RC1_LEFT_SIDE: u8 = 0x03;

pub const ZWIFT_CUSTOM_SERVICE: Uuid = uuid!("00000001-19CA-4651-86E5-FA29DCDD09D1");
/// Notifications: controller/battery/idle messages (device -> host).
pub const ZWIFT_ASYNC_CHARACTERISTIC: Uuid = uuid!("00000002-19CA-4651-86E5-FA29DCDD09D1");
/// Write-without-response: handshake and commands (host -> device).
pub const ZWIFT_SYNC_RX_CHARACTERISTIC: Uuid = uuid!("00000003-19CA-4651-86E5-FA29DCDD09D1");
/// Indications: handshake response (device -> host).
pub const ZWIFT_SYNC_TX_CHARACTERISTIC: Uuid = uuid!("00000004-19CA-4651-86E5-FA29DCDD09D1");

/// ASCII "RideOn" - the entire plaintext handshake. In plaintext mode the
/// device echoes this back bare (the `01 04` "Play" tag and device public key
/// only appear in the encrypted-handshake flow).
pub const RIDE_ON: [u8; 6] = [0x52, 0x69, 0x64, 0x65, 0x4F, 0x6E];

/// Message type bytes (first byte of every notification).
pub const MSG_PLAY_KEYPAD: u8 = 0x07;
pub const MSG_IDLE: u8 = 0x15;
pub const MSG_BATTERY: u8 = 0x19;
pub const MSG_DISCONNECT: u8 = 0xFE;

/// Fixed vibration command (a pre-encoded PlayCommand protobuf), written to
/// SYNC_RX. Pattern + intensity byte 0x20.
pub const VIBRATE_COMMAND: [u8; 11] = [0x12, 0x12, 0x08, 0x0A, 0x06, 0x08, 0x02, 0x10, 0x00, 0x18, 0x20];
