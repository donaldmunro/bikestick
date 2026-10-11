//! Portable helpers for host identity and per-user directories.

use std::path::PathBuf;

const MAC_UNKNOWN: &str = "00-00-00-00-00-00";

/// Host name sanitized for use as an mDNS host label.
pub fn hostname() -> String
{
   let raw = gethostname::gethostname().to_string_lossy().into_owned();
   let cleaned: String = raw.trim()
                            .chars()
                            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
                            .collect();
   let cleaned = cleaned.trim_matches('-');
   if cleaned.is_empty() { "bikestick".to_string() } else { cleaned.to_string() }
}

/// MAC address in `XX-XX-XX-XX-XX-XX` form. `ip` is the outbound IPv4 address
/// (unused for now; Linux uses the default-route interface, others the first NIC).
pub fn host_mac(_ip: &str) -> String
{
   #[cfg(target_os = "linux")]
   {
      if let Some(mac) = linux_default_route_mac()
      {
         return mac;
      }
   }
   match mac_address::get_mac_address()
   {
      | Ok(Some(mac)) => mac.to_string().replace(':', "-").to_uppercase(),
      | _ => MAC_UNKNOWN.to_string(),
   }
}

#[cfg(target_os = "linux")]
fn linux_default_route_mac() -> Option<String>
{
   fn iface_mac(iface: &str) -> Option<String>
   {
      let mac = std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok()?;
      let mac = mac.trim();
      (mac.len() == 17 && mac != "00:00:00:00:00:00").then(|| mac.replace(':', "-").to_uppercase())
   }

   let content = std::fs::read_to_string("/proc/net/route").ok()?;
   let iface = content.lines().skip(1).find_map(|line| {
                                          let mut fields = line.split_whitespace();
                                          match (fields.next(), fields.next())
                                          {
                                             | (Some(iface), Some("00000000")) => Some(iface.to_string()),
                                             | _ => None,
                                          }
                                       })?;
   iface_mac(&iface)
}

/// Directory holding `config.toml`.
pub fn config_dir() -> Option<PathBuf>
{
   #[cfg(target_os = "linux")]
   let dirs = directories::ProjectDirs::from("foo", "bar", "bikestick");
   #[cfg(not(target_os = "linux"))]
   let dirs = directories::ProjectDirs::from("io.github", "donaldmunro", "bikestick");
   dirs.map(|d| d.config_dir().to_path_buf())
}
