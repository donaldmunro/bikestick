//! OpenBikeControl BLE peripheral: GATT application + advertising supervision.
//!
//! OpenBikeControl compatible layout: OBC service with a Button State notify
//! characteristic and an App Info write characteristic, plus Device
//! Information and Battery services. App connects as central, subscribes to
//! Button State, writes its App Info (possibly fragmented), and then receives
//! `[0x01, id, state]` notifications.

use std::{collections::BTreeSet, sync::Arc};

use anyhow::{Context, Result};
use bluer::{Adapter, Session,
            adv::{Advertisement, AdvertisementHandle, Type as AdvType},
            gatt::local::{Application, ApplicationHandle, Characteristic, CharacteristicNotifier, CharacteristicNotify, CharacteristicNotifyMethod,
                          CharacteristicRead, CharacteristicWrite, CharacteristicWriteMethod, Service}};
use futures::FutureExt;
use tokio::{sync::Mutex,
            time::{Duration, sleep}};
use tracing::{debug, info, warn};

use crate::{config::ObcConfig,
            obc::{constants::*, protocol::AppInfoReassembler}};

#[derive(Default)]
struct PeripheralState
{
   notifier:    Option<CharacteristicNotifier>,
   app_info:    Option<crate::obc::protocol::AppInfo>,
   reassembler: AppInfoReassembler,
}

/// Cloneable handle used by the output backend.
#[derive(Clone)]
pub struct ObcHandle
{
   state: Arc<Mutex<PeripheralState>>,
}

impl ObcHandle
{
   /// True once a central is subscribed AND has declared support for `button_id`.
   pub async fn is_ready_for(&self, button_id: u8) -> bool
   {
      let state = self.state.lock().await;
      state.notifier.as_ref().is_some_and(|n| !n.is_stopped())
      && state.app_info
              .as_ref()
              .is_some_and(|info| info.supported_buttons.contains(&button_id))
   }

   /// True once a central is subscribed, regardless of declared buttons.
   pub async fn has_subscriber(&self) -> bool
   {
      let state = self.state.lock().await;
      state.notifier.as_ref().is_some_and(|n| !n.is_stopped())
   }

   /// Send one `[0x01, id, state]` notification (no declared-button check).
   pub async fn send_button(&self, button_id: u8, state_byte: u8) -> Result<()>
   {
      let mut state = self.state.lock().await;
      let Some(notifier) = state.notifier.as_mut()
      else
      {
         anyhow::bail!("no subscribed central");
      };
      let message = crate::obc::protocol::encode_button_state(&[(button_id, state_byte)]);
      notifier.notify(message).await.context("notify failed")?;
      Ok(())
   }
}

pub struct ObcPeripheral
{
   pub handle:  ObcHandle,
   // Held to keep the GATT app registered for the daemon's lifetime.
   _app_handle: ApplicationHandle,
   // Keeps the D-Bus connection to bluetoothd alive.
   _session:    Session,
}

impl ObcPeripheral
{
   /// Register the GATT application and spawn the advertising supervisor.
   pub async fn start(adapter_name: &str, config: ObcConfig) -> Result<Self>
   {
      let (session, adapter) = crate::ble::adapter(adapter_name).await?;
      let state = Arc::new(Mutex::new(PeripheralState::default()));

      let application = build_application(state.clone(), &config);
      let app_handle = adapter.serve_gatt_application(application)
                              .await
                              .context("registering GATT application")?;
      info!("OBC GATT application registered");

      tokio::spawn(advertising_supervisor(adapter, config.clone(), state.clone()));

      Ok(Self { handle:      ObcHandle { state },
                _app_handle: app_handle,
                _session:    session, })
   }
}

fn read_only_characteristic(uuid: uuid::Uuid, value: Vec<u8>) -> Characteristic
{
   Characteristic { uuid,
                    read: Some(CharacteristicRead { read: true,
                                                    fun: Box::new(move |_req| {
                                                       let value = value.clone();
                                                       async move { Ok(value) }.boxed()
                                                    }),
                                                    ..Default::default() }),
                    ..Default::default() }
}

fn build_application(state: Arc<Mutex<PeripheralState>>, config: &ObcConfig) -> Application
{
   let write_state = state.clone();
   let notify_state = state;
   let battery = config.battery_percent.min(100);

   Application { services: vec![
                                Service { uuid: OBC_SERVICE,
                                          primary: true,
                                          characteristics: vec![
      // Button State: notify-only.
      Characteristic { uuid: BUTTON_STATE_CHARACTERISTIC,
                       notify: Some(CharacteristicNotify { notify: true,
                                                           method: CharacteristicNotifyMethod::Fun(Box::new(move |notifier| {
                                                              let state = notify_state.clone();
                                                              async move {
                                                                 info!("central subscribed to Button State");
                                                                 state.lock().await.notifier = Some(notifier);
                                                              }.boxed()
                                                           })),
                                                           ..Default::default() }),
                       ..Default::default() },
      // App Info: write + write-without-response, fragments tolerated.
      Characteristic { uuid: APP_INFO_CHARACTERISTIC,
                       write: Some(CharacteristicWrite { write: true,
                                                         write_without_response: true,
                                                         method: CharacteristicWriteMethod::Fun(Box::new(move |value, _req| {
                                                            let state = write_state.clone();
                                                            async move {
                                                               let mut guard = state.lock().await;
                                                               debug!(bytes = value.len(), "app info fragment");
                                                               if let Some(info) = guard.reassembler.offer(&value)
                                                               {
                                                                  info!(
                                                                      app_id = %info.app_id,
                                                                      version = %info.app_version,
                                                                      buttons = info.supported_buttons.len(),
                                                                      "trainer app identified"
                                                                  );
                                                                  guard.app_info = Some(info);
                                                               }
                                                               Ok(())
                                                            }.boxed()
                                                         })),
                                                         ..Default::default() }),
                       ..Default::default() },
   ],
                                          ..Default::default() },
                                Service { uuid: DEVICE_INFORMATION_SERVICE,
                                          primary: true,
                                          characteristics: vec![
      read_only_characteristic(CHAR_MANUFACTURER_NAME, b"BikeControl".to_vec()),
      read_only_characteristic(CHAR_SERIAL_NUMBER, b"1337".to_vec()),
      read_only_characteristic(CHAR_HARDWARE_REVISION, b"1.0".to_vec()),
      read_only_characteristic(CHAR_FIRMWARE_REVISION, env!("CARGO_PKG_VERSION").as_bytes().to_vec()),
   ],
                                          ..Default::default() },
                                Service { uuid: BATTERY_SERVICE,
                                          primary: true,
                                          characteristics: vec![read_only_characteristic(CHAR_BATTERY_LEVEL, vec![battery])],
                                          ..Default::default() },
   ],
                 ..Default::default() }
}

async fn advertise(adapter: &Adapter, config: &ObcConfig) -> Result<AdvertisementHandle>
{
   let advertisement = Advertisement { advertisement_type: AdvType::Peripheral,
                                       service_uuids: BTreeSet::from([OBC_SERVICE]),
                                       local_name: Some(config.local_name.clone()),
                                       discoverable: Some(true),
                                       ..Default::default() };
   Ok(adapter.advertise(advertisement).await?)
}

/// Keep the advertisement alive: BlueZ can silently stop advertising after a
/// central disconnects so re-register whenever the subscriber goes away.
async fn advertising_supervisor(adapter: Adapter, config: ObcConfig, state: Arc<Mutex<PeripheralState>>)
{
   let mut handle: Option<AdvertisementHandle> = None;
   let mut had_subscriber = false;

   loop
   {
      if handle.is_none()
      {
         match advertise(&adapter, &config).await
         {
            | Ok(new_handle) =>
            {
               info!(name = %config.local_name, "advertising OBC service");
               handle = Some(new_handle);
            }
            | Err(error) =>
            {
               warn!(%error, "advertising failed; retrying in 5s");
               sleep(Duration::from_secs(5)).await;
               continue;
            }
         }
      }

      sleep(Duration::from_secs(2)).await;

      let has_subscriber = {
         let mut guard = state.lock().await;
         let alive = guard.notifier.as_ref().is_some_and(|n| !n.is_stopped());
         if !alive && guard.notifier.is_some()
         {
            // Central went away: clear session state.
            guard.notifier = None;
            guard.app_info = None;
            guard.reassembler.reset();
         }
         alive
      };

      if had_subscriber && !has_subscriber
      {
         info!("central disconnected; refreshing advertisement");
         handle = None; // drop -> unregister -> re-register next iteration
      }
      had_subscriber = has_subscriber;
   }
}
