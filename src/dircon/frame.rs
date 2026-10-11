//! DirCon (Wahoo Direct Connect) wire format.
//!
//! Adapted from the author's bt-bridge project (`/src/Rust/bt-bridge`), whose
//! codec was validated against a real Wahoo KICKR Direct Connect capture:
//!
//! ```text
//! version(1=0x01) | msgType(1) | seq_LE(2) | paylen_BE(2) | payload(paylen)
//! ```
//!
//! `seq` is a notification counter (LE u16, increments per notification; zero
//! for all other message types). `paylen` is the exact payload byte count.

use bytes::{BufMut, Bytes, BytesMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType
{
   DiscoverServices = 0x01,
   DiscoverChars = 0x02,
   ReadChar = 0x03,
   WriteChar = 0x04,
   Subscribe = 0x05,
   Notification = 0x06,
}

impl TryFrom<u8> for MsgType
{
   type Error = u8;

   fn try_from(v: u8) -> Result<Self, u8>
   {
      match v
      {
         | 0x01 => Ok(MsgType::DiscoverServices),
         | 0x02 => Ok(MsgType::DiscoverChars),
         | 0x03 => Ok(MsgType::ReadChar),
         | 0x04 => Ok(MsgType::WriteChar),
         | 0x05 => Ok(MsgType::Subscribe),
         | 0x06 => Ok(MsgType::Notification),
         | other => Err(other),
      }
   }
}

/// Characteristic property flags used in DiscoverChars responses.
pub mod char_flags
{
   #[allow(dead_code)]
   pub const READ: u8 = 0x01;
   pub const WRITE: u8 = 0x02;
   pub const NOTIFY: u8 = 0x04;
}

/// Frame-level protocol violation: the stream cannot be resynchronised, so the
/// caller should close the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError
{
   #[error("unsupported DirCon version byte {0:#04X}")]
   BadVersion(u8),
   #[error("unknown DirCon message type {0:#04X}")]
   UnknownMsgType(u8),
}

#[derive(Debug, Clone)]
pub struct DirconFrame
{
   pub msg_type: MsgType,
   /// Notification sequence counter (LE u16). Zero for non-notification frames.
   pub seq:      u16,
   pub payload:  Bytes,
}

impl DirconFrame
{
   pub const HEADER_LEN: usize = 6;
   const VERSION: u8 = 0x01;

   /// Parse one frame from the front of `buf`. `Ok(Some((frame, consumed)))`
   /// on a complete frame, `Ok(None)` if more bytes are needed, `Err` on a
   /// protocol violation.
   pub fn parse(buf: &[u8]) -> Result<Option<(DirconFrame, usize)>, FrameError>
   {
      if buf.len() < Self::HEADER_LEN
      {
         return Ok(None);
      }
      if buf[0] != Self::VERSION
      {
         return Err(FrameError::BadVersion(buf[0]));
      }
      let msg_type = MsgType::try_from(buf[1]).map_err(FrameError::UnknownMsgType)?;
      let seq = u16::from_le_bytes([buf[2], buf[3]]);
      let paylen = u16::from_be_bytes([buf[4], buf[5]]) as usize;
      let total = Self::HEADER_LEN + paylen;
      if buf.len() < total
      {
         return Ok(None);
      }
      let payload = Bytes::copy_from_slice(&buf[Self::HEADER_LEN..total]);
      Ok(Some((DirconFrame { msg_type, seq, payload }, total)))
   }

   pub fn serialize(&self) -> Bytes
   {
      let paylen = self.payload.len();
      let mut buf = BytesMut::with_capacity(Self::HEADER_LEN + paylen);
      buf.put_u8(Self::VERSION);
      buf.put_u8(self.msg_type as u8);
      buf.put_u16_le(self.seq);
      buf.put_u16(paylen as u16); // big-endian
      buf.put_slice(&self.payload);
      buf.freeze()
   }

   pub fn reply(msg_type: MsgType, payload: impl Into<Bytes>) -> Self
   {
      Self { msg_type,
             seq: 0,
             payload: payload.into() }
   }

   pub fn notification(seq: u16, payload: impl Into<Bytes>) -> Self
   {
      Self { msg_type: MsgType::Notification,
             seq,
             payload: payload.into() }
   }
}

/// Convert a `uuid::Uuid` to the 16-byte wire representation used in DirCon.
pub fn uuid_to_bytes(uuid: uuid::Uuid) -> [u8; 16] { *uuid.as_bytes() }

/// Parse a 16-byte UUID from a DirCon payload offset.
pub fn uuid_from_bytes(bytes: &[u8]) -> Option<uuid::Uuid>
{
   let arr: [u8; 16] = bytes.get(..16)?.try_into().ok()?;
   Some(uuid::Uuid::from_bytes(arr))
}

#[cfg(test)]
mod tests
{
   use super::*;

   #[test]
   fn round_trip_with_payload()
   {
      let frame = DirconFrame::notification(42, vec![0xDE, 0xAD, 0xBE, 0xEF]);
      let bytes = frame.serialize();
      let (parsed, consumed) = DirconFrame::parse(&bytes).unwrap().unwrap();
      assert_eq!(consumed, bytes.len());
      assert_eq!(parsed.msg_type, MsgType::Notification);
      assert_eq!(parsed.seq, 42);
      assert_eq!(&parsed.payload[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
   }

   #[test]
   fn partial_buffer_returns_none()
   {
      let frame = DirconFrame::notification(1, vec![1, 2, 3]);
      let bytes = frame.serialize();
      assert!(DirconFrame::parse(&bytes[..bytes.len() - 1]).unwrap()
                                                           .is_none());
   }

   #[test]
   fn real_capture_discover_services_request()
   {
      // Frame 8 from the Wahoo KICKR capture in bt-bridge.
      let raw = [0x01, 0x01, 0x00, 0x00, 0x00, 0x00];
      let (frame, consumed) = DirconFrame::parse(&raw).unwrap().unwrap();
      assert_eq!(consumed, 6);
      assert_eq!(frame.msg_type, MsgType::DiscoverServices);
      assert!(frame.payload.is_empty());
   }

   #[test]
   fn bad_version_is_a_protocol_violation()
   {
      let raw = [0xFF, 0x01, 0x00, 0x00, 0x00, 0x00];
      assert!(matches!(DirconFrame::parse(&raw), Err(FrameError::BadVersion(0xFF))));
   }

   #[test]
   fn obc_uuid_round_trips()
   {
      let uuid = crate::obc::constants::OBC_SERVICE;
      assert_eq!(uuid_from_bytes(&uuid_to_bytes(uuid)).unwrap(), uuid);
   }
}
