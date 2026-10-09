//! Machine-wide policy for IT departments and classrooms.
//!
//! A JSON file that administrators deploy:
//!
//! | | |
//! |---|---|
//! | Windows | `%ProgramData%\CraftSpace\policy.json` |
//! | Linux | `/etc/craftspace/policy.json` |
//! | macOS | `/Library/Application Support/CraftSpace/policy.json` |
//!
//! (`CRAFTSPACE_POLICY` points somewhere else.) Example:
//!
//! ```json
//! {
//!   "settings": { "auto_install_updates": true, "include_prereleases": false },
//!   "required_apps": ["photocraft", "pdfcraft"],
//!   "allowed_apps": ["photocraft", "pdfcraft", "gridcraft"],
//!   "disable_self_update": true,
//!   "quiet": true
//! }
//! ```
//!
//! `settings` values override the user's and can't be changed in the app. `required_apps` are
//! installed by `craftspace-cli apply-policy` (and by the app on start). `allowed_apps` hides
//! everything else.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Policy {
    /// Settings fields (as in `settings.json`) that are forced and locked.
    pub settings: serde_json::Map<String, serde_json::Value>,
    pub required_apps: Vec<String>,
    /// When non-empty, only these apps are shown and installable.
    pub allowed_apps: Vec<String>,
    pub disable_self_update: bool,
    /// Installers run without any UI (MSI `/qn`).
    pub quiet: bool,
    /// Where the policy was read from.
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

impl Policy {
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("CRAFTSPACE_POLICY").filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
        if cfg!(windows) {
            let base =
                std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
            base.join("CraftSpace").join("policy.json")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/Library/Application Support/CraftSpace/policy.json")
        } else {
            PathBuf::from("/etc/craftspace/policy.json")
        }
    }

    /// The deployed policy, or an empty one. A broken file is reported and ignored.
    pub fn load() -> Policy {
        let path = Policy::default_path();
        let Ok(bytes) = std::fs::read(&path) else { return Policy::default() };
        match serde_json::from_slice::<Policy>(&bytes) {
            Ok(mut p) => {
                p.source = Some(path);
                p
            }
            Err(err) => {
                log::error!("ignoring the policy in {}: {err}", path.display());
                Policy::default()
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.source.is_some()
    }

    /// Whether the user may change `key` (a `Settings` field name).
    pub fn locks(&self, key: &str) -> bool {
        self.settings.contains_key(key)
    }

    pub fn allows(&self, app_id: &str) -> bool {
        self.allowed_apps.is_empty() || self.allowed_apps.iter().any(|a| a.eq_ignore_ascii_case(app_id))
    }

    /// `settings` with the policy's values forced in.
    pub fn apply(&self, settings: &Settings) -> Settings {
        if self.settings.is_empty() {
            return settings.clone();
        }
        let mut value = serde_json::to_value(settings).expect("settings serialize");
        if let Some(obj) = value.as_object_mut() {
            for (k, v) in &self.settings {
                obj.insert(k.clone(), v.clone());
            }
        }
        serde_json::from_value(value).unwrap_or_else(|err| {
            log::error!("the policy's settings don't fit ({err}); ignoring them");
            settings.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forces_and_locks_settings() {
        let policy: Policy = serde_json::from_str(
            r#"{"settings": {"auto_install_updates": true, "check_interval_hours": 2},
                "allowed_apps": ["photocraft"], "required_apps": ["photocraft"]}"#,
        )
        .unwrap();
        let s = policy.apply(&Settings::default());
        assert!(s.auto_install_updates);
        assert_eq!(s.check_interval_hours, 2);
        assert!(policy.locks("auto_install_updates"));
        assert!(!policy.locks("theme"));
        assert!(policy.allows("PhotoCraft"));
        assert!(!policy.allows("gridcraft"));
        assert!(Policy::default().allows("gridcraft"));

        // A wrongly typed value is ignored rather than breaking the app.
        let bad: Policy = serde_json::from_str(r#"{"settings": {"check_interval_hours": "soon"}}"#).unwrap();
        assert_eq!(bad.apply(&Settings::default()), Settings::default());
    }
}
