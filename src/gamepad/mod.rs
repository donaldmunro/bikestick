//! Gamepad input via gilrs, bridged from a blocking poll thread to the
//! tokio event bus. Covers Xbox/PlayStation and anything else gilrs maps.

use gilrs::{Axis, Button, Event, EventType, Gilrs};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{config::GamepadConfig,
            events::{ControllerEvent, Edge, Source, SourceButton}};

fn map_button(button: Button) -> Option<SourceButton>
{
   Some(match button
   {
      | Button::South => SourceButton::South,
      | Button::East => SourceButton::East,
      | Button::North => SourceButton::North,
      | Button::West => SourceButton::West,
      | Button::LeftTrigger => SourceButton::LeftBumper,
      | Button::RightTrigger => SourceButton::RightBumper,
      | Button::LeftTrigger2 => SourceButton::LeftTrigger2,
      | Button::RightTrigger2 => SourceButton::RightTrigger2,
      | Button::DPadUp => SourceButton::DPadUp,
      | Button::DPadDown => SourceButton::DPadDown,
      | Button::DPadLeft => SourceButton::DPadLeft,
      | Button::DPadRight => SourceButton::DPadRight,
      | Button::Start => SourceButton::Start,
      | Button::Select => SourceButton::Select,
      | _ => return None,
   })
}

/// Left-stick X steering state with hysteresis.
#[derive(Default, Clone, Copy, PartialEq)]
enum SteerState
{
   #[default]
   Center,
   Left,
   Right,
}

/// Spawn the blocking gilrs thread. Returns immediately; events flow into `tx`.
pub fn spawn(config: GamepadConfig, tx: mpsc::Sender<ControllerEvent>)
{
   std::thread::Builder::new().name("gilrs".into())
                              .spawn(move || run(config, tx))
                              .expect("spawning gilrs thread");
}

fn run(config: GamepadConfig, tx: mpsc::Sender<ControllerEvent>)
{
   let mut gilrs = match Gilrs::new()
   {
      | Ok(gilrs) => gilrs,
      | Err(error) =>
      {
         warn!(%error, "gamepad support unavailable");
         return;
      }
   };
   for (id, gamepad) in gilrs.gamepads()
   {
      info!(id = %id, name = gamepad.name(), "gamepad present");
   }

   let mut steer = SteerState::Center;

   loop
   {
      while let Some(Event { id, event, .. }) = gilrs.next_event()
      {
         let source = Source::Gamepad(usize::from(id));
         let send = |button: SourceButton, edge: Edge| {
            let event = ControllerEvent { source, button, edge };
            if tx.blocking_send(event).is_err()
            {
               warn!("event bus closed; stopping gamepad thread");
            }
         };
         match event
         {
            | EventType::ButtonPressed(button, _) =>
            {
               if let Some(mapped) = map_button(button)
               {
                  send(mapped, Edge::Pressed);
               }
            }
            | EventType::ButtonReleased(button, _) =>
            {
               if let Some(mapped) = map_button(button)
               {
                  send(mapped, Edge::Released);
               }
            }
            | EventType::AxisChanged(Axis::LeftStickX, value, _) =>
            {
               let next = if value <= -config.stick_engage
               {
                  SteerState::Left
               }
               else if value >= config.stick_engage
               {
                  SteerState::Right
               }
               else if value.abs() < config.stick_release
               {
                  SteerState::Center
               }
               else
               {
                  steer // inside hysteresis band: hold state
               };
               if next != steer
               {
                  match steer
                  {
                     | SteerState::Left => send(SourceButton::StickLeft, Edge::Released),
                     | SteerState::Right => send(SourceButton::StickRight, Edge::Released),
                     | SteerState::Center =>
                     {}
                  }
                  match next
                  {
                     | SteerState::Left => send(SourceButton::StickLeft, Edge::Pressed),
                     | SteerState::Right => send(SourceButton::StickRight, Edge::Pressed),
                     | SteerState::Center =>
                     {}
                  }
                  steer = next;
               }
            }
            | EventType::Connected => info!(id = %id, "gamepad connected"),
            | EventType::Disconnected => info!(id = %id, "gamepad disconnected"),
            | _ =>
            {}
         }
      }
      std::thread::sleep(std::time::Duration::from_millis(4));
   }
}
