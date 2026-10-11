use super::*;

const ALL_PROFILES: &[(&str, &str, &str)] = &[("tpv", "keyboard/tpv.toml", include_str!("../../../keyboard/tpv.toml")),
                                              ("mywhoosh", "keyboard/mywhoosh.toml", include_str!("../../../keyboard/mywhoosh.toml")),
                                              ("biketerra", "keyboard/biketerra.toml", include_str!("../../../keyboard/biketerra.toml")),
                                              ("rouvy", "keyboard/rouvy.toml", include_str!("../../../keyboard/rouvy.toml"))];

fn chord(text: &str) -> KeyChord { text.parse().unwrap() }

fn binding(text: &str, mode: Mode) -> Binding { Binding { chord: chord(text), mode } }

fn key(key: KeyName, down: bool) -> Step
{
   Step { key: KeyLike::Key(key),
          down }
}

fn modifier(modifier: Modifier, down: bool) -> Step
{
   Step { key: KeyLike::Modifier(modifier),
          down }
}

#[test]
fn every_key_name_round_trips()
{
   for key in KeyName::ALL
   {
      assert_eq!(key.name().parse::<KeyName>().unwrap(), *key);
   }
   // names are unique
   let mut names: Vec<&str> = KeyName::ALL.iter().map(|k| k.name()).collect();
   names.sort();
   names.dedup();
   assert_eq!(names.len(), KeyName::ALL.len());
}

#[test]
fn digit_aliases()
{
   assert_eq!("0".parse::<KeyName>().unwrap(), KeyName::Digit0);
   assert_eq!("7".parse::<KeyName>().unwrap(), KeyName::Digit7);
   assert_eq!("digit_9".parse::<KeyName>().unwrap(), KeyName::Digit9);
}

#[test]
fn chord_parsing_good_cases()
{
   assert_eq!(chord("x"), KeyChord { modifiers: vec![],
                                     key:       KeyName::X, });
   assert_eq!(chord("  CTRL + Shift + X "), KeyChord { modifiers: vec![Modifier::Ctrl, Modifier::Shift],
                                                       key:       KeyName::X, });
   assert_eq!(chord("kp_plus").key, KeyName::KpPlus);
   assert_eq!(chord("alt+3").key, KeyName::Digit3);
   // modifiers may follow the key and are put in canonical order
   assert_eq!(chord("x+shift+ctrl").modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
}

#[test]
fn modifier_aliases()
{
   assert_eq!(chord("option+a").modifiers, vec![Modifier::Alt]);
   for alias in ["meta", "cmd", "win", "super"]
   {
      assert_eq!(chord(&format!("{alias}+a")).modifiers, vec![Modifier::Meta], "{alias}");
   }
}

#[test]
fn duplicate_modifiers_are_deduped()
{
   assert_eq!(chord("ctrl+ctrl+shift+ctrl+a").modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
   assert_eq!(chord("alt+option+a").modifiers, vec![Modifier::Alt]);
}

#[test]
fn chord_errors_name_the_token()
{
   let error = "a+b".parse::<KeyChord>().unwrap_err();
   assert!(matches!(error, ParseError::MultipleKeys { .. }));
   assert!(error.to_string().contains("'a'") && error.to_string().contains("'b'"), "{error}");

   let error = "ctrl+frobnicate".parse::<KeyChord>().unwrap_err();
   assert_eq!(error, ParseError::UnknownKey("frobnicate".into()));
   assert!(error.to_string().contains("frobnicate"));

   assert!(matches!("ctrl+shift".parse::<KeyChord>().unwrap_err(), ParseError::NoKey(_)));
   assert!(matches!("".parse::<KeyChord>().unwrap_err(), ParseError::EmptyToken(_)));
   assert!(matches!("ctrl++a".parse::<KeyChord>().unwrap_err(), ParseError::EmptyToken(_)));
}

#[test]
fn binding_string_table_and_bad_mode()
{
   let profile = Profile::parse(
                                r#"
app = "t"
[bindings]
shift_up = "kp_plus"
steer_left = { key = "left", mode = "hold" }
hud_toggle = { key = "ctrl+h", mode = "tap" }
lap = { key = "l" }
"#,
   ).unwrap();
   assert_eq!(profile.bindings[&InGameAction::ShiftUp], binding("kp_plus", Mode::Hold));
   assert_eq!(profile.bindings[&InGameAction::SteerLeft], binding("left", Mode::Hold));
   assert_eq!(profile.bindings[&InGameAction::HudToggle], binding("ctrl+h", Mode::Tap));
   assert_eq!(profile.bindings[&InGameAction::Lap], binding("l", Mode::Hold));

   let error = Profile::parse("app = \"t\"\n[bindings]\nlap = { key = \"l\", mode = \"toggle\" }\n").unwrap_err();
   assert!(error.to_string().contains("toggle"), "{error}");
   assert!(Profile::parse("app = \"t\"\n[bindings]\nlap = { mode = \"tap\" }\n").is_err());
   assert!(Profile::parse("app = \"t\"\n[bindings]\nlap = { key = \"l\", extra = 1 }\n").is_err());
   assert!(Profile::parse("app = \"t\"\n[bindings]\nlap = \"a+b\"\n").is_err());
   assert!(Profile::parse("app = \"t\"\n[bindings]\nlap = 5\n").is_err());
}

#[test]
fn unknown_action_is_an_error_naming_it()
{
   let error = Profile::parse("app = \"t\"\n[bindings]\nwarp_speed = \"w\"\n").unwrap_err();
   assert!(error.to_string().contains("warp_speed"), "{error}");
}

#[test]
fn profile_rejects_unknown_top_level_keys()
{
   assert!(Profile::parse("app = \"t\"\nextra = 1\n").is_err());
   assert!(Profile::parse("bindings = {}\n").is_err()); // app is required
}

#[test]
fn all_embedded_profiles_parse()
{
   for (app, file, text) in ALL_PROFILES
   {
      let profile = Profile::parse(text).unwrap_or_else(|e| panic!("{file}: {e}"));
      assert!(!profile.app.is_empty(), "{file}");
      if *app == "tpv"
      {
         assert_eq!(profile.bindings.len(), 28, "{file}");
      }
   }
}

#[test]
fn tpv_profiles_match_legacy_mapping()
{
   let common = [(InGameAction::ShiftUp, KeyName::KpPlus, Mode::Hold),
                 (InGameAction::ShiftDown, KeyName::KpMinus, Mode::Hold),
                 (InGameAction::SteerLeft, KeyName::Left, Mode::Hold),
                 (InGameAction::SteerRight, KeyName::Right, Mode::Hold),
                 (InGameAction::NavigateLeft, KeyName::Left, Mode::Hold),
                 (InGameAction::NavigateRight, KeyName::Right, Mode::Hold),
                 (InGameAction::NavigateUp, KeyName::Up, Mode::Hold),
                 (InGameAction::NavigateDown, KeyName::Down, Mode::Hold),
                 (InGameAction::HudToggle, KeyName::H, Mode::Hold),
                 (InGameAction::Brake, KeyName::S, Mode::Hold),
                 (InGameAction::CameraViewBehind, KeyName::Digit1, Mode::Tap),
                 (InGameAction::CameraViewFirstPerson, KeyName::Digit3, Mode::Tap),
                 (InGameAction::CameraViewFromFront, KeyName::Digit6, Mode::Tap),
                 (InGameAction::MapToggle, KeyName::G, Mode::Hold),
                 (InGameAction::UTurn, KeyName::U, Mode::Hold),
                 (InGameAction::PushToTalk, KeyName::T, Mode::Hold),
                 (InGameAction::Home, KeyName::Home, Mode::Tap),
                 (InGameAction::Screenshot, KeyName::F10, Mode::Tap),
                 (InGameAction::CameraViewHelicopter, KeyName::Digit8, Mode::Tap),
                 (InGameAction::CameraViewBirdseye, KeyName::Digit9, Mode::Tap),
                 (InGameAction::CruiseControl, KeyName::C, Mode::Tap),
                 (InGameAction::CameraView, KeyName::RightBracket, Mode::Tap)];
   let resistance = [(InGameAction::IncreaseResistance, KeyName::PageUp, Mode::Hold),
                     (InGameAction::DecreaseResistance, KeyName::PageDown, Mode::Hold),
                     (InGameAction::SkipInterval, KeyName::Tab, Mode::Hold),
                     (InGameAction::Pause, KeyName::Space, Mode::Hold)];
   for (app, file, text) in ALL_PROFILES.iter().filter(|(app, _, _)| *app == "tpv")
   {
      let profile = Profile::parse(text).unwrap();
      let mut expected = common.to_vec();
      expected.extend(resistance);
      // + the two Shift chords checked below
      assert_eq!(profile.bindings.len(), expected.len() + 2, "{file}");
      for (action, key) in [(InGameAction::FocusGroupAhead, KeyName::PageUp),
                            (InGameAction::FocusGroupBehind, KeyName::PageDown)]
      {
         let binding = &profile.bindings[&action];
         assert_eq!(binding.chord,
                    KeyChord { modifiers: vec![Modifier::Shift],
                               key },
                    "{app}/{file} {action:?}");
         assert_eq!(binding.mode, Mode::Tap, "{app}/{file} {action:?}");
      }
      for (action, key, mode) in expected
      {
         let binding = &profile.bindings[&action];
         assert_eq!(binding.chord, KeyChord { modifiers: vec![], key }, "{app}/{file} {action:?}");
         assert_eq!(binding.mode, mode, "{app}/{file} {action:?}");
      }
   }
}

#[test]
fn hold_sequencing()
{
   let b = binding("ctrl+shift+x", Mode::Hold);
   assert_eq!(b.steps(Edge::Pressed), vec![modifier(Modifier::Ctrl, true), modifier(Modifier::Shift, true), key(KeyName::X, true)]);
   assert_eq!(b.steps(Edge::Released), vec![key(KeyName::X, false), modifier(Modifier::Shift, false), modifier(Modifier::Ctrl, false)]);
}

#[test]
fn tap_sequencing()
{
   let b = binding("alt+f1", Mode::Tap);
   assert_eq!(b.steps(Edge::Pressed), vec![modifier(Modifier::Alt, true),
                                           key(KeyName::F1, true),
                                           key(KeyName::F1, false),
                                           modifier(Modifier::Alt, false)]);
   assert!(b.steps(Edge::Released).is_empty());
}

#[test]
fn plain_key_sequencing()
{
   let b = binding("h", Mode::Hold);
   assert_eq!(b.steps(Edge::Pressed), vec![key(KeyName::H, true)]);
   assert_eq!(b.steps(Edge::Released), vec![key(KeyName::H, false)]);
}

#[test]
fn profile_names_are_validated()
{
   assert!(CATALOG.check_name("tpv").is_ok());
   assert!(CATALOG.check_name("my-app_2").is_ok());
   assert!(CATALOG.check_name("").is_err());
   assert!(CATALOG.check_name("../etc/passwd").is_err());
   assert!(CATALOG.check_name("a/b").is_err());
}
