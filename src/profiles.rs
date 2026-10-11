//! Named TOML files that ship embedded in the binary and can be overridden or
//! extended by the user: keyboard profiles (`keyboard/<name>.toml`) and
//! controller mappings (`mappings/<name>.toml`) in the config directory.
//!
//! An argument that looks like a path (contains a path separator or ends in
//! `.toml`) names a file directly; anything else is a name, looked up as a
//! user file first and then among the embedded defaults.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct Catalog
{
   /// Human description used in messages, e.g. "keyboard profile".
   pub kind:     &'static str,
   /// Subdirectory of the config directory holding the user files.
   pub subdir:   &'static str,
   /// `(name, TOML text)` pairs compiled into the binary.
   pub embedded: &'static [(&'static str, &'static str)],
}

/// True if `arg` should be treated as a file path rather than a name.
pub fn is_path(arg: &str) -> bool { arg.contains('/') || arg.contains('\\') || arg.ends_with(".toml") }

impl Catalog
{
   /// Names of the embedded files.
   pub fn embedded_names(&self) -> Vec<&'static str> { self.embedded.iter().map(|(name, _)| *name).collect() }

   fn embedded_text(&self, name: &str) -> Option<&'static str>
   {
      self.embedded
          .iter()
          .find(|(n, _)| *n == name)
          .map(|(_, text)| *text)
   }

   /// Names become file names, so keep them simple.
   pub fn check_name(&self, name: &str) -> Result<()>
   {
      let ok = !name.is_empty()
               && name.chars()
                      .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
      if !ok
      {
         anyhow::bail!("invalid {} name '{name}' (use letters, digits, '-' and '_')", self.kind);
      }
      Ok(())
   }

   fn path_in(&self, base: &Path, name: &str) -> PathBuf { base.join(self.subdir).join(format!("{name}.toml")) }

   /// Location of the user file for `name` in the config directory.
   pub fn user_path(&self, name: &str) -> Option<PathBuf> { crate::platform::config_dir().map(|base| self.path_in(&base, name)) }

   /// User files currently present (sorted).
   pub fn user_files(&self) -> Vec<PathBuf> { self.user_files_in(crate::platform::config_dir().as_deref()) }

   fn user_files_in(&self, base: Option<&Path>) -> Vec<PathBuf>
   {
      let Some(base) = base
      else
      {
         return Vec::new();
      };
      let Ok(entries) = std::fs::read_dir(base.join(self.subdir))
      else
      {
         return Vec::new();
      };
      let mut files: Vec<PathBuf> = entries.filter_map(|e| e.ok())
                                           .map(|e| e.path())
                                           .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml"))
                                           .collect();
      files.sort();
      files
   }

   /// Resolve `arg` (name or file path) to `(TOML text, source description)`.
   pub fn resolve(&self, arg: &str) -> Result<(String, String)> { self.resolve_in(crate::platform::config_dir().as_deref(), arg) }

   /// `resolve` against an explicit config directory.
   pub fn resolve_in(&self, base: Option<&Path>, arg: &str) -> Result<(String, String)>
   {
      if is_path(arg)
      {
         let path = Path::new(arg);
         if !path.is_file()
         {
            anyhow::bail!("{} file '{arg}' not found", self.kind);
         }
         let text = std::fs::read_to_string(path).with_context(|| format!("reading {} {}", self.kind, path.display()))?;
         return Ok((text, format!("file {}", path.display())));
      }

      self.check_name(arg)?;
      let user_path = base.map(|base| self.path_in(base, arg));
      if let Some(path) = user_path.as_deref().filter(|p| p.is_file())
      {
         let text = std::fs::read_to_string(path).with_context(|| format!("reading {} {}", self.kind, path.display()))?;
         return Ok((text, format!("user file {}", path.display())));
      }
      if let Some(text) = self.embedded_text(arg)
      {
         return Ok((text.to_string(), "embedded default".to_string()));
      }

      let searched = user_path.as_deref()
                              .map_or("<no config directory>".to_string(), |p| p.display().to_string());
      let mut available: Vec<String> = self.embedded_names()
                                           .iter()
                                           .map(|name| name.to_string())
                                           .collect();
      for file in self.user_files_in(base)
      {
         if let Some(stem) = file.file_stem().and_then(|s| s.to_str())
         {
            if !available.iter().any(|name| name == stem)
            {
               available.push(stem.to_string());
            }
         }
      }
      let available = if available.is_empty() { "none".to_string() } else { available.join(", ") };
      anyhow::bail!("{} '{arg}' not found: looked for {searched}; available: {available}", self.kind)
   }

   /// Write the embedded default for `name` to the user config directory.
   pub fn dump(&self, name: &str, force: bool) -> Result<PathBuf> { self.dump_in(crate::platform::config_dir().as_deref(), name, force) }

   /// `dump` against an explicit config directory.
   pub fn dump_in(&self, base: Option<&Path>, name: &str, force: bool) -> Result<PathBuf>
   {
      self.check_name(name)?;
      let Some(text) = self.embedded_text(name)
      else
      {
         anyhow::bail!("no embedded {} '{name}' (available: {})", self.kind, self.embedded_names().join(", "));
      };
      let Some(base) = base
      else
      {
         anyhow::bail!("could not determine a config directory");
      };
      let path = self.path_in(base, name);
      if path.exists() && !force
      {
         anyhow::bail!("{} already exists; pass --force to overwrite", path.display());
      }
      if let Some(dir) = path.parent()
      {
         std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
      }
      std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
      Ok(path)
   }

   /// Print the embedded and user files (for `--list-keys` / `--list-maps`).
   pub fn print_list(&self)
   {
      let names = self.embedded_names();
      println!("embedded {}s:", self.kind);
      if names.is_empty()
      {
         println!("  (none)");
      }
      for name in names
      {
         println!("  {name}");
      }
      println!("user {}s:", self.kind);
      let files = self.user_files();
      if files.is_empty()
      {
         println!("  (none)");
      }
      for file in files
      {
         println!("  {}", file.display());
      }
   }
}

#[cfg(test)]
pub fn test_dir(tag: &str) -> PathBuf
{
   let dir = std::env::temp_dir().join(format!("bikestick-test-{}-{tag}", std::process::id()));
   let _ = std::fs::remove_dir_all(&dir);
   std::fs::create_dir_all(&dir).unwrap();
   dir
}

#[cfg(test)]
mod tests
{
   use super::*;

   const TEST: Catalog = Catalog { kind:     "widget",
                                   subdir:   "widgets",
                                   embedded: &[("alpha", "x = 1\n"), ("beta", "x = 2\n")], };

   #[test]
   fn path_or_name_detection()
   {
      assert!(is_path("./mine.toml"));
      assert!(is_path("mine.toml"));
      assert!(is_path("dir/mine"));
      assert!(is_path("C:\\maps\\mine"));
      assert!(!is_path("ride"));
      assert!(!is_path("my-map_2"));
   }

   #[test]
   fn names_are_validated()
   {
      assert!(TEST.check_name("my-app_2").is_ok());
      assert!(TEST.check_name("").is_err());
      assert!(TEST.check_name("../etc/passwd").is_err());
      assert!(TEST.check_name("a.b").is_err());
   }

   #[test]
   fn resolves_embedded_user_and_path()
   {
      let base = test_dir("resolve");
      let (text, source) = TEST.resolve_in(Some(&base), "alpha").unwrap();
      assert_eq!((text.as_str(), source.as_str()), ("x = 1\n", "embedded default"));

      std::fs::create_dir_all(base.join("widgets")).unwrap();
      std::fs::write(base.join("widgets/alpha.toml"), "x = 10\n").unwrap();
      std::fs::write(base.join("widgets/gamma.toml"), "x = 3\n").unwrap();
      let (text, source) = TEST.resolve_in(Some(&base), "alpha").unwrap();
      assert_eq!(text, "x = 10\n");
      assert!(source.starts_with("user file "), "{source}");
      assert_eq!(TEST.resolve_in(Some(&base), "gamma").unwrap().0, "x = 3\n");

      let file = base.join("widgets/gamma.toml");
      let (text, source) = TEST.resolve_in(Some(&base), file.to_str().unwrap())
                               .unwrap();
      assert_eq!(text, "x = 3\n");
      assert!(source.starts_with("file "), "{source}");

      let missing = base.join("nope.toml");
      let error = TEST.resolve_in(Some(&base), missing.to_str().unwrap())
                      .unwrap_err()
                      .to_string();
      assert!(error.contains("not found"), "{error}");
      // a name never silently falls back to a path or vice versa
      let error = TEST.resolve_in(Some(&base), "delta")
                      .unwrap_err()
                      .to_string();
      assert!(error.contains("alpha, beta, gamma"), "{error}");
      assert!(error.contains("delta.toml"), "{error}");
      let _ = std::fs::remove_dir_all(&base);
   }

   #[test]
   fn dump_refuses_overwrite_without_force()
   {
      let base = test_dir("dump");
      let path = TEST.dump_in(Some(&base), "beta", false).unwrap();
      assert_eq!(path, base.join("widgets").join("beta.toml"));
      assert_eq!(std::fs::read_to_string(&path).unwrap(), "x = 2\n");

      std::fs::write(&path, "edited\n").unwrap();
      let error = TEST.dump_in(Some(&base), "beta", false)
                      .unwrap_err()
                      .to_string();
      assert!(error.contains("--force"), "{error}");
      assert_eq!(std::fs::read_to_string(&path).unwrap(), "edited\n");

      TEST.dump_in(Some(&base), "beta", true).unwrap();
      assert_eq!(std::fs::read_to_string(&path).unwrap(), "x = 2\n");

      assert!(TEST.dump_in(Some(&base), "nope", false).is_err());
      assert!(TEST.dump_in(Some(&base), "../x", false).is_err());
      assert!(TEST.dump_in(None, "alpha", false).is_err());
      let _ = std::fs::remove_dir_all(&base);
   }
}
