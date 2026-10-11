//! Placeholder OBC BLE peripheral for platforms without a backend yet.

use anyhow::Result;
use tracing::warn;

use crate::config::ObcConfig;

/// Cloneable handle used by the output backend.
#[derive(Clone)]
pub struct ObcHandle;

impl ObcHandle
{
   pub async fn is_ready_for(&self, _button_id: u8) -> bool { false }

   pub async fn has_subscriber(&self) -> bool { false }

   pub async fn send_button(&self, _button_id: u8, _state_byte: u8) -> Result<()>
   {
      anyhow::bail!("OpenBikeControl BLE peripheral is not supported on this platform yet");
   }
}

pub struct ObcPeripheral
{
   pub handle: ObcHandle,
}

impl ObcPeripheral
{
   pub async fn start(adapter_name: &str, _config: ObcConfig) -> Result<Self>
   {
      if !adapter_name.is_empty()
      {
         warn!("general.adapter is Linux-only; ignored");
      }
      anyhow::bail!("OpenBikeControl BLE peripheral is not supported on this platform yet")
   }
}
