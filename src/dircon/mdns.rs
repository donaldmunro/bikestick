//! mDNS advertisement of the DirCon service, modeled on bt-bridge's
//! implementation:
//! type `_wahoo-fitness-tnp._tcp`, TXT records `ble-service-uuids`
//! (the OBC service UUID string), `mac-address`, and `serial-number`.

use anyhow::{Context, Result};
use mdns_sd::{ServiceDaemon, ServiceInfo};
use tracing::{info, warn};

use crate::{obc::constants::OBC_SERVICE, platform};

const SERVICE_TYPE: &str = "_wahoo-fitness-tnp._tcp.local.";

/// `wahoo-fitness-tnp` is 17 bytes - Wahoo implementation violates the RFC 6763
/// 15-byte service-name limit that mdns-sd enforces by default.
const SERVICE_NAME_LEN_MAX: u8 = 30;

/// Keeps the advertisement registered; unregisters on drop.
pub struct DirconAdvertisement
{
   daemon:   ServiceDaemon,
   fullname: String,
}

impl DirconAdvertisement
{
   pub fn register(instance_name: &str, port: u16) -> Result<Self>
   {
      let daemon = ServiceDaemon::new().context("creating mDNS daemon")?;
      daemon.set_service_name_len_max(SERVICE_NAME_LEN_MAX)
            .context("raising mDNS service-name length limit")?;

      let ip = local_ipv4();
      let host_name = format!("{}.local.", platform::hostname());
      // Lowercase hyphenated form
      let ble_uuids = OBC_SERVICE.hyphenated().to_string();
      let properties = [("ble-service-uuids", ble_uuids.as_str()),
                        ("mac-address", &platform::host_mac(&ip)),
                        ("serial-number", "1234567890")];

      let service_info =
         ServiceInfo::new(SERVICE_TYPE, instance_name, &host_name, ip.as_str(), port, &properties[..]).context("building mDNS service info")?;
      let fullname = service_info.get_fullname().to_string();

      daemon.register(service_info)
            .context("registering mDNS service")?;
      info!(
          instance = instance_name,
          %ip,
          port,
          service_type = SERVICE_TYPE,
          "DirCon service advertised via mDNS"
      );

      Ok(Self { daemon, fullname })
   }
}

impl Drop for DirconAdvertisement
{
   fn drop(&mut self)
   {
      if let Err(error) = self.daemon.unregister(&self.fullname)
      {
         warn!(%error, "mDNS unregister failed");
      }
      let _ = self.daemon.shutdown();
   }
}

/// Outbound IPv4 via the route table (no packets are sent).
pub fn local_ipv4() -> String
{
   std::net::UdpSocket::bind("0.0.0.0:0").and_then(|socket| {
                                            socket.connect("8.8.8.8:53")?;
                                            socket.local_addr()
                                         })
                                         .map(|addr| addr.ip().to_string())
                                         .unwrap_or_else(|error| {
                                            warn!(%error, "could not determine outbound IPv4; advertising 0.0.0.0");
                                            "0.0.0.0".to_string()
                                         })
}
