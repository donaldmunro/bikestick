//! Zwift Play controller support: scan loop + one connection task per side.

pub mod connection;
pub mod constants;
pub mod decoder;
pub mod state;

use std::{collections::HashMap,
          sync::{Arc, Mutex}};

use anyhow::{Result, anyhow};
use bluest::{Adapter, Device, ManufacturerData};
use futures::StreamExt;
use tokio::{sync::mpsc,
            time::{Duration, Instant, sleep, timeout}};
use tracing::{debug, info, warn};
use connection::SideCommand;
use constants::{RC1_LEFT_SIDE, RC1_RIGHT_SIDE, ZWIFT_MANUFACTURER_ID};

use crate::{config::ZwiftPlayConfig,
            events::{ControllerEvent, Source}};

/// Shared registry of connected sides, used to route vibrate feedback.
#[derive(Clone, Default)]
pub struct SideRegistry
{
   inner: Arc<Mutex<HashMap<Source, mpsc::UnboundedSender<SideCommand>>>>,
}

impl SideRegistry
{
   fn register(&self, source: Source) -> mpsc::UnboundedReceiver<SideCommand>
   {
      let (tx, rx) = mpsc::unbounded_channel();
      self.inner.lock().unwrap().insert(source, tx);
      rx
   }

   fn unregister(&self, source: Source) { self.inner.lock().unwrap().remove(&source); }

   fn is_connected(&self, source: Source) -> bool { self.inner.lock().unwrap().contains_key(&source) }

   pub fn vibrate(&self, source: Source)
   {
      if let Some(tx) = self.inner.lock().unwrap().get(&source)
      {
         let _ = tx.send(SideCommand::Vibrate);
      }
   }

   pub fn both_connected(&self) -> bool
   {
      let map = self.inner.lock().unwrap();
      map.contains_key(&Source::ZwiftPlayLeft) && map.contains_key(&Source::ZwiftPlayRight)
   }
}

/// Identify a discovered device as a Zwift Play side from manufacturer data.
/// bluest splits the advertisement into `company_id` and `data`; `data` holds
/// the payload after the company id, so its first byte is the side marker.
fn identify(manufacturer_data: &ManufacturerData) -> Option<Source>
{
   if manufacturer_data.company_id != ZWIFT_MANUFACTURER_ID
   {
      return None;
   }
   match manufacturer_data.data.first().copied()?
   {
      | RC1_LEFT_SIDE => Some(Source::ZwiftPlayLeft),
      | RC1_RIGHT_SIDE => Some(Source::ZwiftPlayRight),
      | _ => None,
   }
}

/// Does the opaque device-ID pin allow `id`? An empty pin accepts anything;
/// otherwise compare trimmed and case-insensitively.
fn pin_allows(pin: &str, id: &str) -> bool
{
   let pin = pin.trim();
   pin.is_empty() || pin.eq_ignore_ascii_case(id.trim())
}

/// Scan for Zwift Play controllers forever, maintaining one connection per
/// side with reconnect-on-drop. Discovery pauses while both sides are up.
pub async fn run(config: ZwiftPlayConfig, events: mpsc::Sender<ControllerEvent>, registry: SideRegistry) -> Result<()>
{
   let adapter = Adapter::default().await
                                   .ok_or_else(|| anyhow!("no Bluetooth adapter available for Zwift Play"))?;
   info!("waiting for Bluetooth adapter...");
   adapter.wait_available().await?;

   loop
   {
      if registry.both_connected()
      {
         sleep(Duration::from_secs(3)).await;
         continue;
      }

      info!("scanning for Zwift Play controllers...");
      // Empty service filter: the Play may not advertise the Zwift service.
      let mut discovery = match adapter.scan(&[]).await
      {
         | Ok(stream) => stream,
         | Err(error) =>
         {
            warn!(%error, "discovery failed; retrying in 5s");
            sleep(Duration::from_secs(5)).await;
            continue;
         }
      };

      while let Some(advertisement) = discovery.next().await
      {
         let Some(manufacturer_data) = &advertisement.adv_data.manufacturer_data
         else
         {
            continue;
         };
         let Some(source) = identify(manufacturer_data)
         else
         {
            continue;
         };
         let device = advertisement.device;
         let id = device.id().to_string();
         let pin = match source
         {
            | Source::ZwiftPlayLeft => &config.left_address,
            | Source::ZwiftPlayRight => &config.right_address,
            | Source::Gamepad(_) => unreachable!(),
         };
         if !pin_allows(pin, &id)
         {
            debug!(%source, %id, "ignoring non-pinned device");
            continue;
         }
         if registry.is_connected(source)
         {
            continue;
         }

         info!(%source, %id, "found Zwift Play controller");
         info!("pin with zwift_play.left_address/right_address = \"{id}\" to restrict to this controller");
         let commands = registry.register(source);
         tokio::spawn(side_task(adapter.clone(), device, source, config.clone(), events.clone(), registry.clone(), commands));

         if registry.both_connected()
         {
            break; // stop discovery: adapter hygiene for the peripheral role
         }
      }
      drop(discovery);
      sleep(Duration::from_secs(1)).await;
   }
}

/// Time allowed for a (re)connect before the device is considered gone.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Consecutive short-lived failed sessions before the slot is released.
const MAX_FAILED_SESSIONS: u32 = 5;
/// A session that lasted at least this long counts as having worked.
const HEALTHY_SESSION: Duration = Duration::from_secs(10);

/// Own one side: connect, run the session, back off, and release the slot on
/// exit so the scan loop can find the device again.
///
/// bluest has no portable RSSI/"still visible" query, so "device gone" is
/// inferred from failure instead: a reconnect that errors or times out, or
/// too many consecutive short-lived sessions, releases the slot.
async fn side_task(adapter: Adapter, device: Device, source: Source, config: ZwiftPlayConfig, events: mpsc::Sender<ControllerEvent>,
                   registry: SideRegistry, mut commands: mpsc::UnboundedReceiver<SideCommand>)
{
   let mut backoff = Duration::from_secs(1);
   let mut failures = 0u32;
   loop
   {
      if !device.is_connected().await
      {
         info!(%source, "connecting...");
         match timeout(CONNECT_TIMEOUT, adapter.connect_device(&device)).await
         {
            | Ok(Ok(())) =>
            {}
            | Ok(Err(error)) =>
            {
               warn!(%source, %error, "connect failed");
               break;
            }
            | Err(_) =>
            {
               warn!(%source, "connect timed out");
               break;
            }
         }
      }

      let started = Instant::now();
      match connection::run_session(&adapter, &device, source, &config, events.clone(), &mut commands).await
      {
         | Ok(()) => break,
         | Err(error) =>
         {
            warn!(%source, error = %format!("{error:#}"), "session ended");
            let _ = adapter.disconnect_device(&device).await;
            failures = if started.elapsed() >= HEALTHY_SESSION { 1 } else { failures + 1 };
            if failures >= MAX_FAILED_SESSIONS
            {
               warn!(%source, "too many consecutive failures; giving up this attempt");
               break;
            }
            sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(5));
         }
      }
   }
   registry.unregister(source);
   info!(%source, "released; will reconnect via scan");
}

#[cfg(test)]
mod tests
{
   use super::*;

   fn md(company_id: u16, data: &[u8]) -> ManufacturerData
   {
      ManufacturerData { company_id,
                         data: data.to_vec() }
   }

   #[test]
   fn identify_sides()
   {
      assert_eq!(identify(&md(ZWIFT_MANUFACTURER_ID, &[RC1_LEFT_SIDE, 0x01])), Some(Source::ZwiftPlayLeft));
      assert_eq!(identify(&md(ZWIFT_MANUFACTURER_ID, &[RC1_RIGHT_SIDE])), Some(Source::ZwiftPlayRight));
   }

   #[test]
   fn identify_rejects()
   {
      assert_eq!(identify(&md(0x004C, &[RC1_LEFT_SIDE])), None);
      assert_eq!(identify(&md(ZWIFT_MANUFACTURER_ID, &[0x7F])), None);
      assert_eq!(identify(&md(ZWIFT_MANUFACTURER_ID, &[])), None);
   }

   #[test]
   fn pin_matching()
   {
      assert!(pin_allows("", "AA:BB:CC:DD:EE:FF"));
      assert!(pin_allows("  ", "AA:BB:CC:DD:EE:FF"));
      assert!(pin_allows("aa:bb:cc:dd:ee:ff", "AA:BB:CC:DD:EE:FF"));
      assert!(pin_allows(" AA:BB:CC:DD:EE:FF ", "aa:bb:cc:dd:ee:ff"));
      assert!(!pin_allows("AA:BB:CC:DD:EE:00", "AA:BB:CC:DD:EE:FF"));
   }
}
