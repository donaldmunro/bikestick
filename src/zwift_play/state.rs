//! Pure edge-extraction from successive `PlayKeyPadStatus` frames.
//!
//! The device re-sends identical frames continuously, and proto2 defaults make
//! absent button fields read as "pressed", so the first frame only seeds the
//! tracker and emits nothing.

use crate::events::{ControllerEvent, Edge, Source, SourceButton};
use super::decoder::PlayKeyPadStatus;

#[derive(Debug, Default)]
pub struct EdgeTracker
{
   previous:       Option<PlayKeyPadStatus>,
   /// Whether the paddle is currently considered "pulled".
   paddle_engaged: bool,
}

impl EdgeTracker
{
   /// Diff `next` against the previous frame and emit press/release events.
   pub fn advance(&mut self, next: PlayKeyPadStatus, source: Source, paddle_threshold: i32) -> Vec<ControllerEvent>
   {
      let mut events = Vec::new();
      let Some(prev) = self.previous.replace(next)
      else
      {
         // Seed frame: initialize paddle state, emit nothing.
         self.paddle_engaged = next.analog_lr.abs() >= paddle_threshold;
         return events;
      };
      if prev == next
      {
         return events;
      }

      let right = next.right_pad;
      let mut push = |pressed_prev: bool, pressed_next: bool, button: SourceButton| {
         if pressed_prev != pressed_next
         {
            events.push(ControllerEvent { source,
                                          button,
                                          edge: if pressed_next { Edge::Pressed } else { Edge::Released } });
         }
      };

      // The same four face positions mean ABYZ on the right controller and a
      // navigation d-pad on the left one
      if right
      {
         push(prev.y_up, next.y_up, SourceButton::Y);
         push(prev.z_left, next.z_left, SourceButton::Z);
         push(prev.a_right, next.a_right, SourceButton::A);
         push(prev.b_down, next.b_down, SourceButton::B);
         push(prev.shift, next.shift, SourceButton::SideButtonRight);
         push(prev.on, next.on, SourceButton::OnOffRight);
      }
      else
      {
         push(prev.y_up, next.y_up, SourceButton::NavUp);
         push(prev.z_left, next.z_left, SourceButton::NavLeft);
         push(prev.a_right, next.a_right, SourceButton::NavRight);
         push(prev.b_down, next.b_down, SourceButton::NavDown);
         push(prev.shift, next.shift, SourceButton::SideButtonLeft);
         push(prev.on, next.on, SourceButton::OnOffLeft);
      }

      let engaged = next.analog_lr.abs() >= paddle_threshold;
      if engaged != self.paddle_engaged
      {
         self.paddle_engaged = engaged;
         events.push(ControllerEvent { source,
                                       button: if right { SourceButton::PaddleRight } else { SourceButton::PaddleLeft },
                                       edge: if engaged { Edge::Pressed } else { Edge::Released } });
      }

      events
   }
}

#[cfg(test)]
mod tests
{
   use super::*;

   fn idle(right: bool) -> PlayKeyPadStatus
   {
      PlayKeyPadStatus { right_pad: right,
                         y_up:      false,
                         z_left:    false,
                         a_right:   false,
                         b_down:    false,
                         shift:     false,
                         on:        false,
                         analog_lr: 0,
                         analog_ud: 0, }
   }

   #[test]
   fn first_frame_seeds_without_events()
   {
      let mut tracker = EdgeTracker::default();
      // A hostile first frame (all defaults = pressed) must emit nothing.
      let events = tracker.advance(PlayKeyPadStatus::default(), Source::ZwiftPlayRight, 100);
      assert!(events.is_empty());
   }

   #[test]
   fn emits_press_and_release_once()
   {
      let mut tracker = EdgeTracker::default();
      tracker.advance(idle(true), Source::ZwiftPlayRight, 100);

      let mut pressed = idle(true);
      pressed.a_right = true;
      let events = tracker.advance(pressed, Source::ZwiftPlayRight, 100);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].button, SourceButton::A);
      assert_eq!(events[0].edge, Edge::Pressed);

      // Re-sent identical frame: no events.
      assert!(tracker.advance(pressed, Source::ZwiftPlayRight, 100)
                     .is_empty());

      let events = tracker.advance(idle(true), Source::ZwiftPlayRight, 100);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].edge, Edge::Released);
   }

   #[test]
   fn paddle_threshold_engages_and_releases()
   {
      let mut tracker = EdgeTracker::default();
      tracker.advance(idle(false), Source::ZwiftPlayLeft, 100);

      let mut pulled = idle(false);
      pulled.analog_lr = -100;
      let events = tracker.advance(pulled, Source::ZwiftPlayLeft, 100);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].button, SourceButton::PaddleLeft);
      assert_eq!(events[0].edge, Edge::Pressed);

      // Partial pull below threshold releases.
      let mut partial = idle(false);
      partial.analog_lr = -40;
      let events = tracker.advance(partial, Source::ZwiftPlayLeft, 100);
      assert_eq!(events.len(), 1);
      assert_eq!(events[0].edge, Edge::Released);
   }

   #[test]
   fn left_side_maps_to_navigation()
   {
      let mut tracker = EdgeTracker::default();
      tracker.advance(idle(false), Source::ZwiftPlayLeft, 100);
      let mut pressed = idle(false);
      pressed.y_up = true;
      let events = tracker.advance(pressed, Source::ZwiftPlayLeft, 100);
      assert_eq!(events[0].button, SourceButton::NavUp);
   }
}
