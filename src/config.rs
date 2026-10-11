//! TOML configuration.

use std::{collections::BTreeMap,
          path::{Path, PathBuf}};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::mapping::ActionSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode
{
   /// Prefer whichever OBC transport app is connected on (DirCon first, then
   /// BLE); fall back to the keyboard.
   Auto,
   /// OBC over BLE only.
   Obc,
   /// OBC over DirCon (TCP + mDNS) only.
   Dircon,
   Keyboard,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig
{
   pub output:    OutputMode,
   pub log_level: String,
   /// Adapter name like "hci0"; empty = system default. Linux-only, and used only by the OBC peripheral
   /// (Zwift Play scanning via bluest always uses the system default adapter).
   pub adapter:   String,
}

impl Default for GeneralConfig
{
   fn default() -> Self
   {
      Self { output:    OutputMode::Auto,
             log_level: "info".into(),
             adapter:   String::new(), }
   }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ZwiftPlayConfig
{
   pub enabled:                bool,
   /// Optional opaque device-ID pins; empty = accept any. The ID is the
   /// platform's BLE device identifier as logged when a controller is found:
   /// a MAC address ("AA:BB:CC:DD:EE:FF") on Linux/Windows, a UUID on macOS.
   /// Compared case-insensitively.
   pub left_address:           String,
   pub right_address:          String,
   /// |Analog_LR| >= threshold counts as a full paddle pull (a shift).
   pub paddle_shift_threshold: i32,
   /// Vibrate the controller when a paddle press delivers any action (the key
   /// name is historical: it is not limited to shifts).
   pub vibrate_on_shift:       bool,
   /// Escape hatch for the protobuf field 6/7 (Shift vs On) ambiguity between
   /// old and current upstream codebases.
   pub swap_shift_on_fields:   bool,
}

impl Default for ZwiftPlayConfig
{
   fn default() -> Self
   {
      Self { enabled:                true,
             left_address:           String::new(),
             right_address:          String::new(),
             paddle_shift_threshold: 100,
             vibrate_on_shift:       true,
             swap_shift_on_fields:   false, }
   }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObcConfig
{
   pub enabled:         bool,
   /// Advertised BLE local name. "BikeControl" is proven to be discovered by app.
   pub local_name:      String,
   pub battery_percent: u8,
   /// Encoding of the TPV camera-view actions; applies to both OBC transports
   /// (BLE peripheral and DirCon).
   pub camera_views:    CameraViewsConfig,
}

/// One OBC button encoding: the button `id` plus the state byte sent when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObcCodeConfig
{
   pub id:    u8,
   /// State byte sent on press (release is always 0x00). Default 1.
   #[serde(default = "default_obc_value")]
   pub value: u8,
}

fn default_obc_value() -> u8 { 1 }

/// `[obc.camera_views]`: override how camera-view actions are encoded.
/// Omitting a key keeps the built-in default (behind = 0x41, first person =
/// 0x43, from front = none, i.e. keyboard only).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CameraViewsConfig
{
   #[serde(skip_serializing_if = "Option::is_none")]
   pub camera_view_behind:       Option<ObcCodeConfig>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub camera_view_first_person: Option<ObcCodeConfig>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub camera_view_from_front:   Option<ObcCodeConfig>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub camera_view_helicopter:   Option<ObcCodeConfig>,
   #[serde(skip_serializing_if = "Option::is_none")]
   pub camera_view_birdseye:     Option<ObcCodeConfig>,
}

impl CameraViewsConfig
{
   pub fn validate(&self) -> anyhow::Result<()>
   {
      for (name, code) in [("camera_view_behind", self.camera_view_behind),
                           ("camera_view_first_person", self.camera_view_first_person),
                           ("camera_view_from_front", self.camera_view_from_front),
                           ("camera_view_helicopter", self.camera_view_helicopter),
                           ("camera_view_birdseye", self.camera_view_birdseye)]
      {
         let Some(code) = code
         else
         {
            continue;
         };
         if !(0x40..=0x4F).contains(&code.id)
         {
            anyhow::bail!("obc.camera_views.{name}.id must be 0x40..=0x4F (got {:#04X})", code.id);
         }
         if code.id == 0x40 && !(0x01..=0x1F).contains(&code.value)
         {
            anyhow::bail!("obc.camera_views.{name}.value must be 0x01..=0x1F for id 0x40 (got {:#04X})", code.value);
         }
         if code.value == 0
         {
            anyhow::bail!("obc.camera_views.{name}.value must not be 0 (0 means released)");
         }
      }
      Ok(())
   }
}

impl Default for ObcConfig
{
   fn default() -> Self
   {
      Self { enabled:         true,
             local_name:      "BikeControl".into(),
             battery_percent: 100,
             camera_views:    CameraViewsConfig::default(), }
   }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DirconConfig
{
   pub enabled:       bool,
   /// Preferred TCP port; the server walks to an ephemeral port if taken and
   /// the actual port is what gets advertised via mDNS.
   pub port:          u16,
   /// mDNS instance name.
   pub instance_name: String,
}

impl Default for DirconConfig
{
   fn default() -> Self
   {
      Self { enabled:       true,
             port:          36867,
             instance_name: "BikeControl".into(), }
   }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyboardConfig
{
   pub enabled: bool,
   /// Keyboard profile name (user file `keyboard/<name>.toml` in the config
   /// directory, else the embedded default) or a path to a profile file.
   pub profile: String,
}

impl Default for KeyboardConfig
{
   fn default() -> Self
   {
      Self { enabled: true,
             profile: "tpv".into(), }
   }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct GamepadConfig
{
   pub enabled:       bool,
   /// Stick deflection that engages steering (0.0..=1.0).
   pub stick_engage:  f32,
   /// Stick deflection that releases steering (hysteresis; < stick_engage).
   pub stick_release: f32,
}

impl GamepadConfig
{
   pub fn defaults() -> Self
   {
      Self { enabled:       true,
             stick_engage:  0.6,
             stick_release: 0.4, }
   }
}

/// `[mapping]`: which mapping to use plus per-button overrides.
///
/// `name` selects the base mapping (an embedded preset, a user file in
/// `mappings/`, or a file path). Each `[mapping.zwift_play]` / `[mapping.gamepad]`
/// entry (`button = "action" | ["action", ...] | "none"`) then replaces just
/// that button's binding.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MappingConfig
{
   pub name:       String,
   #[serde(skip_serializing_if = "BTreeMap::is_empty")]
   pub zwift_play: BTreeMap<String, ActionSpec>,
   #[serde(skip_serializing_if = "BTreeMap::is_empty")]
   pub gamepad:    BTreeMap<String, ActionSpec>,
}

impl Default for MappingConfig
{
   fn default() -> Self
   {
      Self { name:       "ride".into(),
             zwift_play: BTreeMap::new(),
             gamepad:    BTreeMap::new(), }
   }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config
{
   pub general:    GeneralConfig,
   pub zwift_play: ZwiftPlayConfig,
   pub obc:        ObcConfig,
   pub dircon:     DirconConfig,
   pub keyboard:   KeyboardConfig,
   pub gamepad:    GamepadConfig,
   pub mapping:    MappingConfig,
}

impl Default for Config
{
   fn default() -> Self
   {
      Self { general:    GeneralConfig::default(),
             zwift_play: ZwiftPlayConfig::default(),
             obc:        ObcConfig::default(),
             dircon:     DirconConfig::default(),
             keyboard:   KeyboardConfig::default(),
             gamepad:    GamepadConfig::defaults(),
             mapping:    MappingConfig::default(), }
   }
}

/// The documented default `config.toml` (written by `--write-config`).
pub const TEMPLATE: &str = include_str!("../config/config.toml");

impl Config
{
   pub fn default_path() -> Option<PathBuf> { crate::platform::config_dir().map(|d| d.join("config.toml")) }

   /// Write the embedded template to `path`, creating parent directories.
   /// Refuses to replace an existing file unless `force`.
   pub fn write_template(path: &Path, force: bool) -> anyhow::Result<()>
   {
      if path.exists() && !force
      {
         anyhow::bail!("{} already exists; pass --force to overwrite", path.display());
      }
      if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty())
      {
         std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
      }
      std::fs::write(path, TEMPLATE).with_context(|| format!("writing {}", path.display()))
   }

   /// The effective configuration as TOML (for `--print-config`).
   pub fn to_toml(&self) -> anyhow::Result<String> { toml::to_string_pretty(self).context("serializing config") }

   /// Load from `path` if given, else the default path; missing file = defaults.
   pub fn load(path: Option<&Path>) -> anyhow::Result<Self>
   {
      let path = match path
      {
         | Some(p) => p.to_path_buf(),
         | None => match Self::default_path()
         {
            | Some(p) if p.exists() => p,
            | _ => return Ok(Self::default()),
         },
      };
      let text = std::fs::read_to_string(&path).with_context(|| format!("reading config {}", path.display()))?;
      let config: Config = toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
      config.validate()?;
      Ok(config)
   }

   pub fn validate(&self) -> anyhow::Result<()>
   {
      if self.obc.battery_percent > 100
      {
         anyhow::bail!("obc.battery_percent must be 0..=100");
      }
      self.obc.camera_views.validate()?;
      if self.zwift_play.paddle_shift_threshold <= 0 || self.zwift_play.paddle_shift_threshold > 100
      {
         anyhow::bail!("zwift_play.paddle_shift_threshold must be 1..=100");
      }
      if self.gamepad.stick_release >= self.gamepad.stick_engage
      {
         anyhow::bail!("gamepad.stick_release must be < gamepad.stick_engage");
      }
      Ok(())
   }
}

#[cfg(test)]
mod tests
{
   use super::*;
   use crate::events::InGameAction;

   #[test]
   fn default_config_is_valid() { Config::default().validate().unwrap(); }

   #[test]
   fn parses_full_config()
   {
      let text = r#"
[general]
output = "keyboard"
log_level = "debug"
adapter = "hci0"

[zwift_play]
enabled = true
left_address = "AA:BB:CC:DD:EE:FF"
paddle_shift_threshold = 90
vibrate_on_shift = false
swap_shift_on_fields = true

[keyboard]
profile = "mywhoosh"

[obc]
local_name = "MyBridge"
battery_percent = 88

[mapping]
name = "workout"

[mapping.zwift_play]
paddle_right = "shift_up"
paddle_left = ["shift_down", "decrease_resistance"]
a = "select"
b = "none"

[mapping.gamepad]
start = "pause"
"#;
      let config: Config = toml::from_str(text).unwrap();
      config.validate().unwrap();
      assert_eq!(config.general.output, OutputMode::Keyboard);
      assert_eq!(config.obc.local_name, "MyBridge");
      assert_eq!(config.keyboard.profile, "mywhoosh");
      assert!(config.keyboard.enabled);
      assert_eq!(config.mapping.name, "workout");
      assert_eq!(config.mapping.zwift_play.get("paddle_right"), Some(&ActionSpec(vec![InGameAction::ShiftUp])));
      assert_eq!(config.mapping.zwift_play.get("paddle_left"),
                 Some(&ActionSpec(vec![InGameAction::ShiftDown, InGameAction::DecreaseResistance])));
      assert_eq!(config.mapping.zwift_play.get("b"), Some(&ActionSpec(Vec::new())));
      assert_eq!(config.mapping.gamepad.get("start"), Some(&ActionSpec(vec![InGameAction::Pause])));
      assert!(config.zwift_play.swap_shift_on_fields);
   }

   #[test]
   fn mapping_name_defaults_to_ride()
   {
      assert_eq!(Config::default().mapping.name, "ride");
   }

   #[test]
   fn keyboard_profile_defaults_to_tpv()
   {
      assert_eq!(Config::default().keyboard.profile, "tpv");
   }

   #[test]
   fn parses_camera_views()
   {
      let text = "[obc.camera_views]\ncamera_view_behind = { id = 0x42 }\ncamera_view_from_front = { id = 0x40, value = 0x05 }\n";
      let config: Config = toml::from_str(text).unwrap();
      config.validate().unwrap();
      let views = &config.obc.camera_views;
      assert_eq!(views.camera_view_behind, Some(ObcCodeConfig { id: 0x42, value: 1 }));
      assert_eq!(views.camera_view_first_person, None);
      assert_eq!(views.camera_view_from_front, Some(ObcCodeConfig { id: 0x40, value: 5 }));
   }

   #[test]
   fn rejects_bad_camera_views()
   {
      for text in ["[obc.camera_views]\ncamera_view_behind = { id = 0x10 }\n",
                   "[obc.camera_views]\ncamera_view_behind = { id = 0x50 }\n",
                   "[obc.camera_views]\ncamera_view_behind = { id = 0x40, value = 0x20 }\n",
                   "[obc.camera_views]\ncamera_view_behind = { id = 0x40, value = 0 }\n",
                   "[obc.camera_views]\ncamera_view_behind = { id = 0x41, value = 0 }\n"]
      {
         let config: Config = toml::from_str(text).unwrap();
         assert!(config.validate().is_err(), "{text}");
      }
      assert!(toml::from_str::<Config>("[obc.camera_views]\nbogus = { id = 0x41 }\n").is_err());
      assert!(toml::from_str::<Config>("[obc.camera_views]\ncamera_view_behind = { value = 1 }\n").is_err());
   }

   fn temp_path(tag: &str) -> PathBuf
   {
      std::env::temp_dir().join(format!("bikestick-config-test-{}-{tag}", std::process::id()))
                          .join("sub")
                          .join("config.toml")
   }

   #[test]
   fn template_equals_defaults()
   {
      let config: Config = toml::from_str(TEMPLATE).unwrap();
      config.validate().unwrap();
      assert_eq!(config, Config::default());
   }

   #[test]
   fn default_config_round_trips()
   {
      let config = Config::default();
      let text = config.to_toml().unwrap();
      let parsed: Config = toml::from_str(&text).unwrap();
      assert_eq!(parsed, config);
      assert_eq!(parsed.to_toml().unwrap(), text);
   }

   #[test]
   fn customized_config_round_trips()
   {
      let mut config = Config::default();
      config.general.output = OutputMode::Dircon;
      config.obc.camera_views.camera_view_behind = Some(ObcCodeConfig { id: 0x42, value: 1 });
      config.obc.camera_views.camera_view_from_front = Some(ObcCodeConfig { id: 0x40, value: 5 });
      config.mapping.name = "race".into();
      config.mapping
            .zwift_play
            .insert("paddle_right".into(), ActionSpec(vec![InGameAction::ShiftUp]));
      config.mapping
            .zwift_play
            .insert("paddle_left".into(), ActionSpec(vec![InGameAction::ShiftDown, InGameAction::DecreaseResistance]));
      config.mapping
            .zwift_play
            .insert("b".into(), ActionSpec(Vec::new()));
      config.mapping
            .gamepad
            .insert("start".into(), ActionSpec(vec![InGameAction::Pause]));
      config.validate().unwrap();
      let text = config.to_toml().unwrap();
      let parsed: Config = toml::from_str(&text).unwrap();
      assert_eq!(parsed, config);
      assert!(text.contains("\"none\""), "{text}");
   }

   #[test]
   fn write_template_refuses_overwrite_without_force()
   {
      let path = temp_path("write");
      Config::write_template(&path, false).unwrap();
      assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);

      std::fs::write(&path, "edited\n").unwrap();
      let error = Config::write_template(&path, false).unwrap_err()
                                                      .to_string();
      assert!(error.contains("--force"), "{error}");
      assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited\n");

      Config::write_template(&path, true).unwrap();
      assert_eq!(std::fs::read_to_string(&path).unwrap(), TEMPLATE);
      let _ = std::fs::remove_dir_all(path.parent().and_then(Path::parent).unwrap());
   }

   #[test]
   fn rejects_unknown_keys()
   {
      let text = "[general]\nbogus = 1\n";
      assert!(toml::from_str::<Config>(text).is_err());
   }
}
