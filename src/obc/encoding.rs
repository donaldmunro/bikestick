//! Mapping from in-game actions to OpenBikeControl button codes.

use std::collections::HashMap;

use crate::{config::ObcConfig, events::InGameAction, obc::constants::STATE_PRESSED};

/// OBC button id plus the state byte sent while pressed (release is always 0x00).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObcCode
{
   pub id:    u8,
   pub value: u8,
}

impl ObcCode
{
   const fn pressed(id: u8) -> Self { Self { id, value: STATE_PRESSED } }
}

/// Built-in encoding; `None` means the action has no OBC representation.
fn default_code(action: InGameAction) -> Option<ObcCode>
{
   use InGameAction as IA;
   let id = match action
   {
      | IA::ShiftUp => 0x01,
      | IA::ShiftDown => 0x02,
      | IA::NavigateUp => 0x10,
      | IA::NavigateDown => 0x11,
      | IA::NavigateLeft => 0x12,
      | IA::NavigateRight => 0x13,
      | IA::Select => 0x14,
      | IA::Back => 0x15,
      | IA::Menu => 0x16,
      | IA::SteerLeft => 0x18,
      | IA::SteerRight => 0x19,
      | IA::Brake => 0x1A,
      | IA::PushToTalk => 0x21,
      | IA::IncreaseResistance => 0x30,
      | IA::DecreaseResistance => 0x31,
      | IA::SkipInterval => 0x32,
      | IA::Pause => 0x33,
      | IA::Resume => 0x34,
      | IA::Lap => 0x35,
      | IA::PreviousInterval => 0x36,
      | IA::UTurn => 0x37,
      | IA::ChangeMode => 0x38,
      | IA::TakeABreak => 0x39,
      | IA::JoinRider => 0x3A,
      | IA::ChangeRoute => 0x3B,
      | IA::CameraView => 0x40,
      | IA::CameraViewBehind => 0x41,
      | IA::CameraViewFirstPerson => 0x43,
      | IA::CameraViewFromFront => return None,
      | IA::HudToggle => 0x44,
      | IA::MapToggle => 0x45,
      | IA::Screenshot => 0x24,
      | IA::CruiseControl => 0x3C, // 0x01 = toggle at current power
      // OBC 0x17 means "return to home screen", not "focus own rider".
      | IA::Home => return None,
      // No standard OBC ids; TPV only declares Camera 1-3 (0x41-0x43).
      // Override per view in [obc.camera_views].
      | IA::CameraViewHelicopter | IA::CameraViewBirdseye => return None,
      // TPV-specific camera focus; no OBC equivalent.
      | IA::FocusGroupAhead | IA::FocusGroupBehind => return None,
   };
   Some(ObcCode::pressed(id))
}

/// Resolves actions to OBC codes: built-in defaults plus config overrides.
/// Shared by both OBC transports (BLE peripheral and DirCon).
#[derive(Debug, Clone, Default)]
pub struct ObcEncoding
{
   overrides: HashMap<InGameAction, ObcCode>,
}

impl ObcEncoding
{
   pub fn from_config(config: &ObcConfig) -> anyhow::Result<Self>
   {
      config.camera_views.validate()?;
      let views = &config.camera_views;
      let mut overrides = HashMap::new();
      for (action, entry) in [(InGameAction::CameraViewBehind, views.camera_view_behind),
                              (InGameAction::CameraViewFirstPerson, views.camera_view_first_person),
                              (InGameAction::CameraViewFromFront, views.camera_view_from_front),
                              (InGameAction::CameraViewHelicopter, views.camera_view_helicopter),
                              (InGameAction::CameraViewBirdseye, views.camera_view_birdseye)]
      {
         if let Some(entry) = entry
         {
            overrides.insert(action, ObcCode { id:    entry.id,
                                               value: entry.value, });
         }
      }
      Ok(Self { overrides })
   }

   pub fn resolve(&self, action: InGameAction) -> Option<ObcCode>
   {
      self.overrides
          .get(&action)
          .copied()
          .or_else(|| default_code(action))
   }
}

#[cfg(test)]
mod tests
{
   use super::*;
   use crate::config::{CameraViewsConfig, ObcCodeConfig};

   #[test]
   fn defaults()
   {
      let encoding = ObcEncoding::default();
      assert_eq!(encoding.resolve(InGameAction::ShiftUp), Some(ObcCode { id: 0x01, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraView), Some(ObcCode { id: 0x40, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::Brake), Some(ObcCode { id: 0x1A, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewBehind), Some(ObcCode { id: 0x41, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewFirstPerson), Some(ObcCode { id: 0x43, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewFromFront), None);
      assert_eq!(encoding.resolve(InGameAction::Screenshot), Some(ObcCode { id: 0x24, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CruiseControl), Some(ObcCode { id: 0x3C, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::Home), None);
      assert_eq!(encoding.resolve(InGameAction::CameraViewHelicopter), None);
      assert_eq!(encoding.resolve(InGameAction::FocusGroupAhead), None);
   }

   #[test]
   fn config_overrides_defaults()
   {
      let config = ObcConfig { camera_views: CameraViewsConfig { camera_view_behind: Some(ObcCodeConfig { id: 0x40, value: 5 }),
                                                                 camera_view_first_person: None,
                                                                 camera_view_from_front: Some(ObcCodeConfig { id: 0x42, value: 1 }),
                                                                 camera_view_birdseye: Some(ObcCodeConfig { id: 0x40, value: 9 }),
                                                                 ..CameraViewsConfig::default() },
                               ..ObcConfig::default() };
      let encoding = ObcEncoding::from_config(&config).unwrap();
      assert_eq!(encoding.resolve(InGameAction::CameraViewBehind), Some(ObcCode { id: 0x40, value: 5 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewFirstPerson), Some(ObcCode { id: 0x43, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewFromFront), Some(ObcCode { id: 0x42, value: 1 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewBirdseye), Some(ObcCode { id: 0x40, value: 9 }));
      assert_eq!(encoding.resolve(InGameAction::CameraViewHelicopter), None);
   }

   #[test]
   fn invalid_config_is_rejected()
   {
      let bad = |id, value| ObcConfig { camera_views: CameraViewsConfig { camera_view_behind: Some(ObcCodeConfig { id, value }),
                                                                          ..CameraViewsConfig::default() },
                                        ..ObcConfig::default() };
      assert!(ObcEncoding::from_config(&bad(0x30, 1)).is_err());
      assert!(ObcEncoding::from_config(&bad(0x50, 1)).is_err());
      assert!(ObcEncoding::from_config(&bad(0x40, 0x20)).is_err());
      assert!(ObcEncoding::from_config(&bad(0x40, 0)).is_err());
      assert!(ObcEncoding::from_config(&bad(0x4F, 1)).is_ok());
   }
}
