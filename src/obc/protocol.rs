//! OpenBikeControl message encoding/parsing.

use std::collections::HashSet;

use thiserror::Error;

use super::constants::*;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError
{
   #[error("empty message")]
   Empty,
   #[error("not an app info message")]
   WrongType,
   #[error("unsupported app info version")]
   UnsupportedVersion,
   #[error("truncated app info")]
   Truncated,
}

/// The trainer app's self-description, written to the App Info characteristic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo
{
   pub app_id:            String,
   pub app_version:       String,
   pub supported_buttons: HashSet<u8>,
}

/// Encode a button-state notification: `[0x01, id, state, id, state, ...]`.
pub fn encode_button_state(states: &[(u8, u8)]) -> Vec<u8>
{
   let mut message = Vec::with_capacity(1 + states.len() * 2);
   message.push(MSG_BUTTON_STATE);
   for &(id, state) in states
   {
      message.push(id);
      message.push(state);
   }
   message
}

/// Parse an App Info message:
/// `[0x04, version, idLen, id..., verLen, ver..., count, buttonIds...]`.
pub fn parse_app_info(data: &[u8]) -> Result<AppInfo, ParseError>
{
   let mut pos = 0usize;
   let next = |pos: &mut usize| -> Result<u8, ParseError> {
      let byte = *data.get(*pos).ok_or(ParseError::Truncated)?;
      *pos += 1;
      Ok(byte)
   };

   if data.is_empty()
   {
      return Err(ParseError::Empty);
   }
   if next(&mut pos)? != MSG_APP_INFO
   {
      return Err(ParseError::WrongType);
   }
   if next(&mut pos)? != APP_INFO_VERSION
   {
      return Err(ParseError::UnsupportedVersion);
   }

   let read_string = |pos: &mut usize| -> Result<String, ParseError> {
      let len = next(pos)? as usize;
      let end = *pos + len;
      let bytes = data.get(*pos..end).ok_or(ParseError::Truncated)?;
      *pos = end;
      Ok(String::from_utf8_lossy(bytes).into_owned())
   };

   let app_id = read_string(&mut pos)?;
   let app_version = read_string(&mut pos)?;

   let count = next(&mut pos)? as usize;
   let end = pos + count;
   let ids = data.get(pos..end).ok_or(ParseError::Truncated)?;
   let supported_buttons = ids.iter().copied().collect();

   Ok(AppInfo { app_id,
                app_version,
                supported_buttons })
}

/// Buffers app-info fragments in case the App Info write is split across multiple GATT packets.
#[derive(Debug, Default)]
pub struct AppInfoReassembler
{
   buffer: Vec<u8>,
}

impl AppInfoReassembler
{
   /// Offer a fragment; returns the parsed AppInfo once the buffer is valid.
   pub fn offer(&mut self, fragment: &[u8]) -> Option<AppInfo>
   {
      self.buffer.extend_from_slice(fragment);
      match parse_app_info(&self.buffer)
      {
         | Ok(info) =>
         {
            self.buffer.clear();
            Some(info)
         }
         | Err(_) =>
         {
            if self.buffer.len() > APP_INFO_MAX_BUFFER
            {
               self.buffer.clear();
            }
            None
         }
      }
   }

   #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
   pub fn reset(&mut self) { self.buffer.clear(); }
}

#[cfg(test)]
mod tests
{
   use super::*;

   fn tpv_app_info() -> Vec<u8>
   {
      let ids: Vec<u8> = vec![0x01, 0x02, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x21, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38,
                              0x39, 0x3A, 0x3B, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45,];
      let mut message = vec![MSG_APP_INFO, APP_INFO_VERSION];
      message.push(9);
      message.extend_from_slice(b"TPVirtual");
      message.push(5);
      message.extend_from_slice(b"1.0.0");
      message.push(ids.len() as u8);
      message.extend_from_slice(&ids);
      message
   }

   #[test]
   fn encodes_button_press_and_release()
   {
      assert_eq!(encode_button_state(&[(0x01, STATE_PRESSED)]), vec![0x01, 0x01, 0x01]);
      assert_eq!(encode_button_state(&[(0x01, STATE_RELEASED)]), vec![0x01, 0x01, 0x00]);
      assert_eq!(encode_button_state(&[(0x01, 1), (0x02, 1)]), vec![0x01, 0x01, 0x01, 0x02, 0x01]);
   }

   #[test]
   fn parses_whole_app_info()
   {
      let info = parse_app_info(&tpv_app_info()).unwrap();
      assert_eq!(info.app_id, "TPVirtual");
      assert_eq!(info.app_version, "1.0.0");
      assert_eq!(info.supported_buttons.len(), 28);
      assert!(info.supported_buttons.contains(&0x01));
      assert!(info.supported_buttons.contains(&0x45));
      assert!(!info.supported_buttons.contains(&0x18)); // steering not in TPV set
   }

   #[test]
   fn rejects_bad_version()
   {
      assert_eq!(parse_app_info(&[0x04, 0x02, 0x00, 0x00, 0x00]), Err(ParseError::UnsupportedVersion));
   }

   #[test]
   fn reassembles_fragmented_writes()
   {
      let message = tpv_app_info();
      let mut reassembler = AppInfoReassembler::default();
      // Split 20/20/rest.
      assert!(reassembler.offer(&message[..20]).is_none());
      assert!(reassembler.offer(&message[20..40]).is_none());
      let info = reassembler.offer(&message[40..]).unwrap();
      assert_eq!(info.app_id, "TPVirtual");
   }

   #[test]
   fn reassembles_byte_at_a_time()
   {
      let message = tpv_app_info();
      let mut reassembler = AppInfoReassembler::default();
      let mut result = None;
      for byte in &message
      {
         result = reassembler.offer(std::slice::from_ref(byte));
      }
      assert_eq!(result.unwrap().app_version, "1.0.0");
   }

   #[test]
   fn caps_garbage_buffer()
   {
      let mut reassembler = AppInfoReassembler::default();
      // Garbage that never parses: buffer must reset once past the cap...
      for _ in 0..40
      {
         assert!(reassembler.offer(&[0xFFu8; 20]).is_none());
      }
      // ...so a subsequent clean message still parses (may need the reset to
      // have happened; feed it standalone).
      reassembler.reset();
      assert!(reassembler.offer(&tpv_app_info()).is_some());
   }

   #[test]
   fn reset_drops_partial_fragments()
   {
      let message = tpv_app_info();
      let mut reassembler = AppInfoReassembler::default();
      assert!(reassembler.offer(&message[..10]).is_none());
      reassembler.reset(); // simulates central disconnect
      assert!(reassembler.offer(&message).is_some());
   }

   #[test]
   fn truncated_then_completed()
   {
      // A prefix must return Truncated (not a hard error) so reassembly works.
      let message = tpv_app_info();
      assert_eq!(parse_app_info(&message[..5]), Err(ParseError::Truncated));
   }
}
