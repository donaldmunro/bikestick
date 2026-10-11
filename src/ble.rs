//! BlueZ adapter setup for the OBC peripheral role (Linux only).

use anyhow::{Context, Result};
use bluer::{Adapter, Session};
use tracing::info;

pub async fn adapter(name: &str) -> Result<(Session, Adapter)>
{
   let session = Session::new().await.context("connecting to bluetoothd")?;
   let adapter = if name.is_empty()
   {
      session.default_adapter()
             .await
             .context("no default Bluetooth adapter")?
   }
   else
   {
      session.adapter(name)
             .with_context(|| format!("adapter {name}"))?
   };
   adapter.set_powered(true)
          .await
          .context("powering adapter")?;
   info!(
       adapter = %adapter.name(),
       address = %adapter.address().await?,
       "Bluetooth adapter ready"
   );
   Ok((session, adapter))
}
