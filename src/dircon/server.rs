//! DirCon TCP server exposing the OpenBikeControl GATT service to trainer
//! apps that speak Wahoo Direct Connect over the network.
//!
//! The GATT model tunneled here is exactly the BLE one: service `d273f680-…`
//! with Button State (notify) and App Info (write). Request/response
//! semantics (UUID-echo acks, no error frames) follow the
//! bt-bridge server (https://github.com/donaldmunro/bt-bridge),
//! which was described from a real Wahoo network capture.

use std::{collections::HashMap,
          sync::{Arc, Mutex,
                 atomic::{AtomicU64, Ordering}}};

use anyhow::{Context, Result};
use bytes::{BufMut, Bytes, BytesMut};
use tokio::{io::{AsyncReadExt, AsyncWriteExt},
            net::{TcpListener, TcpStream},
            sync::broadcast};
use tracing::{debug, info, warn};

use crate::obc::{constants::{APP_INFO_CHARACTERISTIC, BUTTON_STATE_CHARACTERISTIC, OBC_SERVICE},
                 protocol::{AppInfo, AppInfoReassembler, encode_button_state}};
use super::frame::{DirconFrame, MsgType, char_flags, uuid_from_bytes, uuid_to_bytes};

/// The 28 button ids TPV declares over BLE. Used as a
/// fallback when a DirCon client subscribes without writing App Info first.
const TPV_DEFAULT_BUTTONS: [u8; 28] = [0x01, 0x02, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x21, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
                                       0x38, 0x39, 0x3A, 0x3B, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45];

#[derive(Default)]
struct ClientReadiness
{
   subscribed: bool,
   app_info:   Option<AppInfo>,
}

impl ClientReadiness
{
   fn supports(&self, button_id: u8) -> bool
   {
      match &self.app_info
      {
         | Some(info) => info.supported_buttons.contains(&button_id),
         // Subscribed but no App Info written (yet): assume the TPV set so
         // shifting works from the first paddle pull.
         | None => TPV_DEFAULT_BUTTONS.contains(&button_id),
      }
   }
}

/// One button notification fanned out to clients. `force` skips the
/// per-client "declared support" check (used by `--simulate-obc`).
#[derive(Clone, Copy, Debug)]
struct ButtonEvent
{
   button_id:  u8,
   state_byte: u8,
   force:      bool,
}

type Registry = Arc<Mutex<HashMap<u64, ClientReadiness>>>;

/// Cloneable handle used by the output router.
#[derive(Clone)]
pub struct DirconHandle
{
   registry: Registry,
   events:   broadcast::Sender<ButtonEvent>,
}

impl DirconHandle
{
   /// True when any connected client is subscribed and supports the button.
   pub fn is_ready_for(&self, button_id: u8) -> bool
   {
      self.registry
          .lock()
          .unwrap()
          .values()
          .any(|client| client.subscribed && client.supports(button_id))
   }

   /// Fan a `[0x01, id, state]` value out to all ready clients.
   pub fn send_button(&self, button_id: u8, state_byte: u8) -> Result<()>
   {
      if !self.is_ready_for(button_id)
      {
         anyhow::bail!("no subscribed DirCon client supports button {button_id:#04X}");
      }
      self.events
          .send(ButtonEvent { button_id,
                              state_byte,
                              force: false })
          .context("no DirCon client listening")?;
      Ok(())
   }

   /// True when any connected client is subscribed, regardless of declared buttons.
   pub fn has_subscriber(&self) -> bool
   {
      self.registry
          .lock()
          .unwrap()
          .values()
          .any(|client| client.subscribed)
   }

   /// Like `send_button` but ignores the clients' declared button lists; a
   /// subscribed client is still required. For probing what an app accepts.
   pub fn send_button_unchecked(&self, button_id: u8, state_byte: u8) -> Result<()>
   {
      if !self.has_subscriber()
      {
         anyhow::bail!("no subscribed DirCon client");
      }
      self.events
          .send(ButtonEvent { button_id,
                              state_byte,
                              force: true })
          .context("no DirCon client listening")?;
      Ok(())
   }
}

pub struct DirconServer
{
   pub handle:     DirconHandle,
   pub bound_port: u16,
}

impl DirconServer
{
   /// Bind the TCP listener (walking to an ephemeral port if the preferred
   /// one is taken the real port is advertised via mDNS) and start serving.
   pub async fn start(preferred_port: u16) -> Result<Self>
   {
      let listener = match TcpListener::bind(("0.0.0.0", preferred_port)).await
      {
         | Ok(listener) => listener,
         | Err(error) =>
         {
            warn!(%error, preferred_port, "preferred DirCon port taken; using ephemeral port");
            TcpListener::bind(("0.0.0.0", 0)).await
                                             .context("binding DirCon listener")?
         }
      };
      let bound_port = listener.local_addr()?.port();
      info!(port = bound_port, "DirCon server listening");

      let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
      let (events, _) = broadcast::channel(64);
      let handle = DirconHandle { registry: registry.clone(),
                                  events:   events.clone(), };

      tokio::spawn(accept_loop(listener, registry, events));

      Ok(Self { handle, bound_port })
   }
}

async fn accept_loop(listener: TcpListener, registry: Registry, events: broadcast::Sender<ButtonEvent>)
{
   static CLIENT_IDS: AtomicU64 = AtomicU64::new(0);
   loop
   {
      match listener.accept().await
      {
         | Ok((stream, peer)) =>
         {
            let client_id = CLIENT_IDS.fetch_add(1, Ordering::Relaxed);
            info!(%peer, client_id, "DirCon client connected");
            registry.lock()
                    .unwrap()
                    .insert(client_id, ClientReadiness::default());
            let registry = registry.clone();
            let events_rx = events.subscribe();
            tokio::spawn(async move {
               if let Err(error) = handle_client(stream, client_id, &registry, events_rx).await
               {
                  warn!(client_id, %error, "DirCon client error");
               }
               registry.lock().unwrap().remove(&client_id);
               info!(client_id, "DirCon client disconnected");
            });
         }
         | Err(error) =>
         {
            warn!(%error, "DirCon accept failed");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
         }
      }
   }
}

async fn handle_client(stream: TcpStream, client_id: u64, registry: &Registry, mut events: broadcast::Receiver<ButtonEvent>) -> Result<()>
{
   let (mut reader, mut writer) = stream.into_split();
   let mut buf: Vec<u8> = Vec::with_capacity(4096);
   let mut tmp = [0u8; 4096];
   let mut reassembler = AppInfoReassembler::default();
   let mut seq: u16 = 0;

   loop
   {
      tokio::select! {
          read = reader.read(&mut tmp) => {
              let n = read?;
              if n == 0 {
                  return Ok(()); // clean disconnect
              }
              buf.extend_from_slice(&tmp[..n]);

              let mut pos = 0;
              loop {
                  match DirconFrame::parse(&buf[pos..]) {
                      Ok(Some((frame, consumed))) => {
                          pos += consumed;
                          debug!(client_id, msg_type = ?frame.msg_type, len = frame.payload.len(), "DirCon frame");
                          if let Some(reply) = dispatch(frame, client_id, registry, &mut reassembler) {
                              writer.write_all(&reply.serialize()).await?;
                          }
                      }
                      Ok(None) => break, // incomplete frame: read more
                      Err(error) => {
                          warn!(client_id, %error, "DirCon protocol violation; closing");
                          return Ok(());
                      }
                  }
              }
              buf.drain(..pos);
          }
          event = events.recv() => {
              match event {
                  Ok(ButtonEvent { button_id, state_byte, force }) => {
                      let ready = {
                          let clients = registry.lock().unwrap();
                          clients.get(&client_id).is_some_and(|c| c.subscribed && (force || c.supports(button_id)))
                      };
                      if ready {
                          let value = encode_button_state(&[(button_id, state_byte)]);
                          let mut payload = BytesMut::with_capacity(16 + value.len());
                          payload.put_slice(&uuid_to_bytes(BUTTON_STATE_CHARACTERISTIC));
                          payload.put_slice(&value);
                          let frame = DirconFrame::notification(seq, payload.freeze());
                          seq = seq.wrapping_add(1);
                          writer.write_all(&frame.serialize()).await?;
                      }
                  }
                  Err(broadcast::error::RecvError::Lagged(missed)) => {
                      warn!(client_id, missed, "DirCon notification receiver lagged");
                  }
                  Err(broadcast::error::RecvError::Closed) => return Ok(()),
              }
          }
      }
   }
}

fn dispatch(frame: DirconFrame, client_id: u64, registry: &Registry, reassembler: &mut AppInfoReassembler) -> Option<DirconFrame>
{
   match frame.msg_type
   {
      | MsgType::DiscoverServices =>
      {
         // Only the OBC service is exposed.
         info!(client_id, "DiscoverServices");
         Some(DirconFrame::reply(MsgType::DiscoverServices, uuid_to_bytes(OBC_SERVICE).to_vec()))
      }
      | MsgType::DiscoverChars =>
      {
         let Some(service_uuid) = uuid_from_bytes(&frame.payload)
         else
         {
            warn!(client_id, "DiscoverChars payload too short");
            return Some(DirconFrame::reply(MsgType::DiscoverChars, Bytes::new()));
         };
         let mut out = BytesMut::new();
         out.put_slice(&uuid_to_bytes(service_uuid));
         if service_uuid == OBC_SERVICE
         {
            out.put_slice(&uuid_to_bytes(BUTTON_STATE_CHARACTERISTIC));
            out.put_u8(char_flags::NOTIFY);
            out.put_slice(&uuid_to_bytes(APP_INFO_CHARACTERISTIC));
            out.put_u8(char_flags::WRITE);
            info!(client_id, "DiscoverChars for OBC service");
         }
         else
         {
            warn!(client_id, %service_uuid, "DiscoverChars for unknown service");
         }
         Some(DirconFrame::reply(MsgType::DiscoverChars, out.freeze()))
      }
      | MsgType::ReadChar =>
      {
         // No readable characteristics; echo the UUID with an empty value
         // (DirCon has no error frame).
         let mut out = BytesMut::new();
         if let Some(char_uuid) = uuid_from_bytes(&frame.payload)
         {
            out.put_slice(&uuid_to_bytes(char_uuid));
            debug!(client_id, %char_uuid, "ReadChar (no value)");
         }
         Some(DirconFrame::reply(MsgType::ReadChar, out.freeze()))
      }
      | MsgType::WriteChar =>
      {
         let Some(char_uuid) = uuid_from_bytes(&frame.payload)
         else
         {
            warn!(client_id, "WriteChar payload too short");
            return Some(DirconFrame::reply(MsgType::WriteChar, Bytes::new()));
         };
         let value = &frame.payload[16..];
         if char_uuid == APP_INFO_CHARACTERISTIC
         {
            if let Some(info) = reassembler.offer(value)
            {
               info!(
                   client_id,
                   app_id = %info.app_id,
                   version = %info.app_version,
                   buttons = info.supported_buttons.len(),
                   "trainer app identified over DirCon"
               );
               if let Some(client) = registry.lock().unwrap().get_mut(&client_id)
               {
                  client.app_info = Some(info);
               }
            }
         }
         else
         {
            debug!(client_id, %char_uuid, len = value.len(), "WriteChar to unknown characteristic");
         }
         Some(DirconFrame::reply(MsgType::WriteChar, uuid_to_bytes(char_uuid).to_vec()))
      }
      | MsgType::Subscribe =>
      {
         let Some(char_uuid) = uuid_from_bytes(&frame.payload)
         else
         {
            warn!(client_id, "Subscribe payload too short");
            return Some(DirconFrame::reply(MsgType::Subscribe, Bytes::new()));
         };
         // Tolerant extension from bt-bridge: 17th byte 0x00 = unsubscribe.
         let unsubscribe = frame.payload.len() >= 17 && frame.payload[16] == 0x00;
         if char_uuid == BUTTON_STATE_CHARACTERISTIC
         {
            if let Some(client) = registry.lock().unwrap().get_mut(&client_id)
            {
               client.subscribed = !unsubscribe;
            }
            info!(client_id, subscribed = !unsubscribe, "Button State subscription");
         }
         else
         {
            debug!(client_id, %char_uuid, "Subscribe for unknown characteristic; acking");
         }
         Some(DirconFrame::reply(MsgType::Subscribe, uuid_to_bytes(char_uuid).to_vec()))
      }
      | MsgType::Notification =>
      {
         warn!(client_id, "unexpected Notification frame from client");
         None
      }
   }
}

#[cfg(test)]
mod tests
{
   use super::*;

   fn app_info_message(buttons: &[u8]) -> Vec<u8>
   {
      let mut message = vec![0x04, 0x01];
      message.push(9);
      message.extend_from_slice(b"TPVirtual");
      message.push(5);
      message.extend_from_slice(b"1.0.0");
      message.push(buttons.len() as u8);
      message.extend_from_slice(buttons);
      message
   }

   fn registry_with_client(client_id: u64) -> Registry
   {
      let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
      registry.lock()
              .unwrap()
              .insert(client_id, ClientReadiness::default());
      registry
   }

   #[test]
   fn discover_services_returns_obc_uuid()
   {
      let registry = registry_with_client(0);
      let mut reassembler = AppInfoReassembler::default();
      let request = DirconFrame::reply(MsgType::DiscoverServices, Bytes::new());
      let reply = dispatch(request, 0, &registry, &mut reassembler).unwrap();
      assert_eq!(reply.payload.len(), 16);
      assert_eq!(uuid_from_bytes(&reply.payload).unwrap(), OBC_SERVICE);
   }

   #[test]
   fn discover_chars_lists_button_state_and_app_info()
   {
      let registry = registry_with_client(0);
      let mut reassembler = AppInfoReassembler::default();
      let request = DirconFrame::reply(MsgType::DiscoverChars, uuid_to_bytes(OBC_SERVICE).to_vec());
      let reply = dispatch(request, 0, &registry, &mut reassembler).unwrap();
      // service uuid + 2 × (uuid + flags)
      assert_eq!(reply.payload.len(), 16 + 2 * 17);
      assert_eq!(uuid_from_bytes(&reply.payload[16..32]).unwrap(), BUTTON_STATE_CHARACTERISTIC);
      assert_eq!(reply.payload[32], char_flags::NOTIFY);
      assert_eq!(uuid_from_bytes(&reply.payload[33..49]).unwrap(), APP_INFO_CHARACTERISTIC);
      assert_eq!(reply.payload[49], char_flags::WRITE);
   }

   #[test]
   fn subscribe_then_app_info_makes_client_ready()
   {
      let registry = registry_with_client(7);
      let mut reassembler = AppInfoReassembler::default();
      let (events, _rx) = broadcast::channel(4);
      let handle = DirconHandle { registry: registry.clone(),
                                  events };

      // Not ready before subscription.
      assert!(!handle.is_ready_for(0x01));

      let subscribe = DirconFrame::reply(MsgType::Subscribe, uuid_to_bytes(BUTTON_STATE_CHARACTERISTIC).to_vec());
      let ack = dispatch(subscribe, 7, &registry, &mut reassembler).unwrap();
      assert_eq!(uuid_from_bytes(&ack.payload).unwrap(), BUTTON_STATE_CHARACTERISTIC);

      // Subscribed without App Info: TPV default set applies.
      assert!(handle.is_ready_for(0x01));
      assert!(!handle.is_ready_for(0x18)); // steering not in TPV default set

      // App Info restricting to shifts only.
      let mut write = uuid_to_bytes(APP_INFO_CHARACTERISTIC).to_vec();
      write.extend_from_slice(&app_info_message(&[0x01, 0x02]));
      let request = DirconFrame::reply(MsgType::WriteChar, write);
      dispatch(request, 7, &registry, &mut reassembler).unwrap();

      assert!(handle.is_ready_for(0x02));
      assert!(!handle.is_ready_for(0x33)); // Pause no longer supported
   }

   #[test]
   fn unsubscribe_via_extension_byte()
   {
      let registry = registry_with_client(1);
      let mut reassembler = AppInfoReassembler::default();
      let (events, _rx) = broadcast::channel(4);
      let handle = DirconHandle { registry: registry.clone(),
                                  events };

      let mut payload = uuid_to_bytes(BUTTON_STATE_CHARACTERISTIC).to_vec();
      dispatch(DirconFrame::reply(MsgType::Subscribe, payload.clone()), 1, &registry, &mut reassembler);
      assert!(handle.is_ready_for(0x01));

      payload.push(0x00);
      dispatch(DirconFrame::reply(MsgType::Subscribe, payload), 1, &registry, &mut reassembler);
      assert!(!handle.is_ready_for(0x01));
   }
}
