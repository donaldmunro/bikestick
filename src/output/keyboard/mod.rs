//! Configurable per-application keyboard bindings and the platform backends
//! that inject them (uinput on Linux, SendInput on Windows, CGEvent on macOS).
//!
//! Bindings live in TOML "profiles" (one per cycling application). A profile
//! is looked up in `<config dir>/keyboard/<name>.toml` first, then in the
//! defaults embedded in the binary (see `profiles::Catalog`); a file path may
//! be given instead of a name.

#[cfg(target_os = "linux")] mod linux;
#[cfg(target_os = "macos")] mod macos;
#[cfg(windows)] mod windows;

use std::{collections::HashMap, fmt, path::PathBuf, str::FromStr};

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, de};
use tracing::{info, warn};

use crate::{events::{Edge, InGameAction},
            output::KeyboardOutput,
            profiles::{Catalog, is_path}};

/// Create the keyboard backend for this OS driven by `profile`.
pub fn create_backend(profile: Profile) -> Result<Box<dyn KeyboardOutput>>
{
   #[cfg(target_os = "linux")]
   {
      Ok(Box::new(linux::LinuxKeyboardBackend::new(profile)?))
   }
   #[cfg(windows)]
   {
      Ok(Box::new(windows::WindowsKeyboardBackend::new(profile)?))
   }
   #[cfg(target_os = "macos")]
   {
      Ok(Box::new(macos::MacosKeyboardBackend::new(profile)?))
   }
   #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
   {
      let _ = profile;
      anyhow::bail!("keyboard output not supported on this platform")
   }
}

// ---------------------------------------------------------------------------
// Key names
// ---------------------------------------------------------------------------

macro_rules! key_names {
   ($($variant:ident => $name:literal),+ $(,)?) => {
      /// Platform-neutral key identifier. Names are snake_case in profiles.
      #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
      pub enum KeyName
      {
         $($variant),+
      }

      impl KeyName
      {
         /// Every key, for tests.
         #[cfg(test)]
         pub const ALL: &'static [KeyName] = &[$(KeyName::$variant),+];

         pub fn name(self) -> &'static str
         {
            match self
            {
               $(| KeyName::$variant => $name),+
            }
         }

         fn lookup(name: &str) -> Option<KeyName>
         {
            match name
            {
               $(| $name => Some(KeyName::$variant),)+
               | _ => None,
            }
         }
      }
   };
}

key_names! {
   A => "a", B => "b", C => "c", D => "d", E => "e", F => "f", G => "g", H => "h", I => "i",
   J => "j", K => "k", L => "l", M => "m", N => "n", O => "o", P => "p", Q => "q", R => "r",
   S => "s", T => "t", U => "u", V => "v", W => "w", X => "x", Y => "y", Z => "z",
   Digit0 => "digit_0", Digit1 => "digit_1", Digit2 => "digit_2", Digit3 => "digit_3", Digit4 => "digit_4",
   Digit5 => "digit_5", Digit6 => "digit_6", Digit7 => "digit_7", Digit8 => "digit_8", Digit9 => "digit_9",
   F1 => "f1", F2 => "f2", F3 => "f3", F4 => "f4", F5 => "f5", F6 => "f6",
   F7 => "f7", F8 => "f8", F9 => "f9", F10 => "f10", F11 => "f11", F12 => "f12",
   Up => "up", Down => "down", Left => "left", Right => "right",
   PageUp => "page_up", PageDown => "page_down", Home => "home", End => "end",
   Insert => "insert", Delete => "delete", Backspace => "backspace", Space => "space",
   Enter => "enter", Escape => "escape", Tab => "tab",
   Minus => "minus", Equal => "equal", LeftBracket => "left_bracket", RightBracket => "right_bracket",
   Semicolon => "semicolon", Apostrophe => "apostrophe", Comma => "comma", Period => "period",
   Slash => "slash", Backslash => "backslash", Grave => "grave",
   Kp0 => "kp_0", Kp1 => "kp_1", Kp2 => "kp_2", Kp3 => "kp_3", Kp4 => "kp_4",
   Kp5 => "kp_5", Kp6 => "kp_6", Kp7 => "kp_7", Kp8 => "kp_8", Kp9 => "kp_9",
   KpPlus => "kp_plus", KpMinus => "kp_minus", KpMultiply => "kp_multiply", KpDivide => "kp_divide",
   KpEnter => "kp_enter", KpDecimal => "kp_decimal",
}

impl KeyName
{
   /// True for numeric-keypad keys (macOS marks these with a flag).
   #[cfg(target_os = "macos")]
   pub fn is_keypad(self) -> bool
   {
      matches!(self, |KeyName::Kp0| KeyName::Kp1
                                    | KeyName::Kp2
                                    | KeyName::Kp3
                                    | KeyName::Kp4
                                    | KeyName::Kp5
                                    | KeyName::Kp6
                                    | KeyName::Kp7
                                    | KeyName::Kp8
                                    | KeyName::Kp9
                                    | KeyName::KpPlus
                                    | KeyName::KpMinus
                                    | KeyName::KpMultiply
                                    | KeyName::KpDivide
                                    | KeyName::KpEnter
                                    | KeyName::KpDecimal)
   }
}

impl fmt::Display for KeyName
{
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.name()) }
}

impl FromStr for KeyName
{
   type Err = ParseError;

   fn from_str(text: &str) -> Result<Self, ParseError>
   {
      let lower = text.trim().to_ascii_lowercase();
      // "0".."9" are accepted as aliases of digit_0..digit_9.
      let canonical = match lower.as_str()
      {
         | digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => format!("digit_{digit}"),
         | _ => lower,
      };
      KeyName::lookup(&canonical).ok_or_else(|| ParseError::UnknownKey(text.trim().to_string()))
   }
}

impl<'de> Deserialize<'de> for KeyName
{
   fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
   {
      let text = String::deserialize(deserializer)?;
      text.parse().map_err(de::Error::custom)
   }
}

// ---------------------------------------------------------------------------
// Modifiers and chords
// ---------------------------------------------------------------------------

/// Declaration order is the canonical chord order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Modifier
{
   Ctrl,
   Shift,
   Alt,
   Meta,
}

impl Modifier
{
   pub fn name(self) -> &'static str
   {
      match self
      {
         | Modifier::Ctrl => "ctrl",
         | Modifier::Shift => "shift",
         | Modifier::Alt => "alt",
         | Modifier::Meta => "meta",
      }
   }

   fn lookup(name: &str) -> Option<Modifier>
   {
      match name
      {
         | "ctrl" | "control" => Some(Modifier::Ctrl),
         | "shift" => Some(Modifier::Shift),
         | "alt" | "option" => Some(Modifier::Alt),
         | "meta" | "cmd" | "win" | "super" => Some(Modifier::Meta),
         | _ => None,
      }
   }
}

impl fmt::Display for Modifier
{
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.name()) }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError
{
   #[error("unknown key '{0}'")]
   UnknownKey(String),
   #[error("empty token in chord '{0}'")]
   EmptyToken(String),
   #[error("chord '{0}' has no non-modifier key")]
   NoKey(String),
   #[error("chord '{chord}' has more than one non-modifier key ('{first}' and '{second}')")]
   MultipleKeys
   {
      chord: String, first: String, second: String
   },
}

/// A key plus the modifiers held while it is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord
{
   /// Deduplicated, in canonical order (ctrl, shift, alt, meta).
   pub modifiers: Vec<Modifier>,
   pub key:       KeyName,
}

impl FromStr for KeyChord
{
   type Err = ParseError;

   fn from_str(text: &str) -> Result<Self, ParseError>
   {
      let mut modifiers: Vec<Modifier> = Vec::new();
      let mut key: Option<(KeyName, String)> = None;
      for token in text.split('+')
      {
         let token = token.trim();
         if token.is_empty()
         {
            return Err(ParseError::EmptyToken(text.trim().to_string()));
         }
         if let Some(modifier) = Modifier::lookup(&token.to_ascii_lowercase())
         {
            if !modifiers.contains(&modifier)
            {
               modifiers.push(modifier);
            }
            continue;
         }
         let parsed: KeyName = token.parse()?;
         if let Some((_, first)) = &key
         {
            return Err(ParseError::MultipleKeys { chord:  text.trim().to_string(),
                                                  first:  first.clone(),
                                                  second: token.to_string(), });
         }
         key = Some((parsed, token.to_string()));
      }
      let Some((key, _)) = key
      else
      {
         return Err(ParseError::NoKey(text.trim().to_string()));
      };
      modifiers.sort();
      Ok(KeyChord { modifiers, key })
   }
}

// ---------------------------------------------------------------------------
// Bindings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode
{
   /// Key down on button press, key up on button release.
   #[default]
   Hold,
   /// Full press+release on button press; the release edge is ignored.
   Tap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding
{
   pub chord: KeyChord,
   pub mode:  Mode,
}

impl<'de> Deserialize<'de> for Binding
{
   fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
   {
      struct BindingVisitor;

      impl<'de> de::Visitor<'de> for BindingVisitor
      {
         type Value = Binding;

         fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
         {
            f.write_str("a key chord string such as \"ctrl+x\" or a table { key = \"x\", mode = \"hold\" | \"tap\" }")
         }

         fn visit_str<E: de::Error>(self, text: &str) -> Result<Binding, E>
         {
            Ok(Binding { chord: text.parse().map_err(E::custom)?,
                         mode:  Mode::Hold, })
         }

         fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Binding, A::Error>
         {
            let mut chord: Option<KeyChord> = None;
            let mut mode: Option<Mode> = None;
            while let Some(field) = map.next_key::<String>()?
            {
               match field.as_str()
               {
                  | "key" =>
                  {
                     if chord.is_some()
                     {
                        return Err(de::Error::duplicate_field("key"));
                     }
                     let text: String = map.next_value()?;
                     chord = Some(text.parse().map_err(de::Error::custom)?);
                  }
                  | "mode" =>
                  {
                     if mode.is_some()
                     {
                        return Err(de::Error::duplicate_field("mode"));
                     }
                     mode = Some(map.next_value()?);
                  }
                  | other => return Err(de::Error::unknown_field(other, &["key", "mode"])),
               }
            }
            let chord = chord.ok_or_else(|| de::Error::missing_field("key"))?;
            Ok(Binding { chord,
                         mode: mode.unwrap_or_default() })
         }
      }

      deserializer.deserialize_any(BindingVisitor)
   }
}

/// One low-level key transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyLike
{
   Key(KeyName),
   Modifier(Modifier),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step
{
   pub key:  KeyLike,
   pub down: bool,
}

impl Binding
{
   /// The ordered key transitions to inject for `edge`. Pure so it can be
   /// tested without a device; backends only translate the steps.
   pub fn steps(&self, edge: Edge) -> Vec<Step>
   {
      let press = || {
         let modifiers = self.chord
                             .modifiers
                             .iter()
                             .map(|m| Step { key:  KeyLike::Modifier(*m),
                                             down: true, });
         modifiers.chain(std::iter::once(Step { key:  KeyLike::Key(self.chord.key),
                                                down: true, }))
      };
      let release = || {
         let modifiers = self.chord
                             .modifiers
                             .iter()
                             .rev()
                             .map(|m| Step { key:  KeyLike::Modifier(*m),
                                             down: false, });
         std::iter::once(Step { key:  KeyLike::Key(self.chord.key),
                                down: false, }).chain(modifiers)
      };
      match (self.mode, edge)
      {
         | (Mode::Hold, Edge::Pressed) => press().collect(),
         | (Mode::Hold, Edge::Released) => release().collect(),
         | (Mode::Tap, Edge::Pressed) => press().chain(release()).collect(),
         | (Mode::Tap, Edge::Released) => Vec::new(),
      }
   }
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile
{
   pub app:      String,
   #[serde(default)]
   pub bindings: HashMap<InGameAction, Binding>,
   /// Where the profile came from (for logs and `--doctor`).
   #[serde(skip)]
   pub source:   String,
}

/// Keyboard profiles: `<config dir>/keyboard/<name>.toml`, else embedded.
pub const CATALOG: Catalog = Catalog { kind:     "keyboard profile",
                                       subdir:   "keyboard",
                                       embedded: &[("tpv", include_str!("../../../keyboard/tpv.toml")),
                                                   ("mywhoosh", include_str!("../../../keyboard/mywhoosh.toml")),
                                                   ("biketerra", include_str!("../../../keyboard/biketerra.toml")),
                                                   ("rouvy", include_str!("../../../keyboard/rouvy.toml"))], };

impl Profile
{
   pub fn parse(text: &str) -> Result<Profile> { toml::from_str(text).map_err(Into::into) }

   pub fn supports(&self, action: InGameAction) -> bool { self.bindings.contains_key(&action) }

   /// Load profile `arg` (a name or a file path): user override first, then
   /// the embedded default.
   pub fn load(arg: &str) -> Result<Profile>
   {
      let (text, source) = CATALOG.resolve(arg)?;
      let mut profile = Profile::parse(&text).with_context(|| format!("parsing keyboard profile '{arg}' ({source})"))?;
      profile.source = source;
      info!(profile = arg, app = %profile.app, source = %profile.source, bindings = profile.bindings.len(), "keyboard profile loaded");
      if profile.bindings.is_empty()
      {
         let edit = if is_path(arg) { Some(PathBuf::from(arg)) } else { CATALOG.user_path(arg) };
         let edit = edit.map_or("<config dir>/keyboard".to_string(), |p| p.display().to_string());
         warn!("keyboard profile '{arg}' has no bindings; edit {edit}");
      }
      Ok(profile)
   }
}

#[cfg(test)] mod tests;
