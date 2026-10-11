//! Source-button -> in-game-action mapping.
//!
//! A mapping is a named TOML file (embedded preset, user file in the config
//! directory's `mappings/` folder, or an arbitrary path) that may `inherit`
//! another mapping. `[mapping.*]` entries in config.toml then replace single
//! buttons of the chosen mapping. A button maps to zero or more actions.

use std::{collections::{BTreeMap, HashMap},
          fmt,
          path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeSeq};
use tracing::info;

use crate::{config::MappingConfig,
            events::{InGameAction, Source, SourceButton},
            profiles::{Catalog, is_path}};

/// Mappings: `<config dir>/mappings/<name>.toml`, else embedded.
pub const CATALOG: Catalog = Catalog { kind:     "mapping",
                                       subdir:   "mappings",
                                       embedded: &[("ride", include_str!("../mappings/ride.toml")),
                                                   ("race", include_str!("../mappings/race.toml")),
                                                   ("workout", include_str!("../mappings/workout.toml"))], };

/// Longest `inherit` chain accepted (guards runaway chains; cycles are
/// detected separately).
const MAX_INHERIT_DEPTH: usize = 8;

type Table = HashMap<SourceButton, Vec<InGameAction>>;

/// The value of one mapping entry: the actions a button triggers. Empty means
/// "none" (no binding). In TOML: `"action"`, `["action", ...]` or `"none"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionSpec(pub Vec<InGameAction>);

impl Serialize for ActionSpec
{
   fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>
   {
      match self.0.as_slice()
      {
         | [] => serializer.serialize_str("none"),
         | [action] => action.serialize(serializer),
         | actions =>
         {
            let mut seq = serializer.serialize_seq(Some(actions.len()))?;
            for action in actions
            {
               seq.serialize_element(action)?;
            }
            seq.end()
         }
      }
   }
}

impl<'de> Deserialize<'de> for ActionSpec
{
   fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>
   {
      struct SpecVisitor;

      impl<'de> de::Visitor<'de> for SpecVisitor
      {
         type Value = ActionSpec;

         fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
         {
            f.write_str("an action name, a non-empty list of action names, or \"none\"")
         }

         fn visit_str<E: de::Error>(self, text: &str) -> Result<ActionSpec, E>
         {
            if text == "none"
            {
               return Ok(ActionSpec(Vec::new()));
            }
            Ok(ActionSpec(vec![text.parse().map_err(E::custom)?]))
         }

         fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<ActionSpec, A::Error>
         {
            let mut actions: Vec<InGameAction> = Vec::new();
            while let Some(text) = seq.next_element::<String>()?
            {
               if text == "none"
               {
                  return Err(de::Error::custom("\"none\" cannot be combined with other actions in a list"));
               }
               let action: InGameAction = text.parse().map_err(de::Error::custom)?;
               if !actions.contains(&action)
               {
                  actions.push(action);
               }
            }
            if actions.is_empty()
            {
               return Err(de::Error::custom("empty action list (use \"none\" to remove a binding)"));
            }
            Ok(ActionSpec(actions))
         }
      }

      deserializer.deserialize_any(SpecVisitor)
   }
}

/// On-disk form of a mapping file.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MappingFile
{
   name:        Option<String>,
   description: Option<String>,
   inherit:     Option<String>,
   zwift_play:  BTreeMap<String, toml::Value>,
   gamepad:     BTreeMap<String, toml::Value>,
}

/// A parsed (but not yet inherited/validated) mapping file.
struct Layer
{
   name:        Option<String>,
   description: Option<String>,
   inherit:     Option<String>,
   zwift_play:  Vec<(String, ActionSpec)>,
   gamepad:     Vec<(String, ActionSpec)>,
}

fn parse_section(origin: &str, section: &str, entries: BTreeMap<String, toml::Value>) -> Result<Vec<(String, ActionSpec)>>
{
   let mut parsed = Vec::new();
   for (key, value) in entries
   {
      let spec = ActionSpec::deserialize(value).map_err(|e| anyhow::anyhow!("{origin}: [{section}] '{key}': {}", e.message()))?;
      parsed.push((key, spec));
   }
   Ok(parsed)
}

fn parse_layer(text: &str, origin: &str) -> Result<Layer>
{
   let file: MappingFile = toml::from_str(text).with_context(|| format!("parsing {origin}"))?;
   Ok(Layer { name:        file.name,
              description: file.description,
              inherit:     file.inherit,
              zwift_play:  parse_section(origin, "zwift_play", file.zwift_play)?,
              gamepad:     parse_section(origin, "gamepad", file.gamepad)?, })
}

/// Apply `entries` onto `table`: each entry replaces (or with "none" removes)
/// one button. `label` is the section as the user wrote it, for messages.
fn apply(table: &mut Table, play: bool, origin: &str, label: &str, entries: &[(String, ActionSpec)]) -> Result<()>
{
   for (key, spec) in entries
   {
      let button: SourceButton = key.parse()
                                    .map_err(|e| anyhow::anyhow!("{origin}: [{label}] {e}"))?;
      if button.is_play() != play
      {
         let (kind, other) = if button.is_play() { ("Zwift Play", "zwift_play") } else { ("gamepad", "gamepad") };
         anyhow::bail!("{origin}: [{label}] '{key}' is a {kind} button; put it under [{other}]");
      }
      if spec.0.is_empty()
      {
         table.remove(&button);
      }
      else
      {
         table.insert(button, spec.0.clone());
      }
   }
   Ok(())
}

struct Resolved
{
   name:        String,
   description: String,
   source:      String,
   zwift_play:  Table,
   gamepad:     Table,
}

/// Identity used for cycle detection: names as given, paths canonicalised.
fn identity(arg: &str) -> String
{
   if is_path(arg)
   {
      std::fs::canonicalize(arg).map_or_else(|_| arg.to_string(), |p| p.display().to_string())
   }
   else
   {
      arg.to_string()
   }
}

fn resolve_chain(base: Option<&Path>, arg: &str, chain: &mut Vec<String>) -> Result<Resolved>
{
   // Lookups without a config directory only see embedded presets, so tag
   // them: a user `ride` inheriting the embedded `ride` is not a cycle.
   let id = if base.is_none() && !is_path(arg) { format!("embedded:{arg}") } else { identity(arg) };
   if chain.contains(&id)
   {
      chain.push(id);
      anyhow::bail!("mapping inherit cycle: {}", chain.join(" -> "));
   }
   if chain.len() >= MAX_INHERIT_DEPTH
   {
      anyhow::bail!("mapping inherit chain deeper than {MAX_INHERIT_DEPTH}: {} -> {id}", chain.join(" -> "));
   }
   chain.push(id);

   let (text, description) = CATALOG.resolve_in(base, arg)?;
   let origin = format!("mapping '{arg}' ({description})");
   let layer = parse_layer(&text, &origin)?;
   let mut resolved = match &layer.inherit
   {
      // A user file inheriting its own name extends the embedded preset of
      // that name (e.g. a user `ride.toml` with `inherit = "ride"`).
      | Some(parent) if base.is_some() && !is_path(parent) && parent == arg => resolve_chain(None, parent, chain)?,
      | Some(parent) => resolve_chain(base, parent, chain)?,
      | None => Resolved { name:        String::new(),
                           description: String::new(),
                           source:      String::new(),
                           zwift_play:  Table::new(),
                           gamepad:     Table::new(), },
   };
   apply(&mut resolved.zwift_play, true, &origin, "zwift_play", &layer.zwift_play)?;
   apply(&mut resolved.gamepad, false, &origin, "gamepad", &layer.gamepad)?;
   chain.pop();

   resolved.name = layer.name.unwrap_or_else(|| arg.to_string());
   resolved.description = layer.description.unwrap_or_default();
   resolved.source = match &layer.inherit
   {
      | Some(parent) => format!("{description}, inherits '{parent}'"),
      | None => description,
   };
   Ok(resolved)
}

pub struct Mapper
{
   zwift_play: Table,
   gamepad:    Table,
   /// The mapping's `name` (or the name/path it was loaded by).
   pub name:   String,
   /// Where it came from, for logs and `--doctor`.
   pub source: String,
}

impl Mapper
{
   /// Load mapping `arg` (a name or file path) and apply the `[mapping.*]`
   /// override tables from config.toml on top, per button.
   pub fn load(arg: &str, overrides: &MappingConfig) -> Result<Self> { Self::load_in(crate::platform::config_dir().as_deref(), arg, overrides) }

   fn load_in(base: Option<&Path>, arg: &str, overrides: &MappingConfig) -> Result<Self>
   {
      let mut resolved = resolve_chain(base, arg, &mut Vec::new())?;

      let sorted = |map: &BTreeMap<String, ActionSpec>| {
         map.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<(String, ActionSpec)>>()
      };
      apply(&mut resolved.zwift_play, true, "config.toml", "mapping.zwift_play", &sorted(&overrides.zwift_play))?;
      apply(&mut resolved.gamepad, false, "config.toml", "mapping.gamepad", &sorted(&overrides.gamepad))?;

      let mapper = Self { zwift_play: resolved.zwift_play,
                          gamepad:    resolved.gamepad,
                          name:       resolved.name,
                          source:     resolved.source, };
      info!(mapping = %mapper.name, source = %mapper.source, description = %resolved.description, buttons = mapper.bound_buttons(), "mapping loaded");
      Ok(mapper)
   }

   /// The actions bound to `button` on `source` (empty if unmapped).
   pub fn map(&self, source: Source, button: SourceButton) -> &[InGameAction]
   {
      let table = match source
      {
         | Source::ZwiftPlayLeft | Source::ZwiftPlayRight => &self.zwift_play,
         | Source::Gamepad(_) => &self.gamepad,
      };
      table.get(&button).map_or(&[], Vec::as_slice)
   }

   /// Number of buttons with at least one action.
   pub fn bound_buttons(&self) -> usize { self.zwift_play.len() + self.gamepad.len() }

   /// Distinct actions this mapping can produce, sorted by name.
   pub fn actions(&self) -> Vec<InGameAction>
   {
      let mut actions: Vec<InGameAction> = Vec::new();
      for action in self.zwift_play
                        .values()
                        .chain(self.gamepad.values())
                        .flatten()
      {
         if !actions.contains(action)
         {
            actions.push(*action);
         }
      }
      actions.sort_by_key(|action| action.name());
      actions
   }
}

#[cfg(test)]
mod tests
{
   use std::path::PathBuf;

   use super::*;
   use crate::profiles::test_dir;

   fn empty_base(tag: &str) -> PathBuf { test_dir(tag) }

   fn load(base: &Path, arg: &str, overrides: &MappingConfig) -> Result<Mapper> { Mapper::load_in(Some(base), arg, overrides) }

   fn write_map(base: &Path, name: &str, text: &str)
   {
      let dir = base.join("mappings");
      std::fs::create_dir_all(&dir).unwrap();
      std::fs::write(dir.join(format!("{name}.toml")), text).unwrap();
   }

   #[test]
   fn embedded_presets_load()
   {
      // Deliberately not a snapshot of every button: the presets are user
      // tunable. Check what each preset is for, and that all buttons are bound.
      let base = empty_base("embedded");
      let paddles = |mapper: &Mapper| {
         (mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight)
                .to_vec(),
          mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft)
                .to_vec())
      };
      for name in ["ride", "race", "workout"]
      {
         let mapper = load(&base, name, &MappingConfig::default()).unwrap_or_else(|e| panic!("{name}: {e:#}"));
         assert_eq!(mapper.name, name);
         assert_eq!(mapper.source, "embedded default", "{name}");
         assert_eq!(mapper.bound_buttons(), 30, "{name}: every Play and gamepad button bound");
         let expected = if name == "workout"
         {
            (vec![InGameAction::IncreaseResistance], vec![InGameAction::DecreaseResistance])
         }
         else
         {
            (vec![InGameAction::ShiftUp], vec![InGameAction::ShiftDown])
         };
         assert_eq!(paddles(&mapper), expected, "{name}: paddles");
      }
      let race = load(&base, "race", &MappingConfig::default()).unwrap();
      assert_eq!(race.map(Source::ZwiftPlayLeft, SourceButton::NavUp), &[InGameAction::FocusGroupAhead]);
      assert_eq!(race.map(Source::Gamepad(0), SourceButton::DPadRight), &[InGameAction::Home]);
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn every_embedded_mapping_parses()
   {
      for (name, text) in CATALOG.embedded
      {
         let layer = parse_layer(text, name).unwrap_or_else(|e| panic!("{name}: {e:#}"));
         assert_eq!(layer.name.as_deref(), Some(*name));
         assert!(layer.description.is_some(), "{name}");
         assert!(layer.inherit.is_none(), "{name}");
      }
   }

   #[test]
   fn defaults_map_paddles_to_shifts()
   {
      let base = empty_base("paddles");
      let mapper = load(&base, "ride", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::ShiftUp]);
      assert_eq!(mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft), &[InGameAction::ShiftDown]);
      assert!(mapper.map(Source::Gamepad(0), SourceButton::PaddleLeft)
                    .is_empty());
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn override_replaces_only_the_named_button()
   {
      let base = empty_base("override");
      let mut config = MappingConfig::default();
      config.zwift_play
            .insert("paddle_right".into(), ActionSpec(vec![InGameAction::UTurn]));
      config.zwift_play.insert("a".into(), ActionSpec(Vec::new()));
      config.gamepad
            .insert("start".into(), ActionSpec(vec![InGameAction::Menu, InGameAction::Pause]));
      let mapper = load(&base, "ride", &config).unwrap();
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::UTurn]);
      assert!(mapper.map(Source::ZwiftPlayRight, SourceButton::A)
                    .is_empty());
      assert_eq!(mapper.map(Source::Gamepad(1), SourceButton::Start), &[InGameAction::Menu, InGameAction::Pause]);
      // everything else is untouched
      assert_eq!(mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft), &[InGameAction::ShiftDown]);
      assert_eq!(mapper.bound_buttons(), 29);
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn override_errors_name_the_source_and_key()
   {
      let base = empty_base("override-errors");
      let mut config = MappingConfig::default();
      config.zwift_play
            .insert("bogus".into(), ActionSpec(vec![InGameAction::Pause]));
      let error = load(&base, "ride", &config).err().unwrap().to_string();
      assert!(error.contains("config.toml") && error.contains("mapping.zwift_play") && error.contains("bogus"), "{error}");

      let mut config = MappingConfig::default();
      config.gamepad
            .insert("paddle_left".into(), ActionSpec(vec![InGameAction::Pause]));
      let error = load(&base, "ride", &config).err().unwrap().to_string();
      assert!(error.contains("mapping.gamepad") && error.contains("paddle_left") && error.contains("zwift_play"), "{error}");
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn parses_string_array_and_none_values()
   {
      let base = empty_base("values");
      write_map(&base,
                "vals",
                "[zwift_play]\npaddle_right = \"shift_up\"\npaddle_left = [\"shift_down\", \"decrease_resistance\", \"shift_down\"]\non_off_left = \
                 \"none\"\n");
      let mapper = load(&base, "vals", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.name, "vals");
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::ShiftUp]);
      assert_eq!(mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft), &[InGameAction::ShiftDown,
                                                                                InGameAction::DecreaseResistance]);
      assert!(mapper.map(Source::ZwiftPlayLeft, SourceButton::OnOffLeft)
                    .is_empty());
      assert_eq!(mapper.bound_buttons(), 2);
      assert_eq!(mapper.actions(), vec![InGameAction::DecreaseResistance, InGameAction::ShiftDown, InGameAction::ShiftUp]);

      write_map(&base, "empty-list", "[zwift_play]\na = []\n");
      assert!(load(&base, "empty-list", &MappingConfig::default()).is_err());
      write_map(&base, "none-in-list", "[zwift_play]\na = [\"none\", \"pause\"]\n");
      assert!(load(&base, "none-in-list", &MappingConfig::default()).is_err());
      write_map(&base, "number", "[zwift_play]\na = 3\n");
      assert!(load(&base, "number", &MappingConfig::default()).is_err());
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn inherit_chain_overrides_and_removes()
   {
      let base = empty_base("inherit");
      write_map(&base, "mid", "inherit = \"ride\"\nname = \"mid-map\"\n[zwift_play]\npaddle_right = \"brake\"\non_off_left = \"none\"\n");
      write_map(&base, "top", "inherit = \"mid\"\n[zwift_play]\npaddle_left = [\"shift_down\", \"lap\"]\n[gamepad]\nstart = \"none\"\n");
      let mapper = load(&base, "top", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.name, "top");
      assert!(mapper.source.contains("inherits 'mid'"), "{}", mapper.source);
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::Brake]);
      assert_eq!(mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft), &[InGameAction::ShiftDown, InGameAction::Lap]);
      assert!(mapper.map(Source::ZwiftPlayLeft, SourceButton::OnOffLeft)
                    .is_empty());
      assert!(mapper.map(Source::Gamepad(0), SourceButton::Start)
                    .is_empty());
      // inherited from ride
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::A), &[InGameAction::CameraView]);
      assert_eq!(mapper.map(Source::Gamepad(0), SourceButton::RightBumper), &[InGameAction::ShiftUp]);
      assert_eq!(mapper.bound_buttons(), 30 - 2);

      // inherit by path
      let path = base.join("mappings").join("mid.toml");
      write_map(&base, "bypath", &format!("inherit = {:?}\n", path.display().to_string()));
      let mapper = load(&base, "bypath", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::Brake]);
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn inherit_cycle_and_depth_are_errors()
   {
      let base = empty_base("cycle");
      write_map(&base, "a", "inherit = \"b\"\n");
      write_map(&base, "b", "inherit = \"a\"\n");
      let error = load(&base, "a", &MappingConfig::default()).err()
                                                             .unwrap()
                                                             .to_string();
      assert!(error.contains("cycle") && error.contains("a -> b -> a"), "{error}");

      // Inheriting its own name means "the embedded preset of that name";
      // there is no embedded `self`, so this is a not-found error.
      write_map(&base, "self", "inherit = \"self\"\n");
      assert!(load(&base, "self", &MappingConfig::default()).err()
                                                            .unwrap()
                                                            .to_string()
                                                            .contains("not found"));

      // A user `ride` extending the embedded `ride` is not a cycle.
      write_map(&base, "ride", "inherit = \"ride\"\n[zwift_play]\npaddle_right = \"brake\"\n");
      let mapper = load(&base, "ride", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.map(Source::ZwiftPlayRight, SourceButton::PaddleRight), &[InGameAction::Brake]);
      assert_eq!(mapper.map(Source::ZwiftPlayLeft, SourceButton::PaddleLeft), &[InGameAction::ShiftDown]);

      for i in 0..10
      {
         write_map(&base, &format!("d{i}"), &format!("inherit = \"d{}\"\n", i + 1));
      }
      write_map(&base, "d10", "");
      let error = load(&base, "d0", &MappingConfig::default()).err()
                                                              .unwrap()
                                                              .to_string();
      assert!(error.contains("deeper"), "{error}");
      // a chain within the cap works
      assert!(load(&base, "d5", &MappingConfig::default()).is_ok());
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn file_errors_name_source_and_key()
   {
      let base = empty_base("errors");
      write_map(&base, "wrong-section", "[gamepad]\npaddle_left = \"pause\"\n");
      let error = load(&base, "wrong-section", &MappingConfig::default()).err()
                                                                         .unwrap()
                                                                         .to_string();
      assert!(error.contains("wrong-section") && error.contains("[gamepad]") && error.contains("paddle_left") && error.contains("[zwift_play]"),
              "{error}");

      write_map(&base, "wrong-section2", "[zwift_play]\nstart = \"pause\"\n");
      let error = load(&base, "wrong-section2", &MappingConfig::default()).err()
                                                                          .unwrap()
                                                                          .to_string();
      assert!(error.contains("start") && error.contains("[gamepad]"), "{error}");

      write_map(&base, "bad-button", "[zwift_play]\nbogus = \"pause\"\n");
      let error = load(&base, "bad-button", &MappingConfig::default()).err()
                                                                      .unwrap()
                                                                      .to_string();
      assert!(error.contains("bad-button") && error.contains("bogus"), "{error}");

      write_map(&base, "bad-action", "[zwift_play]\na = \"warp_speed\"\n");
      let error = load(&base, "bad-action", &MappingConfig::default()).err()
                                                                      .unwrap()
                                                                      .to_string();
      assert!(error.contains("bad-action") && error.contains("'a'") && error.contains("warp_speed"), "{error}");

      write_map(&base, "bad-key", "colour = \"red\"\n");
      assert!(load(&base, "bad-key", &MappingConfig::default()).is_err());

      let error = load(&base, "no-such-map", &MappingConfig::default()).err()
                                                                       .unwrap()
                                                                       .to_string();
      assert!(error.contains("no-such-map") && error.contains("ride, race, workout"), "{error}");
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn trigger_aliases_map_to_bumpers()
   {
      let base = empty_base("aliases");
      write_map(&base, "alias", "[gamepad]\nleft_trigger = \"lap\"\nright_trigger = \"pause\"\n");
      let mapper = load(&base, "alias", &MappingConfig::default()).unwrap();
      assert_eq!(mapper.map(Source::Gamepad(0), SourceButton::LeftBumper), &[InGameAction::Lap]);
      assert_eq!(mapper.map(Source::Gamepad(0), SourceButton::RightBumper), &[InGameAction::Pause]);
      let _ = std::fs::remove_dir_all(&base);
   }
}
