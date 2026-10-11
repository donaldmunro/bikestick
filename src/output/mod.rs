//! Output routing: deliver in-game actions to TPV via OBC over DirCon
//! (TCP/mDNS), OBC over BLE, or a virtual keyboard.

pub mod keyboard;

use tracing::{debug, warn};

use crate::{config::OutputMode,
            dircon::server::DirconHandle,
            events::{Edge, InGameAction},
            obc::{constants::STATE_RELEASED, encoding::ObcEncoding, peripheral::ObcHandle}};

pub trait KeyboardOutput
{
   fn supports(&self, action: InGameAction) -> bool;
   fn send(&mut self, action: InGameAction, edge: Edge) -> Result<(), anyhow::Error>;
}

pub struct OutputRouter
{
   mode:     OutputMode,
   encoding: ObcEncoding,
   dircon:   Option<DirconHandle>,
   obc:      Option<ObcHandle>,
   keyboard: Option<Box<dyn KeyboardOutput>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered
{
   Dircon,
   Obc,
   Keyboard,
   Dropped,
}

impl OutputRouter
{
   pub fn new(mode: OutputMode, encoding: ObcEncoding, dircon: Option<DirconHandle>, obc: Option<ObcHandle>,
              keyboard: Option<Box<dyn KeyboardOutput>>)
              -> Self
   {
      Self { mode,
             encoding,
             dircon,
             obc,
             keyboard }
   }

   /// Send a raw OBC `[id, state]` over whichever OBC transport has a
   /// subscribed client, ignoring the client's declared button list and the
   /// action encoding. Used by `--simulate-obc` to probe what an app accepts.
   pub async fn send_raw_obc(&mut self, button_id: u8, state: u8) -> Delivered
   {
      if matches!(self.mode, OutputMode::Dircon | OutputMode::Auto)
      {
         if let Some(handle) = &self.dircon
         {
            if handle.has_subscriber()
            {
               match handle.send_button_unchecked(button_id, state)
               {
                  | Ok(()) => return Delivered::Dircon,
                  | Err(error) => warn!(button_id, %error, "raw DirCon delivery failed"),
               }
            }
         }
      }
      if matches!(self.mode, OutputMode::Obc | OutputMode::Auto)
      {
         if let Some(handle) = &self.obc
         {
            if handle.has_subscriber().await
            {
               match handle.send_button(button_id, state).await
               {
                  | Ok(()) => return Delivered::Obc,
                  | Err(error) => warn!(button_id, %error, "raw OBC BLE delivery failed"),
               }
            }
         }
      }
      Delivered::Dropped
   }

   /// Deliver one action edge. In `auto` mode the preference order is
   /// DirCon (a connected network client means TPV chose that path), then
   /// OBC over BLE, then keyboard - evaluated per event so delivery keeps
   /// working if a transport drops mid-ride.
   pub async fn send(&mut self, action: InGameAction, edge: Edge) -> Delivered
   {
      // None = no OBC encoding for this action: DirCon/OBC are not attempted.
      let code = self.encoding.resolve(action);
      let state = match edge
      {
         | Edge::Pressed => code.map_or(0, |code| code.value),
         | Edge::Released => STATE_RELEASED,
      };

      let try_dircon = matches!(self.mode, OutputMode::Dircon | OutputMode::Auto);
      let try_obc = matches!(self.mode, OutputMode::Obc | OutputMode::Auto);
      let try_keyboard = matches!(self.mode, OutputMode::Keyboard | OutputMode::Auto);

      if code.is_none() && matches!(self.mode, OutputMode::Dircon | OutputMode::Obc)
      {
         debug!(?action, "no OBC encoding; dropped");
         return Delivered::Dropped;
      }

      if let Some(code) = code.filter(|_| try_dircon)
      {
         let button_id = code.id;
         if let Some(handle) = &self.dircon
         {
            if handle.is_ready_for(button_id)
            {
               match handle.send_button(button_id, state)
               {
                  | Ok(()) =>
                  {
                     debug!(?action, ?edge, "delivered via DirCon");
                     return Delivered::Dircon;
                  }
                  | Err(error) => warn!(?action, %error, "DirCon delivery failed"),
               }
            }
         }
         if self.mode == OutputMode::Dircon
         {
            warn!(?action, "no ready DirCon client");
            return Delivered::Dropped;
         }
      }

      if let Some(code) = code.filter(|_| try_obc)
      {
         let button_id = code.id;
         if let Some(handle) = &self.obc
         {
            if handle.is_ready_for(button_id).await
            {
               match handle.send_button(button_id, state).await
               {
                  | Ok(()) =>
                  {
                     debug!(?action, ?edge, "delivered via OBC BLE");
                     return Delivered::Obc;
                  }
                  | Err(error) => warn!(?action, %error, "OBC BLE delivery failed"),
               }
            }
         }
         if self.mode == OutputMode::Obc
         {
            warn!(?action, "no ready OBC BLE central");
            return Delivered::Dropped;
         }
      }

      if try_keyboard
      {
         if let Some(keyboard) = &mut self.keyboard
         {
            if (*keyboard).supports(action)
            {
               match keyboard.send(action, edge)
               {
                  | Ok(()) =>
                  {
                     debug!(?action, ?edge, "delivered via keyboard");
                     return Delivered::Keyboard;
                  }
                  | Err(error) => warn!(?action, %error, "keyboard delivery failed"),
               }
            }
            else
            {
               debug!(?action, "no keyboard mapping; dropped");
            }
         }
      }
      Delivered::Dropped
   }
}
