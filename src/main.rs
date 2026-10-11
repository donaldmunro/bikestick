#[cfg(target_os = "linux")] mod ble;
mod config;
mod dircon;
mod events;
mod gamepad;
mod mapping;
mod obc;
mod output;
mod platform;
mod profiles;
mod zwift_play;

use std::path::PathBuf;

use anyhow::Result;
use clap::{ArgAction, Parser};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;
use config::{Config, OutputMode};
use events::{Edge, InGameAction, Source, SourceButton};
use obc::encoding::ObcEncoding;
use mapping::Mapper;
use output::{Delivered, OutputRouter,
             keyboard::{self, Profile}};

/// Bridge Zwift Play controllers and generic gamepads to Virtual Cycling Applications.
///
/// Command line and configuration options include:
/// 1. Choosing how to connect to the application (network using OpenBikeControl/keyboard emulation/BT using OpenBikeControl)
/// 2. Mapping actions to gamepad/Play controller buttons (choosing a preset mapping, dumping mapping files to
/// serve as templates for customization, and choosing user-defined mapping files; individual buttons can also be
/// overridden in config.toml).
/// 3. If using the keyboard connection method then specifying which keys are supported by the application
/// by dumping keyboard configuration files to serve as templates for customization and choosing user-defined
/// keyboard configuration files.
/// 4. Overriding the default configuration file location.
/// 5. Various logging and debugging options (-v/-q adjust the log level; RUST_LOG overrides both).
///
/// Note if using the keyboard connection method that not all options that the application supports may be available,
/// as some application (MyWhoosh for example) support a small subset of keyboard inputs, while other (eg TPV )
/// support an almost complete set of keyboard inputs (and BikeTerra also allows the user to customize the keypresses).
/// If using OpenBikeControl (OBC) as the connection method and the application supports it, then more options may be
/// available.
#[derive(Parser, Debug)]
#[command(
          name = "bikestick",
          version,
          about = "Bridge Zwift Play controllers and generic gamepads to Virtual Cycling Applications.",
          long_about = r#"
Bridge Zwift Play controllers and generic gamepads to Virtual Cycling Applications.

Command line and configuration options include:
1. Choosing how to connect to the application (network using OpenBikeControl/keyboard emulation/BT using OpenBikeControl)
2. Mapping actions to gamepad/Play controller buttons (choosing a preset mapping, dumping mapping files to
serve as templates for customization, and choosing user-defined mapping files; individual buttons can also be
overridden in config.toml).
3. If using the keyboard connection method then specifying which keys are supported by the application 
by dumping keyboard configuration files to serve as templates for customization and choosing user-defined 
keyboard configuration files.
4. Overriding the default configuration file location.
5. Various logging and debugging options (-v/-q adjust the log level; RUST_LOG overrides both).

Note that if using the keyboard connection method, not all options that the application supports may be available,
as some application (MyWhoosh for example) support a small subset of keyboard inputs, while other (eg TPV )
support an almost complete set of keyboard inputs (and BikeTerra also allows the user to customize the keypresses).
If using OpenBikeControl (OBC) as the connection method and the application supports it, then more options may be 
available."#
)]
struct Cli
{
   /// Path to config.toml (default: ~/.config/bikestick/config.toml)
   #[arg(short = 'C', long = "config", help = "Path to configuration file (default: ~/.config/bikestick/config.toml)")]
   config: Option<PathBuf>,

   /// Override [general].output: dircon | obc | keyboard | auto
   #[arg(
         short = 'c',
         long = "connection",
         alias = "conn",
         help = r#"Connection type used to connect to the application: network | dircon | keyboard | obc | auto. 
      network = Network mDNS (DirCon) which requires pairing in the client application.
      dircon = Same as "network"
      keyboard = Virtual keyboard input. Sends keystrokes to the system. Does not require pairing.
      obc = Bluetooth OBC peripheral i.e a Bluetooth OpenBikeControl device (Currently Linux only).      
      "#
   )]
   connection: Option<String>,

   #[arg(
         short = 'm',
         long = "map",
         value_name = "NAME|PATH",
         help = r#"Mapping of controller buttons to actions: a name or a file path.
   Preset mappings:
      ride = Free ride,
      race = Racing (events: group focus, helicopter view, home),
      workout = Workout,
   or the name of a user mapping file (<config dir>/mappings/NAME.toml),
   or the path to a mapping file (a value containing '/' or '\' or ending in .toml).
   Overrides [mapping].name in config.toml; defaults to ride.
   Use --list-maps to list the available mappings and -M to write a template.
      "#
   )]
   map: Option<String>,

   /// Write the embedded mapping NAME to the config directory and exit.
   #[arg(
         short = 'M',
         long = "write-map",
         value_name = "NAME",
         help = r#"Write the embedded mapping NAME to the config directory as a template for customization, 
      print the full path and exit (In Linux and MacOS if the VISUAL (or EDITOR) environment variable is set, 
      the file will be opened in the editor)."#
   )]
   write_map: Option<String>,

   /// List embedded and user mappings and exit.
   #[arg(long = "list-maps")]
   list_maps: bool,

   /// Keyboard profile NAME or PATH, overriding [keyboard].profile.
   #[arg(
         short = 'k',
         long = "keys",
         value_name = "NAME|PATH",
         help = r#"Keyboard profile to use: a name (user file <config dir>/keyboard/NAME.toml, else the embedded profile)
      or the path to a profile file. Overrides [keyboard].profile in config.toml."#
   )]
   keys: Option<String>,

   /// Increase log verbosity (-v = debug, -vv = trace).
   #[arg(short = 'v', long = "verbose", action = ArgAction::Count, conflicts_with = "quiet")]
   verbose: u8,

   /// Only log warnings and errors.
   #[arg(short = 'q', long = "quiet")]
   quiet: bool,

   /// Print the effective configuration (file + command line overrides) as TOML and exit.
   #[arg(long)]
   print_config: bool,

   /// Check environment (adapter, bluetoothd, uinput) and exit.
   #[arg(long)]
   doctor: bool,

   /// Write the embedded default keyboard profile NAME to the config directory and exit.
   #[arg(
         short = 'K',
         long = "write-keys",
         value_name = "NAME",
         help = r#"Write the embedded default keyboard profile NAME to the config directory, 
      print the full path and exit (In Linux and MacOS if the VISUAL (or EDITOR) environment variable is set, 
      the file will be opened in the editor).
      This option is only applicable to keyboard profiles."#
   )]
   dump_keyboard_profile: Option<String>,

   /// Write the documented default config file and exit.
   #[arg(
         long = "write-config",
         help = r#"Write the documented default configuration file to the -C/--config path (default: the standard config
      location), print the full path and exit (In Linux and MacOS if the VISUAL (or EDITOR) environment variable is set,
      the file will be opened in the editor). Refuses to overwrite an existing file unless --force is given."#
   )]
   write_config: bool,

   /// With --write-config / --write-keys / --write-map: overwrite an existing file.
   #[arg(long)]
   force: bool,

   /// List embedded and user keyboard profiles and exit.
   #[arg(long = "list-keys")]
   list_keyboard_profiles: bool,

   /// Repeatedly send this action every 5s (e.g. "shift_up") for testing
   /// the OBC peripheral without physical controllers.
   #[arg(long)]
   simulate_press: Option<String>,

   /// Repeatedly send a raw OBC button <ID>[:<VALUE>] every 5s (hex like
   /// 0x42 or 0x40:0x05, or decimal; VALUE defaults to 1) via DirCon/OBC BLE,
   /// bypassing action mapping and the client's declared button list. For
   /// discovering what an app does with a given id/value.
   #[arg(long, value_name = "ID[:VALUE]", value_parser = parse_obc_probe)]
   simulate_obc: Option<ObcProbe>,
}

/// A raw OBC button id and pressed-state value for `--simulate-obc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObcProbe
{
   id:    u8,
   value: u8,
}

/// Synthetic input from the simulate tasks, delivered straight to the router
/// (bypassing the source-button mapper).
enum Simulated
{
   Action(InGameAction, Edge),
   Obc
   {
      id:    u8,
      state: u8,
   },
}

#[tokio::main]
async fn main() -> Result<()>
{
   let cli = Cli::parse();

   if cli.list_keyboard_profiles || cli.list_maps
   {
      if cli.list_keyboard_profiles
      {
         keyboard::CATALOG.print_list();
      }
      if cli.list_maps
      {
         mapping::CATALOG.print_list();
      }
      return Ok(());
   }
   if cli.write_config
   {
      let Some(path) = cli.config.clone().or_else(Config::default_path)
      else
      {
         anyhow::bail!("could not determine a config directory");
      };
      Config::write_template(&path, cli.force)?;
      show_written(&path)?;
      return Ok(());
   }
   if cli.dump_keyboard_profile.is_some() || cli.write_map.is_some()
   {
      if let Some(name) = &cli.dump_keyboard_profile
      {
         let path = keyboard::CATALOG.dump(name, cli.force)?;
         show_written(&path)?;
      }
      if let Some(name) = &cli.write_map
      {
         let path = mapping::CATALOG.dump(name, cli.force)?;
         show_written(&path)?;
      }
      return Ok(());
   }
   let mut config = Config::load(cli.config.as_deref())?;

   if let Some(connection_method) = &cli.connection
   {
      config.general.output = match connection_method.as_str()
      {
         | "network" | "dircon" => OutputMode::Dircon,
         | "obc" => OutputMode::Obc,
         | "keyboard" => OutputMode::Keyboard,
         | "auto" => OutputMode::Auto,
         | other => anyhow::bail!("invalid --connection '{other}' (network|dircon|obc|keyboard|auto)"),
      };
   }
   // Precedence: command line > config.toml > built-in default.
   if let Some(name) = &cli.map
   {
      config.mapping.name = name.clone();
   }
   if let Some(profile) = &cli.keys
   {
      config.keyboard.profile = profile.clone();
   }

   // RUST_LOG wins; otherwise -q / -v / -vv adjust the configured level.
   let level = if cli.quiet
   {
      "warn".to_string()
   }
   else
   {
      match cli.verbose
      {
         | 0 => config.general.log_level.clone(),
         | 1 => "debug".to_string(),
         | _ => "trace".to_string(),
      }
   };
   // Logs go to stderr so stdout stays clean for --print-config.
   tracing_subscriber::fmt().with_writer(std::io::stderr)
                            .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level)))
                            .init();

   if cli.config.is_none()
   {
      if let Some(path) = Config::default_path().filter(|path| !path.exists())
      {
         info!("no config file at {}; using defaults (create one with `bikestick --write-config`)", path.display());
      }
   }

   if cli.print_config
   {
      print!("{}", config.to_toml()?);
      return Ok(());
   }
   if cli.doctor
   {
      return doctor(&config).await;
   }

   run(config, cli.simulate_press, cli.simulate_obc).await
}

/// Print the path of a freshly written file and, on Linux and macOS, open it
/// in $VISUAL (else $EDITOR) if set, waiting for the editor to exit.
fn show_written(path: &std::path::Path) -> Result<()>
{
   println!("{}", path.display());
   #[cfg(any(target_os = "linux", target_os = "macos"))]
   {
      let editor = ["VISUAL", "EDITOR"].iter()
                                       .filter_map(|name| std::env::var(name).ok())
                                       .find(|value| !value.trim().is_empty());
      if let Some(editor) = editor
      {
         let mut words = editor.split_whitespace();
         let program = words.next().unwrap_or_default();
         let status = std::process::Command::new(program).args(words)
                                                         .arg(path)
                                                         .status()
                                                         .map_err(|error| anyhow::anyhow!("could not run editor '{editor}': {error}"))?;
         if !status.success()
         {
            anyhow::bail!("editor '{editor}' exited with {status}");
         }
      }
   }
   Ok(())
}

/// Actions `mapper` can produce that `profile` has no key for, sorted by name.
fn uncovered_actions(mapper: &Mapper, profile: &Profile) -> Vec<InGameAction>
{
   mapper.actions()
         .into_iter()
         .filter(|action| !profile.supports(*action))
         .collect()
}

fn action_list(actions: &[InGameAction]) -> String
{
   actions.iter()
          .map(|action| action.name())
          .collect::<Vec<_>>()
          .join(", ")
}

async fn run(config: Config, simulate_press: Option<String>, simulate_obc: Option<ObcProbe>) -> Result<()>
{
   // DirCon (TCP + mDNS): the path TPV under Wine can actually see. The
   // advertisement handle must stay alive for the daemon's lifetime.
   let mut _dircon_advertisement = None;
   let dircon_handle = if config.dircon.enabled
   {
      match dircon::server::DirconServer::start(config.dircon.port).await
      {
         | Ok(server) =>
         {
            match dircon::mdns::DirconAdvertisement::register(&config.dircon.instance_name, server.bound_port)
            {
               | Ok(advertisement) => _dircon_advertisement = Some(advertisement),
               | Err(error) => warn!(%error, "mDNS advertisement failed; DirCon reachable by IP:port only"),
            }
            Some(server.handle.clone())
         }
         | Err(error) =>
         {
            warn!(%error, "DirCon server unavailable");
            None
         }
      }
   }
   else
   {
      None
   };

   // Order matters: GATT app + advertisement first, central scan afterwards.
   // `_peripheral` stays alive for the daemon's lifetime to keep the GATT
   // application registered with BlueZ.
   let mut _peripheral = None;
   let obc_handle = if config.obc.enabled
   {
      match obc::peripheral::ObcPeripheral::start(&config.general.adapter, config.obc.clone()).await
      {
         | Ok(peripheral) =>
         {
            let handle = peripheral.handle.clone();
            _peripheral = Some(peripheral);
            Some(handle)
         }
         | Err(error) =>
         {
            warn!(%error, "OBC peripheral unavailable");
            None
         }
      }
   }
   else
   {
      None
   };

   let mapper = Mapper::load(&config.mapping.name, &config.mapping)?;

   let keyboard = if config.keyboard.enabled
   {
      match Profile::load(&config.keyboard.profile)
      {
         | Ok(profile) =>
         {
            let uncovered = uncovered_actions(&mapper, &profile);
            let profile_name = config.keyboard.profile.clone();
            match keyboard::create_backend(profile)
            {
               | Ok(backend) =>
               {
                  if !uncovered.is_empty()
                  {
                     match config.general.output
                     {
                        | OutputMode::Keyboard =>
                        {
                           warn!("these mapped actions have no key in profile {profile_name} and will be dropped: {}", action_list(&uncovered))
                        }
                        | OutputMode::Auto =>
                        {
                           info!("mapped actions with no key in keyboard profile {profile_name} rely on OBC/DirCon: {}", action_list(&uncovered))
                        }
                        | _ => (),
                     }
                  }
                  Some(backend)
               }
               | Err(error) =>
               {
                  warn!(error = %format!("{error:#}"), "keyboard backend unavailable");
                  None
               }
            }
         }
         | Err(error) =>
         {
            warn!(error = %format!("{error:#}"), "keyboard backend unavailable");
            None
         }
      }
   }
   else
   {
      None
   };

   if dircon_handle.is_none() && obc_handle.is_none() && keyboard.is_none()
   {
      anyhow::bail!("no output backend available (DirCon, OBC BLE, and keyboard all failed/disabled)");
   }

   let encoding = ObcEncoding::from_config(&config.obc)?;
   let mut router = OutputRouter::new(config.general.output, encoding, dircon_handle, obc_handle.clone(), keyboard);

   let (event_tx, mut event_rx) = mpsc::channel(64);
   let (sim_tx, mut sim_rx) = mpsc::channel::<Simulated>(16);

   // Zwift Play scanning starts after the OBC GATT app + advertisement.
   let registry = zwift_play::SideRegistry::default();
   // bluest never powers the adapter; on Linux do it via bluer (as before the
   // port) so a Zwift-only setup doesn't block in `wait_available()`.
   #[cfg(target_os = "linux")]
   let _power_session = if config.zwift_play.enabled
   {
      match ble::adapter(&config.general.adapter).await
      {
         | Ok((session, _adapter)) => Some(session),
         | Err(error) =>
         {
            warn!(error = %format!("{error:#}"), "could not power Bluetooth adapter");
            None
         }
      }
   }
   else
   {
      None
   };
   if config.zwift_play.enabled
   {
      let zwift_config = config.zwift_play.clone();
      let tx = event_tx.clone();
      let zwift_registry = registry.clone();
      tokio::spawn(async move {
         if let Err(error) = zwift_play::run(zwift_config, tx, zwift_registry).await
         {
            warn!(error = %format!("{error:#}"), "Zwift Play unavailable");
         }
      });
   }
   if config.gamepad.enabled
   {
      gamepad::spawn(config.gamepad.clone(), event_tx.clone());
   }

   if let Some(name) = simulate_press
   {
      let Some(action) = parse_action(&name)
      else
      {
         anyhow::bail!("unknown action '{name}' for --simulate-press");
      };
      let tx = sim_tx.clone();
      tokio::spawn(async move {
         loop
         {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            info!(?action, "simulating press");
            let _ = tx.send(Simulated::Action(action, Edge::Pressed)).await;
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let _ = tx.send(Simulated::Action(action, Edge::Released)).await;
         }
      });
   }
   if let Some(probe) = simulate_obc
   {
      let tx = sim_tx.clone();
      tokio::spawn(async move {
         loop
         {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            info!(id = format_args!("{:#04X}", probe.id), value = probe.value, "simulating raw OBC press");
            let _ = tx.send(Simulated::Obc { id:    probe.id,
                                             state: probe.value, })
                      .await;
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            let _ = tx.send(Simulated::Obc { id: probe.id, state: 0 }).await;
         }
      });
   }
   drop(event_tx);
   drop(sim_tx);

   info!("bikestick running (ctrl-c to stop)");
   loop
   {
      tokio::select! {
          event = event_rx.recv() => {
              let Some(event) = event else { break };
              let actions = mapper.map(event.source, event.button);
              if actions.is_empty() {
                  debug!(source = %event.source, button = event.button.name(), "unmapped button");
                  continue;
              }
              let mut any_delivered = false;
              for &action in actions {
                  let delivered = router.send(action, event.edge).await;
                  info!(source = %event.source, button = event.button.name(), ?action, edge = ?event.edge, ?delivered, "action");
                  any_delivered |= delivered != Delivered::Dropped;
              }

              let is_paddle = matches!(event.source, Source::ZwiftPlayLeft | Source::ZwiftPlayRight)
                  && matches!(event.button, SourceButton::PaddleLeft | SourceButton::PaddleRight);
              if is_paddle && any_delivered && event.edge == Edge::Pressed && config.zwift_play.vibrate_on_shift {
                  registry.vibrate(event.source);
              }
          }
          Some(simulated) = sim_rx.recv() => {
              match simulated {
                  Simulated::Action(action, edge) => {
                      let delivered = router.send(action, edge).await;
                      info!(?action, ?edge, ?delivered, "simulated action");
                  }
                  Simulated::Obc { id, state } => {
                      let delivered = router.send_raw_obc(id, state).await;
                      info!(id = format_args!("{id:#04X}"), state, ?delivered, "simulated raw OBC");
                  }
              }
          }
          result = tokio::signal::ctrl_c() => {
              result?;
              info!("shutting down");
              break;
          }
      }
   }
   Ok(())
}

async fn doctor(config: &Config) -> Result<()>
{
   println!("bikestick doctor\n");

   match Config::default_path()
   {
      | Some(path) => println!("[ok] config path {} ({})",
                               path.display(),
                               if path.exists() { "present" } else { "absent; using defaults - create one with `bikestick --write-config`" }),
      | None => println!("[!!] config path: could not determine a config directory"),
   }
   let ip = dircon::mdns::local_ipv4();
   println!("[ok] mDNS will advertise host {}.local. at {} (MAC {})", platform::hostname(), ip, platform::host_mac(&ip));

   match bluest::Adapter::default().await
   {
      | Some(adapter) => match tokio::time::timeout(std::time::Duration::from_secs(3), adapter.wait_available()).await
      {
         | Ok(Ok(())) => println!("[ok] Zwift Play: Bluetooth adapter available (bluest)"),
         | Ok(Err(error)) => println!("[!!] Zwift Play: Bluetooth adapter unavailable: {error}"),
         | Err(_) => println!("[!!] Zwift Play: Bluetooth adapter not powered on (timed out)"),
      },
      | None => println!("[!!] Zwift Play: no Bluetooth adapter found"),
   }

   #[cfg(target_os = "linux")]
   {
      match ble::adapter(&config.general.adapter).await
      {
         | Ok((_session, adapter)) =>
         {
            println!("[ok] bluetoothd reachable, adapter {} powered", adapter.name());
            match adapter.address().await
            {
               | Ok(address) => println!("[ok] adapter address {address}"),
               | Err(error) => println!("[!!] adapter address: {error}"),
            }
         }
         | Err(error) => println!("[!!] Bluetooth: {error}"),
      }
   }
   #[cfg(not(target_os = "linux"))]
   {
      let _ = config;
      println!("[--] OBC BLE peripheral: not supported on this platform yet");
   }

   let mapper = match Mapper::load(&config.mapping.name, &config.mapping)
   {
      | Ok(mapper) =>
      {
         println!("[ok] mapping '{}' from {} ({} bound buttons)", mapper.name, mapper.source, mapper.bound_buttons());
         Some(mapper)
      }
      | Err(error) =>
      {
         println!("[!!] mapping '{}': {error:#}", config.mapping.name);
         None
      }
   };

   match Profile::load(&config.keyboard.profile)
   {
      | Ok(profile) =>
      {
         println!("[ok] keyboard profile '{}' for {} from {} ({} bindings)",
                  config.keyboard.profile,
                  profile.app,
                  profile.source,
                  profile.bindings.len());
         if let Some(mapper) = &mapper
         {
            let uncovered = uncovered_actions(mapper, &profile);
            if uncovered.is_empty()
            {
               println!("[ok] every action in mapping '{}' has a key in keyboard profile '{}'", mapper.name, config.keyboard.profile);
            }
            else
            {
               println!("[!!] mapped actions with no key in keyboard profile '{}' (dropped with keyboard output; need OBC/DirCon): {}",
                        config.keyboard.profile,
                        action_list(&uncovered));
            }
         }
         match keyboard::create_backend(profile)
         {
            | Ok(_) => println!("[ok] keyboard backend created"),
            | Err(error) =>
            {
               println!("[!!] keyboard backend: {error:#}");
               #[cfg(target_os = "linux")]
               println!("     fix: install udev/99-bikestick-uinput.rules, add user to 'input' group, re-login");
               #[cfg(target_os = "macos")]
               println!("     fix: grant Accessibility access in System Settings > Privacy & Security > Accessibility, then restart bikestick");
            }
         }
      }
      | Err(error) => println!("[!!] keyboard profile '{}': {error:#}", config.keyboard.profile),
   }
   #[cfg(windows)]
   println!("[--] note: keystrokes cannot be injected into elevated (administrator) applications unless bikestick is elevated too");

   match gilrs::Gilrs::new()
   {
      | Ok(gilrs) =>
      {
         let count = gilrs.gamepads().count();
         println!("[ok] gilrs initialized ({count} gamepad(s) present)");
      }
      | Err(error) => println!("[!!] gilrs: {error}"),
   }

   Ok(())
}

fn parse_action(name: &str) -> Option<InGameAction>
{
   // Reuse the serde snake_case names from the config format.
   serde::Deserialize::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(name)).ok()
}

fn parse_obc_number(text: &str) -> Result<u8, String>
{
   let text = text.trim();
   let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
   {
      | Some(hex) => u8::from_str_radix(hex, 16),
      | None => text.parse::<u8>(),
   };
   parsed.map_err(|_| format!("invalid number '{text}' (expected 0..=255, decimal or 0x hex)"))
}

fn parse_obc_probe(text: &str) -> Result<ObcProbe, String>
{
   let (id, value) = match text.split_once(':')
   {
      | Some((id, value)) => (parse_obc_number(id)?, parse_obc_number(value)?),
      | None => (parse_obc_number(text)?, 1),
   };
   if value == 0
   {
      return Err("VALUE 0 means released; use 1..=255".into());
   }
   Ok(ObcProbe { id, value })
}

#[cfg(test)]
mod tests
{
   use super::*;

   #[test]
   fn parses_obc_probe()
   {
      assert_eq!(parse_obc_probe("0x42"), Ok(ObcProbe { id: 0x42, value: 1 }));
      assert_eq!(parse_obc_probe("66"), Ok(ObcProbe { id: 66, value: 1 }));
      assert_eq!(parse_obc_probe("0x40:0x05"), Ok(ObcProbe { id: 0x40, value: 5 }));
      assert_eq!(parse_obc_probe("0x40:5"), Ok(ObcProbe { id: 0x40, value: 5 }));
      assert!(parse_obc_probe("0x100").is_err());
      assert!(parse_obc_probe("abc").is_err());
      assert!(parse_obc_probe("0x40:").is_err());
      assert!(parse_obc_probe("0x40:0").is_err());
      assert!(parse_obc_probe("").is_err());
   }
}
