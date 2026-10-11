use anyhow::{Context, Result, bail};
use bluest::{Adapter, Characteristic, ConnectionEvent, Device};
use futures::StreamExt;
use tokio::{sync::mpsc,
            time::{Duration, timeout}};
use tracing::{debug, info, warn};

use crate::{config::ZwiftPlayConfig,
            events::{ControllerEvent, Source}};
use super::{constants::*, decoder::PlayKeyPadStatus, state::EdgeTracker};

pub enum SideCommand
{
   Vibrate,
}

async fn find_characteristic(device: &Device, service_uuid: uuid::Uuid, uuid: uuid::Uuid) -> Result<Characteristic>
{
   let service = device.discover_services_with_uuid(service_uuid)
                       .await?
                       .into_iter()
                       .next()
                       .with_context(|| format!("service {service_uuid} not found"))?;
   service.discover_characteristics_with_uuid(uuid)
          .await?
          .into_iter()
          .next()
          .with_context(|| format!("characteristic {uuid} not found"))
}

/// Run a connected session until the device disconnects or errors.
///
/// The caller has already established the BLE connection; this performs
/// subscribe -> handshake -> event loop.
pub async fn run_session(adapter: &Adapter, device: &Device, source: Source, config: &ZwiftPlayConfig, events: mpsc::Sender<ControllerEvent>,
                         commands: &mut mpsc::UnboundedReceiver<SideCommand>)
                         -> Result<()>
{
   let async_char = find_characteristic(device, ZWIFT_CUSTOM_SERVICE, ZWIFT_ASYNC_CHARACTERISTIC).await
                                                                                                 .context("Zwift async characteristic")?;
   let sync_rx = find_characteristic(device, ZWIFT_CUSTOM_SERVICE, ZWIFT_SYNC_RX_CHARACTERISTIC).await
                                                                                                .context("Zwift syncRx characteristic")?;
   let sync_tx = find_characteristic(device, ZWIFT_CUSTOM_SERVICE, ZWIFT_SYNC_TX_CHARACTERISTIC).await
                                                                                                .context("Zwift syncTx characteristic")?;

   // Subscribe BEFORE writing the handshake so the response isn't missed.
   // bluest's notify() covers both notifications and indications. The streams
   // are Unpin, so StreamExt::next works directly.
   let mut async_stream = async_char.notify().await.context("subscribe async char")?;
   let mut sync_tx_stream = sync_tx.notify().await.context("subscribe syncTx char")?;

   // Extra disconnect signal for backends whose notify streams don't end on
   // disconnect. If unavailable, fall back to a never-ready stream.
   let mut connection_events = match adapter.device_connection_events(device).await
   {
      | Ok(stream) => stream.boxed(),
      | Err(error) =>
      {
         debug!(%source, %error, "connection events unavailable");
         futures::stream::pending().boxed()
      }
   };

   sync_rx.write_without_response(&RIDE_ON)
          .await
          .context("write RideOn handshake")?;

   // Await the RideOn acknowledgement on the indication channel. Real
   // hardware in plaintext mode echoes a bare "RideOn" (no 01 04 suffix, no
   // public key), so accept any RideOn-prefixed reply and proceed
   // even if none arrives.
   match timeout(Duration::from_secs(5), sync_tx_stream.next()).await
   {
      | Ok(Some(Err(error))) => bail!("syncTx stream error during handshake: {error}"),
      | Ok(Some(Ok(ack))) if ack.starts_with(&RIDE_ON) =>
      {
         info!(%source, tail = ?&ack[RIDE_ON.len()..], "handshake complete (RideOn acknowledged)");
      }
      | Ok(Some(Ok(other))) =>
      {
         warn!(%source, response = ?other, "unexpected handshake response; continuing anyway");
      }
      | Ok(None) => bail!("syncTx stream closed during handshake"),
      | Err(_) =>
      {
         warn!(%source, "no handshake response within 5s; continuing anyway");
      }
   }

   let mut tracker = EdgeTracker::default();
   let mut battery: Option<u8> = None;

   loop
   {
      tokio::select! {
          frame = async_stream.next() => {
              let Some(frame) = frame else {
                  bail!("notification stream ended (device disconnected)");
              };
              let frame = frame.context("notification stream error")?;
              match frame.first().copied() {
                  Some(MSG_PLAY_KEYPAD) => {
                      match PlayKeyPadStatus::decode(&frame[1..], config.swap_shift_on_fields) {
                          Ok(status) => {
                              debug!(%source, ?status, "keypad frame");
                              for event in tracker.advance(status, source, config.paddle_shift_threshold) {
                                  info!(%source, button = ?event.button, edge = ?event.edge, "input");
                                  if events.send(event).await.is_err() {
                                      bail!("event bus closed");
                                  }
                              }
                          }
                          Err(error) => warn!(%source, %error, raw = ?frame, "keypad decode failed"),
                      }
                  }
                  Some(MSG_IDLE) => {} // periodic keepalive from the device
                  Some(MSG_BATTERY) => {
                      let level = frame.get(2).copied();
                      if level != battery {
                          battery = level;
                          if let Some(level) = level {
                              info!(%source, level, "battery");
                          }
                      }
                  }
                  Some(MSG_DISCONNECT) => {
                      bail!("device sent disconnect (another host took over?)");
                  }
                  Some(other) => debug!(%source, msg_type = format!("{other:#04X}"), raw = ?frame, "unknown message"),
                  None => {}
              }
          }
          indication = sync_tx_stream.next() => {
              // Post-handshake syncTx traffic: log-only.
              match indication {
                  Some(Ok(_)) => debug!(%source, "syncTx indication"),
                  Some(Err(error)) => bail!("syncTx stream error: {error}"),
                  None => bail!("syncTx stream ended (device disconnected)"),
              }
          }
          event = connection_events.next() => {
              match event {
                  Some(ConnectionEvent::Disconnected) => bail!("device disconnected"),
                  Some(ConnectionEvent::Connected) => {}
                  None => connection_events = futures::stream::pending().boxed(),
              }
          }
          command = commands.recv() => {
              match command {
                  Some(SideCommand::Vibrate) => {
                      if let Err(error) = sync_rx.write_without_response(&VIBRATE_COMMAND).await {
                          warn!(%source, %error, "vibrate write failed");
                      }
                  }
                  None => bail!("command channel closed"),
              }
          }
      }
   }
}
