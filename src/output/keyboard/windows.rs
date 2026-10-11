//! Windows backend: `SendInput` with hardware scan codes (set 1).
//!
//! Scan codes are used instead of virtual-key codes because games using
//! DirectInput or raw input ignore VK-only synthetic events.

use anyhow::Result;
use tracing::info;
use windows::Win32::UI::Input::KeyboardAndMouse::{INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
                                                  KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, SendInput, VIRTUAL_KEY};

use super::{KeyLike, KeyName, Modifier, Profile};
use crate::{events::{Edge, InGameAction},
            output::KeyboardOutput};

pub struct WindowsKeyboardBackend
{
   profile: Profile,
}

/// Set-1 scan code and whether the key needs the E0 (extended) prefix.
///
/// Keypad keys use the plain (non-extended) codes, so with NumLock off the
/// system translates them to their navigation function.
fn scan_code(key: KeyName) -> (u16, bool)
{
   match key
   {
      | KeyName::A => (0x1E, false),
      | KeyName::B => (0x30, false),
      | KeyName::C => (0x2E, false),
      | KeyName::D => (0x20, false),
      | KeyName::E => (0x12, false),
      | KeyName::F => (0x21, false),
      | KeyName::G => (0x22, false),
      | KeyName::H => (0x23, false),
      | KeyName::I => (0x17, false),
      | KeyName::J => (0x24, false),
      | KeyName::K => (0x25, false),
      | KeyName::L => (0x26, false),
      | KeyName::M => (0x32, false),
      | KeyName::N => (0x31, false),
      | KeyName::O => (0x18, false),
      | KeyName::P => (0x19, false),
      | KeyName::Q => (0x10, false),
      | KeyName::R => (0x13, false),
      | KeyName::S => (0x1F, false),
      | KeyName::T => (0x14, false),
      | KeyName::U => (0x16, false),
      | KeyName::V => (0x2F, false),
      | KeyName::W => (0x11, false),
      | KeyName::X => (0x2D, false),
      | KeyName::Y => (0x15, false),
      | KeyName::Z => (0x2C, false),
      | KeyName::Digit0 => (0x0B, false),
      | KeyName::Digit1 => (0x02, false),
      | KeyName::Digit2 => (0x03, false),
      | KeyName::Digit3 => (0x04, false),
      | KeyName::Digit4 => (0x05, false),
      | KeyName::Digit5 => (0x06, false),
      | KeyName::Digit6 => (0x07, false),
      | KeyName::Digit7 => (0x08, false),
      | KeyName::Digit8 => (0x09, false),
      | KeyName::Digit9 => (0x0A, false),
      | KeyName::F1 => (0x3B, false),
      | KeyName::F2 => (0x3C, false),
      | KeyName::F3 => (0x3D, false),
      | KeyName::F4 => (0x3E, false),
      | KeyName::F5 => (0x3F, false),
      | KeyName::F6 => (0x40, false),
      | KeyName::F7 => (0x41, false),
      | KeyName::F8 => (0x42, false),
      | KeyName::F9 => (0x43, false),
      | KeyName::F10 => (0x44, false),
      | KeyName::F11 => (0x57, false),
      | KeyName::F12 => (0x58, false),
      | KeyName::Up => (0x48, true),
      | KeyName::Down => (0x50, true),
      | KeyName::Left => (0x4B, true),
      | KeyName::Right => (0x4D, true),
      | KeyName::PageUp => (0x49, true),
      | KeyName::PageDown => (0x51, true),
      | KeyName::Home => (0x47, true),
      | KeyName::End => (0x4F, true),
      | KeyName::Insert => (0x52, true),
      | KeyName::Delete => (0x53, true),
      | KeyName::Backspace => (0x0E, false),
      | KeyName::Space => (0x39, false),
      | KeyName::Enter => (0x1C, false),
      | KeyName::Escape => (0x01, false),
      | KeyName::Tab => (0x0F, false),
      | KeyName::Minus => (0x0C, false),
      | KeyName::Equal => (0x0D, false),
      | KeyName::LeftBracket => (0x1A, false),
      | KeyName::RightBracket => (0x1B, false),
      | KeyName::Semicolon => (0x27, false),
      | KeyName::Apostrophe => (0x28, false),
      | KeyName::Comma => (0x33, false),
      | KeyName::Period => (0x34, false),
      | KeyName::Slash => (0x35, false),
      | KeyName::Backslash => (0x2B, false),
      | KeyName::Grave => (0x29, false),
      | KeyName::Kp0 => (0x52, false),
      | KeyName::Kp1 => (0x4F, false),
      | KeyName::Kp2 => (0x50, false),
      | KeyName::Kp3 => (0x51, false),
      | KeyName::Kp4 => (0x4B, false),
      | KeyName::Kp5 => (0x4C, false),
      | KeyName::Kp6 => (0x4D, false),
      | KeyName::Kp7 => (0x47, false),
      | KeyName::Kp8 => (0x48, false),
      | KeyName::Kp9 => (0x49, false),
      | KeyName::KpPlus => (0x4E, false),
      | KeyName::KpMinus => (0x4A, false),
      | KeyName::KpMultiply => (0x37, false),
      | KeyName::KpDivide => (0x35, true),
      | KeyName::KpEnter => (0x1C, true),
      | KeyName::KpDecimal => (0x53, false),
   }
}

fn modifier_scan_code(modifier: Modifier) -> (u16, bool)
{
   match modifier
   {
      | Modifier::Ctrl => (0x1D, false),
      | Modifier::Shift => (0x2A, false),
      | Modifier::Alt => (0x38, false),
      | Modifier::Meta => (0x5B, true),
   }
}

fn keyboard_input(key: KeyLike, down: bool) -> INPUT
{
   let (scan, extended) = match key
   {
      | KeyLike::Key(key) => scan_code(key),
      | KeyLike::Modifier(modifier) => modifier_scan_code(modifier),
   };
   let mut flags: KEYBD_EVENT_FLAGS = KEYEVENTF_SCANCODE;
   if extended
   {
      flags |= KEYEVENTF_EXTENDEDKEY;
   }
   if !down
   {
      flags |= KEYEVENTF_KEYUP;
   }
   INPUT { r#type:    INPUT_KEYBOARD,
           Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk:         VIRTUAL_KEY(0),
                                                 wScan:       scan,
                                                 dwFlags:     flags,
                                                 time:        0,
                                                 dwExtraInfo: 0, }, }, }
}

impl WindowsKeyboardBackend
{
   pub fn new(profile: Profile) -> Result<Self>
   {
      info!(app = %profile.app, "Windows SendInput keyboard ready");
      Ok(Self { profile })
   }
}

impl KeyboardOutput for WindowsKeyboardBackend
{
   fn supports(&self, action: InGameAction) -> bool { self.profile.supports(action) }

   fn send(&mut self, action: InGameAction, edge: Edge) -> Result<(), anyhow::Error>
   {
      let Some(binding) = self.profile.bindings.get(&action)
      else
      {
         anyhow::bail!("no keyboard mapping for {action:?}");
      };
      let inputs: Vec<INPUT> = binding.steps(edge)
                                      .into_iter()
                                      .map(|step| keyboard_input(step.key, step.down))
                                      .collect();
      if inputs.is_empty()
      {
         return Ok(());
      }
      // SAFETY: `inputs` is a valid slice of fully initialised INPUT structs and
      // the size argument is the size of one element.
      let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
      if sent != inputs.len()
      {
         let os_error = windows::core::Error::from_thread();
         anyhow::bail!("SendInput injected {sent} of {} events ({os_error}); input into an elevated/admin application is blocked by UIPI unless \
                        bikestick also runs elevated",
                       inputs.len());
      }
      Ok(())
   }
}
