//! Normalized event types flowing through the daemon.
//!
//! Input sources (Zwift Play sides, gamepads) emit `ControllerEvent`s on a
//! shared mpsc channel; the `Mapper` translates them to `InGameAction`s which
//! an output backend delivers to TrainingPeaks Virtual.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Which physical input produced an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source
{
   ZwiftPlayLeft,
   ZwiftPlayRight,
   Gamepad(usize),
}

impl fmt::Display for Source
{
   fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
   {
      match self
      {
         | Source::ZwiftPlayLeft => write!(f, "zwift-play-left"),
         | Source::ZwiftPlayRight => write!(f, "zwift-play-right"),
         | Source::Gamepad(id) => write!(f, "gamepad-{id}"),
      }
   }
}

/// A source-level button, before mapping to an in-game action.
/// Play names map controller's Y/Z/A/B positions onto the navigation d-pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceButton
{
   // Zwift Play right controller
   A,
   B,
   Y,
   Z,
   SideButtonRight,
   OnOffRight,
   PaddleRight,
   // Zwift Play left controller
   NavUp,
   NavDown,
   NavLeft,
   NavRight,
   SideButtonLeft,
   OnOffLeft,
   PaddleLeft,
   // Gamepad (gilrs unified names)
   South,
   East,
   North,
   West,
   LeftBumper,
   RightBumper,
   LeftTrigger2,
   RightTrigger2,
   DPadUp,
   DPadDown,
   DPadLeft,
   DPadRight,
   Start,
   Select,
   StickLeft,
   StickRight,
}

impl SourceButton
{
   /// Every button, Zwift Play first.
   pub const ALL: &'static [SourceButton] = &[SourceButton::A,
                                              SourceButton::B,
                                              SourceButton::Y,
                                              SourceButton::Z,
                                              SourceButton::SideButtonRight,
                                              SourceButton::OnOffRight,
                                              SourceButton::PaddleRight,
                                              SourceButton::NavUp,
                                              SourceButton::NavDown,
                                              SourceButton::NavLeft,
                                              SourceButton::NavRight,
                                              SourceButton::SideButtonLeft,
                                              SourceButton::OnOffLeft,
                                              SourceButton::PaddleLeft,
                                              SourceButton::South,
                                              SourceButton::East,
                                              SourceButton::North,
                                              SourceButton::West,
                                              SourceButton::LeftBumper,
                                              SourceButton::RightBumper,
                                              SourceButton::LeftTrigger2,
                                              SourceButton::RightTrigger2,
                                              SourceButton::DPadUp,
                                              SourceButton::DPadDown,
                                              SourceButton::DPadLeft,
                                              SourceButton::DPadRight,
                                              SourceButton::Start,
                                              SourceButton::Select,
                                              SourceButton::StickLeft,
                                              SourceButton::StickRight];

   /// The snake_case name used in mapping files and config.
   pub fn name(self) -> &'static str
   {
      match self
      {
         | SourceButton::A => "a",
         | SourceButton::B => "b",
         | SourceButton::Y => "y",
         | SourceButton::Z => "z",
         | SourceButton::SideButtonRight => "side_button_right",
         | SourceButton::OnOffRight => "on_off_right",
         | SourceButton::PaddleRight => "paddle_right",
         | SourceButton::NavUp => "nav_up",
         | SourceButton::NavDown => "nav_down",
         | SourceButton::NavLeft => "nav_left",
         | SourceButton::NavRight => "nav_right",
         | SourceButton::SideButtonLeft => "side_button_left",
         | SourceButton::OnOffLeft => "on_off_left",
         | SourceButton::PaddleLeft => "paddle_left",
         | SourceButton::South => "south",
         | SourceButton::East => "east",
         | SourceButton::North => "north",
         | SourceButton::West => "west",
         | SourceButton::LeftBumper => "left_bumper",
         | SourceButton::RightBumper => "right_bumper",
         | SourceButton::LeftTrigger2 => "left_trigger2",
         | SourceButton::RightTrigger2 => "right_trigger2",
         | SourceButton::DPadUp => "dpad_up",
         | SourceButton::DPadDown => "dpad_down",
         | SourceButton::DPadLeft => "dpad_left",
         | SourceButton::DPadRight => "dpad_right",
         | SourceButton::Start => "start",
         | SourceButton::Select => "select",
         | SourceButton::StickLeft => "stick_left",
         | SourceButton::StickRight => "stick_right",
      }
   }

   /// True for Zwift Play buttons, false for gamepad buttons.
   pub fn is_play(self) -> bool
   {
      match self
      {
         | SourceButton::A
         | SourceButton::B
         | SourceButton::Y
         | SourceButton::Z
         | SourceButton::SideButtonRight
         | SourceButton::OnOffRight
         | SourceButton::PaddleRight
         | SourceButton::NavUp
         | SourceButton::NavDown
         | SourceButton::NavLeft
         | SourceButton::NavRight
         | SourceButton::SideButtonLeft
         | SourceButton::OnOffLeft
         | SourceButton::PaddleLeft => true,
         | SourceButton::South
         | SourceButton::East
         | SourceButton::North
         | SourceButton::West
         | SourceButton::LeftBumper
         | SourceButton::RightBumper
         | SourceButton::LeftTrigger2
         | SourceButton::RightTrigger2
         | SourceButton::DPadUp
         | SourceButton::DPadDown
         | SourceButton::DPadLeft
         | SourceButton::DPadRight
         | SourceButton::Start
         | SourceButton::Select
         | SourceButton::StickLeft
         | SourceButton::StickRight => false,
      }
   }
}

impl FromStr for SourceButton
{
   type Err = String;

   fn from_str(text: &str) -> Result<Self, String>
   {
      // left_trigger/right_trigger are historical aliases of the bumpers.
      match text
      {
         | "left_trigger" => return Ok(SourceButton::LeftBumper),
         | "right_trigger" => return Ok(SourceButton::RightBumper),
         | _ => (),
      }
      SourceButton::ALL.iter()
                       .find(|button| button.name() == text)
                       .copied()
                       .ok_or_else(|| format!("unknown button '{text}'"))
   }
}

impl<'de> Deserialize<'de> for SourceButton
{
   fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
   {
      let text = String::deserialize(deserializer)?;
      text.parse().map_err(de::Error::custom)
   }
}

/// Press/release edge, or an analog level for OBC's 2..=255 encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge
{
   Pressed,
   Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerEvent
{
   pub source: Source,
   pub button: SourceButton,
   pub edge:   Edge,
}

/// Abstract in-game action; the intersection of what app supports over
/// OpenBikeControl (28 buttons) plus steering (keyboard-only in TPV).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InGameAction
{
   ShiftUp,
   ShiftDown,
   SteerLeft,
   SteerRight,
   NavigateUp,
   NavigateDown,
   NavigateLeft,
   NavigateRight,
   Brake,
   CameraView,            // next camera view
   CameraViewBehind, // TPV Key 1 (default view) see https://help.trainingpeaks.com/hc/en-us/articles/31340399556877-TrainingPeaks-Virtual-Controls-and-Keyboard-Shortcuts
   CameraViewFirstPerson, // TPV Key 3 (1st person `Doom/Quake` view)
   CameraViewFromFront, // TPV Key 6 (looking back - called `Front view`` in TPV docs)
   CameraViewHelicopter, // TPV Key 8 (Helicopter view)
   CameraViewBirdseye, // TPV Key 9 (Birdseye view)
   Select,
   Back,
   Menu,
   PushToTalk,
   IncreaseResistance,
   DecreaseResistance,
   SkipInterval,
   PreviousInterval,
   Pause,
   Resume,
   Lap,
   UTurn,
   ChangeMode,
   TakeABreak,
   JoinRider,
   ChangeRoute,
   HudToggle,
   MapToggle,
   Home,             // focus the camera on your own rider (TPV `Home` key); not OBC 0x17 "home screen"
   Screenshot,       // OBC 0x24
   CruiseControl,    // OBC 0x3C toggle (TPV)
   FocusGroupAhead,  // TPV Shift+Page Up: camera to the group ahead (events only)
   FocusGroupBehind, // TPV Shift+Page Down: camera to the group behind (events only)
}

impl InGameAction
{
   /// Every action, in declaration order.
   pub const ALL: &'static [InGameAction] = &[InGameAction::ShiftUp,
                                              InGameAction::ShiftDown,
                                              InGameAction::SteerLeft,
                                              InGameAction::SteerRight,
                                              InGameAction::NavigateUp,
                                              InGameAction::NavigateDown,
                                              InGameAction::NavigateLeft,
                                              InGameAction::NavigateRight,
                                              InGameAction::Brake,
                                              InGameAction::CameraView,
                                              InGameAction::CameraViewBehind,
                                              InGameAction::CameraViewFirstPerson,
                                              InGameAction::CameraViewFromFront,
                                              InGameAction::CameraViewHelicopter,
                                              InGameAction::CameraViewBirdseye,
                                              InGameAction::Select,
                                              InGameAction::Back,
                                              InGameAction::Menu,
                                              InGameAction::PushToTalk,
                                              InGameAction::IncreaseResistance,
                                              InGameAction::DecreaseResistance,
                                              InGameAction::SkipInterval,
                                              InGameAction::PreviousInterval,
                                              InGameAction::Pause,
                                              InGameAction::Resume,
                                              InGameAction::Lap,
                                              InGameAction::UTurn,
                                              InGameAction::ChangeMode,
                                              InGameAction::TakeABreak,
                                              InGameAction::JoinRider,
                                              InGameAction::ChangeRoute,
                                              InGameAction::HudToggle,
                                              InGameAction::MapToggle,
                                              InGameAction::Home,
                                              InGameAction::Screenshot,
                                              InGameAction::CruiseControl,
                                              InGameAction::FocusGroupAhead,
                                              InGameAction::FocusGroupBehind];

   /// The snake_case name used in config, mapping and keyboard profile files.
   pub fn name(self) -> &'static str
   {
      match self
      {
         | InGameAction::ShiftUp => "shift_up",
         | InGameAction::ShiftDown => "shift_down",
         | InGameAction::SteerLeft => "steer_left",
         | InGameAction::SteerRight => "steer_right",
         | InGameAction::NavigateUp => "navigate_up",
         | InGameAction::NavigateDown => "navigate_down",
         | InGameAction::NavigateLeft => "navigate_left",
         | InGameAction::NavigateRight => "navigate_right",
         | InGameAction::Brake => "brake",
         | InGameAction::CameraView => "camera_view",
         | InGameAction::CameraViewBehind => "camera_view_behind",
         | InGameAction::CameraViewFirstPerson => "camera_view_first_person",
         | InGameAction::CameraViewFromFront => "camera_view_from_front",
         | InGameAction::CameraViewHelicopter => "camera_view_helicopter",
         | InGameAction::CameraViewBirdseye => "camera_view_birdseye",
         | InGameAction::Select => "select",
         | InGameAction::Back => "back",
         | InGameAction::Menu => "menu",
         | InGameAction::PushToTalk => "push_to_talk",
         | InGameAction::IncreaseResistance => "increase_resistance",
         | InGameAction::DecreaseResistance => "decrease_resistance",
         | InGameAction::SkipInterval => "skip_interval",
         | InGameAction::PreviousInterval => "previous_interval",
         | InGameAction::Pause => "pause",
         | InGameAction::Resume => "resume",
         | InGameAction::Lap => "lap",
         | InGameAction::UTurn => "u_turn",
         | InGameAction::ChangeMode => "change_mode",
         | InGameAction::TakeABreak => "take_a_break",
         | InGameAction::JoinRider => "join_rider",
         | InGameAction::ChangeRoute => "change_route",
         | InGameAction::HudToggle => "hud_toggle",
         | InGameAction::MapToggle => "map_toggle",
         | InGameAction::Home => "home",
         | InGameAction::Screenshot => "screenshot",
         | InGameAction::CruiseControl => "cruise_control",
         | InGameAction::FocusGroupAhead => "focus_group_ahead",
         | InGameAction::FocusGroupBehind => "focus_group_behind",
      }
   }
}

impl FromStr for InGameAction
{
   type Err = String;

   /// Accepts the snake_case name (`focus_group_ahead`) and, forgivingly,
   /// any case/underscore variant of it (`FocusGroupAhead`, `Screenshot`).
   fn from_str(text: &str) -> Result<Self, String>
   {
      fn fold(text: &str) -> String
      {
         text.chars()
             .filter(|c| *c != '_' && *c != '-')
             .flat_map(char::to_lowercase)
             .collect()
      }
      let wanted = fold(text.trim());
      InGameAction::ALL.iter()
                       .find(|action| fold(action.name()) == wanted)
                       .copied()
                       .ok_or_else(|| format!("unknown action '{text}'"))
   }
}

impl Serialize for InGameAction
{
   fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serializer.serialize_str(self.name()) }
}

impl<'de> serde::Deserialize<'de> for InGameAction
{
   fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
   {
      let text = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
      text.parse().map_err(serde::de::Error::custom)
   }
}

#[cfg(test)]
mod tests
{
   use super::*;

   #[test]
   fn action_names_match_serde()
   {
      assert_eq!(InGameAction::ALL.len(), 38);
      assert_eq!("FocusGroupAhead".parse::<InGameAction>(), Ok(InGameAction::FocusGroupAhead));
      assert_eq!("Screenshot".parse::<InGameAction>(), Ok(InGameAction::Screenshot));
      assert_eq!("camera-view-behind".parse::<InGameAction>(), Ok(InGameAction::CameraViewBehind));
      assert!("ScreenShotz".parse::<InGameAction>().is_err());
      for action in InGameAction::ALL
      {
         let parsed: InGameAction = serde::Deserialize::deserialize(de::value::StrDeserializer::<de::value::Error>::new(action.name())).unwrap();
         assert_eq!(parsed, *action);
         assert_eq!(action.name().parse::<InGameAction>(), Ok(*action));
      }
   }

   #[test]
   fn button_names_round_trip()
   {
      assert_eq!(SourceButton::ALL.len(), 30);
      for button in SourceButton::ALL
      {
         assert_eq!(button.name().parse::<SourceButton>(), Ok(*button));
      }
      assert_eq!("left_trigger".parse::<SourceButton>(), Ok(SourceButton::LeftBumper));
      assert_eq!("right_trigger".parse::<SourceButton>(), Ok(SourceButton::RightBumper));
      assert!("bogus".parse::<SourceButton>().is_err());
      assert!(SourceButton::PaddleLeft.is_play());
      assert!(!SourceButton::Start.is_play());
   }
}
