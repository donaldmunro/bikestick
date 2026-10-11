//! uinput virtual-keyboard backend (works under X11 and Wayland).
//!
//! Must have keyboard focus as global key injection cannot target an
//! unfocused window.

use anyhow::{Context, Result};
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, uinput::VirtualDevice};
use tracing::info;

use super::{KeyLike, KeyName, Modifier, Profile};
use crate::{events::{Edge, InGameAction},
            output::KeyboardOutput};

pub struct LinuxKeyboardBackend
{
   device:  VirtualDevice,
   profile: Profile,
}

fn evdev_key(key: KeyName) -> KeyCode
{
   match key
   {
      | KeyName::A => KeyCode::KEY_A,
      | KeyName::B => KeyCode::KEY_B,
      | KeyName::C => KeyCode::KEY_C,
      | KeyName::D => KeyCode::KEY_D,
      | KeyName::E => KeyCode::KEY_E,
      | KeyName::F => KeyCode::KEY_F,
      | KeyName::G => KeyCode::KEY_G,
      | KeyName::H => KeyCode::KEY_H,
      | KeyName::I => KeyCode::KEY_I,
      | KeyName::J => KeyCode::KEY_J,
      | KeyName::K => KeyCode::KEY_K,
      | KeyName::L => KeyCode::KEY_L,
      | KeyName::M => KeyCode::KEY_M,
      | KeyName::N => KeyCode::KEY_N,
      | KeyName::O => KeyCode::KEY_O,
      | KeyName::P => KeyCode::KEY_P,
      | KeyName::Q => KeyCode::KEY_Q,
      | KeyName::R => KeyCode::KEY_R,
      | KeyName::S => KeyCode::KEY_S,
      | KeyName::T => KeyCode::KEY_T,
      | KeyName::U => KeyCode::KEY_U,
      | KeyName::V => KeyCode::KEY_V,
      | KeyName::W => KeyCode::KEY_W,
      | KeyName::X => KeyCode::KEY_X,
      | KeyName::Y => KeyCode::KEY_Y,
      | KeyName::Z => KeyCode::KEY_Z,
      | KeyName::Digit0 => KeyCode::KEY_0,
      | KeyName::Digit1 => KeyCode::KEY_1,
      | KeyName::Digit2 => KeyCode::KEY_2,
      | KeyName::Digit3 => KeyCode::KEY_3,
      | KeyName::Digit4 => KeyCode::KEY_4,
      | KeyName::Digit5 => KeyCode::KEY_5,
      | KeyName::Digit6 => KeyCode::KEY_6,
      | KeyName::Digit7 => KeyCode::KEY_7,
      | KeyName::Digit8 => KeyCode::KEY_8,
      | KeyName::Digit9 => KeyCode::KEY_9,
      | KeyName::F1 => KeyCode::KEY_F1,
      | KeyName::F2 => KeyCode::KEY_F2,
      | KeyName::F3 => KeyCode::KEY_F3,
      | KeyName::F4 => KeyCode::KEY_F4,
      | KeyName::F5 => KeyCode::KEY_F5,
      | KeyName::F6 => KeyCode::KEY_F6,
      | KeyName::F7 => KeyCode::KEY_F7,
      | KeyName::F8 => KeyCode::KEY_F8,
      | KeyName::F9 => KeyCode::KEY_F9,
      | KeyName::F10 => KeyCode::KEY_F10,
      | KeyName::F11 => KeyCode::KEY_F11,
      | KeyName::F12 => KeyCode::KEY_F12,
      | KeyName::Up => KeyCode::KEY_UP,
      | KeyName::Down => KeyCode::KEY_DOWN,
      | KeyName::Left => KeyCode::KEY_LEFT,
      | KeyName::Right => KeyCode::KEY_RIGHT,
      | KeyName::PageUp => KeyCode::KEY_PAGEUP,
      | KeyName::PageDown => KeyCode::KEY_PAGEDOWN,
      | KeyName::Home => KeyCode::KEY_HOME,
      | KeyName::End => KeyCode::KEY_END,
      | KeyName::Insert => KeyCode::KEY_INSERT,
      | KeyName::Delete => KeyCode::KEY_DELETE,
      | KeyName::Backspace => KeyCode::KEY_BACKSPACE,
      | KeyName::Space => KeyCode::KEY_SPACE,
      | KeyName::Enter => KeyCode::KEY_ENTER,
      | KeyName::Escape => KeyCode::KEY_ESC,
      | KeyName::Tab => KeyCode::KEY_TAB,
      | KeyName::Minus => KeyCode::KEY_MINUS,
      | KeyName::Equal => KeyCode::KEY_EQUAL,
      | KeyName::LeftBracket => KeyCode::KEY_LEFTBRACE,
      | KeyName::RightBracket => KeyCode::KEY_RIGHTBRACE,
      | KeyName::Semicolon => KeyCode::KEY_SEMICOLON,
      | KeyName::Apostrophe => KeyCode::KEY_APOSTROPHE,
      | KeyName::Comma => KeyCode::KEY_COMMA,
      | KeyName::Period => KeyCode::KEY_DOT,
      | KeyName::Slash => KeyCode::KEY_SLASH,
      | KeyName::Backslash => KeyCode::KEY_BACKSLASH,
      | KeyName::Grave => KeyCode::KEY_GRAVE,
      | KeyName::Kp0 => KeyCode::KEY_KP0,
      | KeyName::Kp1 => KeyCode::KEY_KP1,
      | KeyName::Kp2 => KeyCode::KEY_KP2,
      | KeyName::Kp3 => KeyCode::KEY_KP3,
      | KeyName::Kp4 => KeyCode::KEY_KP4,
      | KeyName::Kp5 => KeyCode::KEY_KP5,
      | KeyName::Kp6 => KeyCode::KEY_KP6,
      | KeyName::Kp7 => KeyCode::KEY_KP7,
      | KeyName::Kp8 => KeyCode::KEY_KP8,
      | KeyName::Kp9 => KeyCode::KEY_KP9,
      | KeyName::KpPlus => KeyCode::KEY_KPPLUS,
      | KeyName::KpMinus => KeyCode::KEY_KPMINUS,
      | KeyName::KpMultiply => KeyCode::KEY_KPASTERISK,
      | KeyName::KpDivide => KeyCode::KEY_KPSLASH,
      | KeyName::KpEnter => KeyCode::KEY_KPENTER,
      | KeyName::KpDecimal => KeyCode::KEY_KPDOT,
   }
}

fn evdev_modifier(modifier: Modifier) -> KeyCode
{
   match modifier
   {
      | Modifier::Ctrl => KeyCode::KEY_LEFTCTRL,
      | Modifier::Shift => KeyCode::KEY_LEFTSHIFT,
      | Modifier::Alt => KeyCode::KEY_LEFTALT,
      | Modifier::Meta => KeyCode::KEY_LEFTMETA,
   }
}

fn evdev_code(key: KeyLike) -> KeyCode
{
   match key
   {
      | KeyLike::Key(key) => evdev_key(key),
      | KeyLike::Modifier(modifier) => evdev_modifier(modifier),
   }
}

impl LinuxKeyboardBackend
{
   pub fn new(profile: Profile) -> Result<Self>
   {
      // Register the union of every key and modifier the profile can emit;
      // uinput silently drops events for keys that were not registered.
      let mut keys = AttributeSet::<KeyCode>::new();
      for binding in profile.bindings.values()
      {
         for step in binding.steps(Edge::Pressed)
         {
            keys.insert(evdev_code(step.key));
         }
      }
      if keys.iter().next().is_none()
      {
         // uinput requires at least one key capability.
         keys.insert(KeyCode::KEY_KPPLUS);
      }
      let device = VirtualDevice::builder().context("opening /dev/uinput (is the udev rule installed and are you in the 'input' group?)")?
                                           .name("bikestick virtual keyboard")
                                           .with_keys(&keys)
                                           .context("registering virtual keys")?
                                           .build()
                                           .context("creating uinput device")?;
      info!(app = %profile.app, "uinput virtual keyboard created");
      Ok(Self { device, profile })
   }
}

impl KeyboardOutput for LinuxKeyboardBackend
{
   fn supports(&self, action: InGameAction) -> bool { self.profile.supports(action) }

   fn send(&mut self, action: InGameAction, edge: Edge) -> Result<(), anyhow::Error>
   {
      let Some(binding) = self.profile.bindings.get(&action)
      else
      {
         anyhow::bail!("no keyboard mapping for {action:?}");
      };
      let events: Vec<InputEvent> = binding.steps(edge)
                                           .into_iter()
                                           .map(|step| InputEvent::new(EventType::KEY.0, evdev_code(step.key).code(), i32::from(step.down)))
                                           .collect();
      if events.is_empty()
      {
         return Ok(());
      }
      // `emit` appends the SYN_REPORT itself, so the whole batch is one report.
      self.device.emit(&events).context("emitting key events")?;
      Ok(())
   }
}
