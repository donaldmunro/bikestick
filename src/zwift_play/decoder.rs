//! Decoder for the `PlayKeyPadStatus` proto2 message.
//! proto2 semantics: an absent optional enum field decodes to its default,
//! which the upstream generated code sets to ON (= pressed) for every button.
//! The decoder replicates that, so callers MUST seed their dedup state from
//! the first frame instead of acting on it.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError
{
   #[error("truncated varint")]
   TruncatedVarint,
   #[error("varint too long")]
   OverlongVarint,
   #[error("unsupported wire type {wire_type} for field {field}")]
   UnsupportedWireType
   {
      field: u32, wire_type: u8
   },
   #[error("truncated length-delimited field")]
   TruncatedField,
}

/// Decoded keypad state. Buttons are `true` when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayKeyPadStatus
{
   /// `true` = this frame is from the right-hand controller.
   pub right_pad: bool,
   pub y_up:      bool,
   pub z_left:    bool,
   pub a_right:   bool,
   pub b_down:    bool,
   pub shift:     bool,
   pub on:        bool,
   pub analog_lr: i32,
   pub analog_ud: i32,
}

const BTN_PRESSED: u64 = 0; // PlayButtonStatus.ON

impl Default for PlayKeyPadStatus
{
   /// proto2 defaults: every absent button field reads as ON (pressed),
   /// analog values 0.
   fn default() -> Self
   {
      Self { right_pad: true,
             y_up:      true,
             z_left:    true,
             a_right:   true,
             b_down:    true,
             shift:     true,
             on:        true,
             analog_lr: 0,
             analog_ud: 0, }
   }
}

fn read_varint(data: &[u8], pos: &mut usize) -> Result<u64, DecodeError>
{
   let mut value: u64 = 0;
   for shift in 0..10
   {
      let byte = *data.get(*pos).ok_or(DecodeError::TruncatedVarint)?;
      *pos += 1;
      value |= u64::from(byte & 0x7F) << (shift * 7);
      if byte & 0x80 == 0
      {
         return Ok(value);
      }
   }
   Err(DecodeError::OverlongVarint)
}

fn zigzag(value: u64) -> i32 { ((value >> 1) as i64 ^ -((value & 1) as i64)) as i32 }

impl PlayKeyPadStatus
{
   /// Decode the payload that follows the `0x07` message-type byte.
   ///
   /// `swap_shift_on` applies the old-codebase field assignment
   /// (6 = Button_On, 7 = Button_Shift) instead of the current one.
   pub fn decode(payload: &[u8], swap_shift_on: bool) -> Result<Self, DecodeError>
   {
      let mut status = Self::default();
      let mut pos = 0usize;

      while pos < payload.len()
      {
         let tag = read_varint(payload, &mut pos)?;
         let field = (tag >> 3) as u32;
         let wire_type = (tag & 0x07) as u8;

         match wire_type
         {
            | 0 =>
            {
               let raw = read_varint(payload, &mut pos)?;
               match (field, swap_shift_on)
               {
                  | (1, _) => status.right_pad = raw == BTN_PRESSED,
                  | (2, _) => status.y_up = raw == BTN_PRESSED,
                  | (3, _) => status.z_left = raw == BTN_PRESSED,
                  | (4, _) => status.a_right = raw == BTN_PRESSED,
                  | (5, _) => status.b_down = raw == BTN_PRESSED,
                  | (6, false) | (7, true) => status.shift = raw == BTN_PRESSED,
                  | (7, false) | (6, true) => status.on = raw == BTN_PRESSED,
                  | (8, _) => status.analog_lr = zigzag(raw),
                  | (9, _) => status.analog_ud = zigzag(raw),
                  | _ =>
                  {} // unknown varint field: skip silently
               }
            }
            | 2 =>
            {
               // length-delimited: skip (no such fields expected)
               let len = read_varint(payload, &mut pos)? as usize;
               if pos + len > payload.len()
               {
                  return Err(DecodeError::TruncatedField);
               }
               pos += len;
            }
            | 5 =>
            {
               if pos + 4 > payload.len()
               {
                  return Err(DecodeError::TruncatedField);
               }
               pos += 4;
            }
            | 1 =>
            {
               if pos + 8 > payload.len()
               {
                  return Err(DecodeError::TruncatedField);
               }
               pos += 8;
            }
            | _ => return Err(DecodeError::UnsupportedWireType { field, wire_type }),
         }
      }
      Ok(status)
   }
}

#[cfg(test)]
mod tests
{
   use super::*;

   // Tag helper: field << 3 | wiretype 0.
   // f1=0x08 f2=0x10 f3=0x18 f4=0x20 f5=0x28 f6=0x30 f7=0x38 f8=0x40 f9=0x48

   const RIGHT_IDLE: &[u8] = &[0x08, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01, 0x38, 0x01, 0x40, 0x00, 0x48, 0x00];
   const LEFT_IDLE: &[u8] = &[0x08, 0x01, 0x10, 0x01, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x30, 0x01, 0x38, 0x01, 0x40, 0x00, 0x48, 0x00];

   #[test]
   fn decodes_right_idle()
   {
      let s = PlayKeyPadStatus::decode(RIGHT_IDLE, false).unwrap();
      assert!(s.right_pad);
      assert!(!s.y_up && !s.z_left && !s.a_right && !s.b_down && !s.shift && !s.on);
      assert_eq!(s.analog_lr, 0);
      assert_eq!(s.analog_ud, 0);
   }

   #[test]
   fn decodes_left_idle()
   {
      let s = PlayKeyPadStatus::decode(LEFT_IDLE, false).unwrap();
      assert!(!s.right_pad);
   }

   #[test]
   fn decodes_single_button_press()
   {
      // Right controller, only A pressed (field 4 = ON), others OFF.
      let payload = [0x08, 0x00, 0x10, 0x01, 0x18, 0x01, 0x20, 0x00, 0x28, 0x01, 0x30, 0x01, 0x38, 0x01, 0x40, 0x00];
      let s = PlayKeyPadStatus::decode(&payload, false).unwrap();
      assert!(s.a_right);
      assert!(!s.y_up && !s.z_left && !s.b_down && !s.shift && !s.on);
   }

   #[test]
   fn decodes_positive_paddle_zigzag()
   {
      // f1=ON, f8 = zigzag(200) = +100. Absent fields default to pressed.
      let payload = [0x08, 0x00, 0x40, 0xC8, 0x01];
      let s = PlayKeyPadStatus::decode(&payload, false).unwrap();
      assert_eq!(s.analog_lr, 100);
      assert!(s.right_pad);
      // Absent-field-⇒-ON semantics:
      assert!(s.y_up && s.shift && s.on);
   }

   #[test]
   fn decodes_negative_paddle_zigzag()
   {
      // f8 = zigzag(199) = -100.
      let payload = [0x08, 0x01, 0x40, 0xC7, 0x01];
      let s = PlayKeyPadStatus::decode(&payload, false).unwrap();
      assert_eq!(s.analog_lr, -100);
      assert!(!s.right_pad);
   }

   #[test]
   fn swap_shift_on_swaps_fields_6_and_7()
   {
      // field 6 = ON, field 7 = OFF
      let payload = [0x08, 0x00, 0x30, 0x00, 0x38, 0x01, 0x40, 0x00];
      let normal = PlayKeyPadStatus::decode(&payload, false).unwrap();
      assert!(normal.shift && !normal.on);
      let swapped = PlayKeyPadStatus::decode(&payload, true).unwrap();
      assert!(!swapped.shift && swapped.on);
   }

   #[test]
   fn rejects_truncated_varint()
   {
      assert_eq!(PlayKeyPadStatus::decode(&[0x40, 0xC8], false), Err(DecodeError::TruncatedVarint));
   }

   #[test]
   fn skips_unknown_fields()
   {
      // field 15 varint + field 12 length-delimited of 2 bytes, then f8=+100
      let payload = [0x78, 0x05, 0x62, 0x02, 0xAA, 0xBB, 0x40, 0xC8, 0x01];
      let s = PlayKeyPadStatus::decode(&payload, false).unwrap();
      assert_eq!(s.analog_lr, 100);
   }
}
