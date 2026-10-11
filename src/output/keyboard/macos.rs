//! macOS backend: CGEvent keyboard events posted to the HID event tap.
//!
//! Requires the Accessibility permission (System Settings > Privacy &
//! Security > Accessibility). Key codes are the `kVK_*` virtual key codes from
//! HIToolbox/Events.h.

use anyhow::Result;
use core_graphics::{event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode},
                    event_source::{CGEventSource, CGEventSourceStateID}};
use tracing::info;

use super::{KeyLike, KeyName, Modifier, Profile};
use crate::{events::{Edge, InGameAction},
            output::KeyboardOutput};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
   fn AXIsProcessTrusted() -> bool;
}

pub struct MacosKeyboardBackend
{
   profile: Profile,
}

fn virtual_key(key: KeyName) -> CGKeyCode
{
   match key
   {
      | KeyName::A => 0x00,
      | KeyName::B => 0x0B,
      | KeyName::C => 0x08,
      | KeyName::D => 0x02,
      | KeyName::E => 0x0E,
      | KeyName::F => 0x03,
      | KeyName::G => 0x05,
      | KeyName::H => 0x04,
      | KeyName::I => 0x22,
      | KeyName::J => 0x26,
      | KeyName::K => 0x28,
      | KeyName::L => 0x25,
      | KeyName::M => 0x2E,
      | KeyName::N => 0x2D,
      | KeyName::O => 0x1F,
      | KeyName::P => 0x23,
      | KeyName::Q => 0x0C,
      | KeyName::R => 0x0F,
      | KeyName::S => 0x01,
      | KeyName::T => 0x11,
      | KeyName::U => 0x20,
      | KeyName::V => 0x09,
      | KeyName::W => 0x0D,
      | KeyName::X => 0x07,
      | KeyName::Y => 0x10,
      | KeyName::Z => 0x06,
      | KeyName::Digit0 => 0x1D,
      | KeyName::Digit1 => 0x12,
      | KeyName::Digit2 => 0x13,
      | KeyName::Digit3 => 0x14,
      | KeyName::Digit4 => 0x15,
      | KeyName::Digit5 => 0x17,
      | KeyName::Digit6 => 0x16,
      | KeyName::Digit7 => 0x1A,
      | KeyName::Digit8 => 0x1C,
      | KeyName::Digit9 => 0x19,
      | KeyName::F1 => 0x7A,
      | KeyName::F2 => 0x78,
      | KeyName::F3 => 0x63,
      | KeyName::F4 => 0x76,
      | KeyName::F5 => 0x60,
      | KeyName::F6 => 0x61,
      | KeyName::F7 => 0x62,
      | KeyName::F8 => 0x64,
      | KeyName::F9 => 0x65,
      | KeyName::F10 => 0x6D,
      | KeyName::F11 => 0x67,
      | KeyName::F12 => 0x6F,
      | KeyName::Up => 0x7E,
      | KeyName::Down => 0x7D,
      | KeyName::Left => 0x7B,
      | KeyName::Right => 0x7C,
      | KeyName::PageUp => 0x74,
      | KeyName::PageDown => 0x79,
      | KeyName::Home => 0x73,
      | KeyName::End => 0x77,
      // kVK_Help: Mac keyboards have no Insert key; Help occupies its position.
      | KeyName::Insert => 0x72,
      | KeyName::Delete => 0x75,    // kVK_ForwardDelete
      | KeyName::Backspace => 0x33, // kVK_Delete
      | KeyName::Space => 0x31,
      | KeyName::Enter => 0x24, // kVK_Return
      | KeyName::Escape => 0x35,
      | KeyName::Tab => 0x30,
      | KeyName::Minus => 0x1B,
      | KeyName::Equal => 0x18,
      | KeyName::LeftBracket => 0x21,
      | KeyName::RightBracket => 0x1E,
      | KeyName::Semicolon => 0x29,
      | KeyName::Apostrophe => 0x27, // kVK_ANSI_Quote
      | KeyName::Comma => 0x2B,
      | KeyName::Period => 0x2F,
      | KeyName::Slash => 0x2C,
      | KeyName::Backslash => 0x2A,
      | KeyName::Grave => 0x32,
      | KeyName::Kp0 => 0x52,
      | KeyName::Kp1 => 0x53,
      | KeyName::Kp2 => 0x54,
      | KeyName::Kp3 => 0x55,
      | KeyName::Kp4 => 0x56,
      | KeyName::Kp5 => 0x57,
      | KeyName::Kp6 => 0x58,
      | KeyName::Kp7 => 0x59,
      | KeyName::Kp8 => 0x5B,
      | KeyName::Kp9 => 0x5C,
      | KeyName::KpPlus => 0x45,
      | KeyName::KpMinus => 0x4E,
      | KeyName::KpMultiply => 0x43,
      | KeyName::KpDivide => 0x4B,
      | KeyName::KpEnter => 0x4C,
      | KeyName::KpDecimal => 0x41,
   }
}

fn modifier_key(modifier: Modifier) -> CGKeyCode
{
   match modifier
   {
      | Modifier::Ctrl => 0x3B,  // kVK_Control
      | Modifier::Shift => 0x38, // kVK_Shift
      | Modifier::Alt => 0x3A,   // kVK_Option
      | Modifier::Meta => 0x37,  // kVK_Command
   }
}

fn modifier_flag(modifier: Modifier) -> CGEventFlags
{
   match modifier
   {
      | Modifier::Ctrl => CGEventFlags::CGEventFlagControl,
      | Modifier::Shift => CGEventFlags::CGEventFlagShift,
      | Modifier::Alt => CGEventFlags::CGEventFlagAlternate,
      | Modifier::Meta => CGEventFlags::CGEventFlagCommand,
   }
}

impl MacosKeyboardBackend
{
   pub fn new(profile: Profile) -> Result<Self>
   {
      // SAFETY: plain query with no arguments or side effects.
      if !unsafe { AXIsProcessTrusted() }
      {
         anyhow::bail!("this process is not trusted for Accessibility, so macOS would silently discard injected key events; grant access in System \
                        Settings > Privacy & Security > Accessibility (add the terminal you run bikestick from, or the bikestick binary) and \
                        restart bikestick");
      }
      info!(app = %profile.app, "macOS CGEvent keyboard ready");
      Ok(Self { profile })
   }
}

impl KeyboardOutput for MacosKeyboardBackend
{
   fn supports(&self, action: InGameAction) -> bool { self.profile.supports(action) }

   fn send(&mut self, action: InGameAction, edge: Edge) -> Result<(), anyhow::Error>
   {
      let Some(binding) = self.profile.bindings.get(&action)
      else
      {
         anyhow::bail!("no keyboard mapping for {action:?}");
      };
      let steps = binding.steps(edge);
      if steps.is_empty()
      {
         return Ok(());
      }
      let mut flags = CGEventFlags::CGEventFlagNull;
      for step in steps
      {
         // CGEventSource is not Send, so create it per call instead of storing it.
         let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|()| anyhow::anyhow!("creating CGEventSource failed"))?;
         let (code, is_keypad) = match step.key
         {
            | KeyLike::Key(key) => (virtual_key(key), key.is_keypad()),
            | KeyLike::Modifier(modifier) => (modifier_key(modifier), false),
         };
         // Track the modifier state so every event carries the flags that are
         // held at that moment (key events inherit nothing from the posted
         // modifier events on their own).
         if let KeyLike::Modifier(modifier) = step.key
         {
            flags.set(modifier_flag(modifier), step.down);
         }
         let event =
            CGEvent::new_keyboard_event(source, code, step.down).map_err(|()| anyhow::anyhow!("creating CGEvent for key {code:#04x} failed"))?;
         let mut event_flags = flags;
         if is_keypad
         {
            event_flags |= CGEventFlags::CGEventFlagNumericPad;
         }
         event.set_flags(event_flags);
         event.post(CGEventTapLocation::HID);
      }
      Ok(())
   }
}
