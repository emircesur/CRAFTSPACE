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
//!
//! For labs and classrooms, also:
//!
//! ```json
//! {
//!   "organization": "Riverside School", "support": "https://it.riverside.example/help",
//!   "blocked_apps": ["effectcraft"],
//!   "pinned_versions": { "photocraft": "0.5.0" },
//!   "update_window": { "days": ["sat", "sun"], "from": "22:00", "to": "06:00" },
//!   "lock_settings": true, "prevent_uninstall": true,
//!   "report_dir": "\\\\server\\craftspace\\reports",
//!   "policy_url": "https://it.riverside.example/craftspace/policy.json",
//!   "settings": { "package_cache": "\\\\server\\craftspace\\packages", "package_cache_write": true },
//!   "profiles": { "photocraft": { "source": "\\\\server\\craftspace\\photocraft.craftprofile",
//!                                 "parts": ["layouts", "shortcuts"], "apply": "every-start" } }
//! }
//! ```

use std::collections::BTreeMap;
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
    /// Shown as "Managed by …" in the app.
    pub organization: Option<String>,
    /// Where to get help: a web address or an email address.
    pub support: Option<String>,
    /// Apps that are hidden and can't be installed.
    pub blocked_apps: Vec<String>,
    /// app id → version every computer stays on.
    pub pinned_versions: BTreeMap<String, String>,
    /// When automatic updates may install (manual updates are always allowed).
    pub update_window: Option<UpdateWindow>,
    /// Every setting is locked.
    pub lock_settings: bool,
    /// Apps can't be uninstalled, rolled back or moved to another channel (only by an
    /// administrator, from the command line).
    pub prevent_uninstall: bool,
    /// Only add-ons checked by CraftSpace may be installed (not plug-ins from other stores).
    pub block_unchecked_addons: bool,
    /// After each check, CraftSpace writes `<computer name>.json` with what's installed here.
    pub report_dir: Option<PathBuf>,
    /// A policy published centrally: fetched on each check and used from the next start (and
    /// right away by `craftspace-cli apply-policy`). Its values replace this file's.
    pub policy_url: Option<String>,
    /// app id → a workspace profile (layouts, shortcuts, preferences, presets) every computer
    /// gets, e.g. the teacher's setup.
    pub profiles: BTreeMap<String, ProfilePolicy>,
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
        self.lock_settings || self.settings.contains_key(key)
    }

    pub fn allows(&self, app_id: &str) -> bool {
        let blocked = self.blocked_apps.iter().any(|a| a.eq_ignore_ascii_case(app_id));
        !blocked && (self.allowed_apps.is_empty() || self.allowed_apps.iter().any(|a| a.eq_ignore_ascii_case(app_id)))
    }

    /// The version `app_id` is pinned to.
    pub fn pinned(&self, app_id: &str) -> Option<semver::Version> {
        let v = self.pinned_versions.iter().find(|(k, _)| k.eq_ignore_ascii_case(app_id))?.1;
        semver::Version::parse(v.trim_start_matches('v')).ok()
    }

    /// Whether an app's channel (and so its version) is the organization's to decide.
    pub fn locks_channel(&self, app_id: &str) -> bool {
        self.lock_settings || self.pinned(app_id).is_some() || self.settings.contains_key("channels")
    }

    /// Whether automatic updates may install now.
    pub fn may_update_now(&self) -> bool {
        self.update_window.as_ref().is_none_or(|w| {
            let (day, minutes) = local_now();
            w.contains(day, minutes)
        })
    }

    /// Lay a centrally published policy over this one: its values win.
    pub fn overlay(&mut self, remote: Policy) {
        let mut value = serde_json::to_value(&*self).expect("policy serializes");
        let remote_value = serde_json::to_value(&remote).expect("policy serializes");
        if let (Some(base), Some(over)) = (value.as_object_mut(), remote_value.as_object()) {
            let defaults = serde_json::to_value(Policy::default()).expect("policy serializes");
            for (k, v) in over {
                // Only what the remote policy actually sets.
                if defaults.get(k) != Some(v) {
                    if let (Some(serde_json::Value::Object(b)), serde_json::Value::Object(o)) = (base.get_mut(k), v) {
                        for (kk, vv) in o {
                            b.insert(kk.clone(), vv.clone());
                        }
                    } else {
                        base.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        let source = self.source.take();
        if let Ok(merged) = serde_json::from_value::<Policy>(value) {
            *self = merged;
        }
        self.source = source;
    }

    /// `settings` with the policy's values forced in.
    pub fn apply(&self, settings: &Settings) -> Settings {
        let mut settings = self.apply_settings(settings);
        for (id, v) in &self.pinned_versions {
            if let Ok(v) = semver::Version::parse(v.trim_start_matches('v')) {
                settings.channels.insert(id.to_ascii_lowercase(), crate::settings::Channel::Pinned(v));
            }
        }
        settings
    }

    fn apply_settings(&self, settings: &Settings) -> Settings {
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

/// A workspace profile the organization hands out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePolicy {
    /// A `.craftprofile` file: a path (a file share works) or an `https://` address.
    pub source: String,
    /// Its SHA-256, to check it wasn't changed.
    #[serde(default)]
    pub sha256: Option<String>,
    /// The parts to apply; empty means all.
    #[serde(default)]
    pub parts: Vec<crate::profiles::Part>,
    #[serde(default)]
    pub apply: ProfileApply,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileApply {
    /// When the profile is new or changed; students' own changes stay after that.
    #[default]
    Once,
    /// Each time CraftSpace starts (or `apply-policy` runs), so every class starts the same.
    EveryStart,
}

/// When automatic updates may install: on these days, between these times (local time). A
/// window past midnight (`22:00`–`06:00`) runs into the next morning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateWindow {
    /// `mon` … `sun`; empty means every day.
    #[serde(default)]
    pub days: Vec<String>,
    /// `HH:MM`.
    pub from: String,
    pub to: String,
}

const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl UpdateWindow {
    fn day_listed(&self, day: usize) -> bool {
        self.days.is_empty() || self.days.iter().any(|d| d.to_ascii_lowercase().starts_with(DAYS[day]))
    }

    /// `day`: 0 = Monday. `now`: minutes since midnight.
    pub fn contains(&self, day: usize, now: u32) -> bool {
        let (Some(from), Some(to)) = (minutes(&self.from), minutes(&self.to)) else { return true };
        if from <= to {
            self.day_listed(day) && (from..to).contains(&now)
        } else {
            // Past midnight: the evening of a listed day, or the morning after one.
            (self.day_listed(day) && now >= from) || (self.day_listed((day + 6) % 7) && now < to)
        }
    }

    pub fn describe(&self) -> String {
        let days = if self.days.is_empty() { "every day".to_string() } else { self.days.join(", ") };
        format!("{days}, {}–{}", self.from, self.to)
    }
}

/// The local weekday (0 = Monday) and minutes since midnight.
pub fn local_now() -> (usize, u32) {
    #[cfg(unix)]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as libc::time_t)
            .unwrap_or(0);
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
            let day = ((tm.tm_wday + 6) % 7) as usize;
            return (day, (tm.tm_hour * 60 + tm.tm_min) as u32);
        }
    }
    #[cfg(windows)]
    {
        let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st) };
        let day = ((st.wDayOfWeek as usize) + 6) % 7;
        return (day, u32::from(st.wHour) * 60 + u32::from(st.wMinute));
    }
    #[allow(unreachable_code)]
    {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        // 1970-01-01 was a Thursday.
        (((secs / 86_400 + 3) % 7) as usize, ((secs % 86_400) / 60) as u32)
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

        // Blocked apps, pinned versions and locked settings.
        let lab: Policy = serde_json::from_str(
            r#"{"blocked_apps": ["effectcraft"], "pinned_versions": {"photocraft": "v0.5.0"}, "lock_settings": true, "organization": "Riverside"}"#,
        )
        .unwrap();
        assert!(!lab.allows("EffectCraft") && lab.allows("photocraft"));
        assert_eq!(lab.pinned("PhotoCraft"), Some(semver::Version::new(0, 5, 0)));
        assert!(lab.locks("theme") && lab.locks_channel("gridcraft"));
        let s = lab.apply(&Settings::default());
        assert_eq!(s.channel("photocraft"), crate::settings::Channel::Pinned(semver::Version::new(0, 5, 0)));

        // A wrongly typed value is ignored rather than breaking the app.
        let bad: Policy = serde_json::from_str(r#"{"settings": {"check_interval_hours": "soon"}}"#).unwrap();
        assert_eq!(bad.apply(&Settings::default()), Settings::default());
    }

    #[test]
    fn update_windows() {
        let night = UpdateWindow { days: vec!["sat".into(), "sun".into()], from: "22:00".into(), to: "06:00".into() };
        let (sat, sun, mon, fri) = (5, 6, 0, 4);
        assert!(night.contains(sat, 23 * 60));
        assert!(night.contains(sun, 3 * 60), "Saturday night into Sunday morning");
        assert!(night.contains(mon, 5 * 60), "Sunday night into Monday morning");
        assert!(!night.contains(mon, 7 * 60));
        assert!(!night.contains(fri, 23 * 60));
        let lunch = UpdateWindow { days: vec![], from: "12:00".into(), to: "13:30".into() };
        assert!(lunch.contains(fri, 12 * 60 + 15) && !lunch.contains(fri, 14 * 60));
        let (day, minutes) = local_now();
        assert!(day < 7 && minutes < 24 * 60);
    }

    #[test]
    fn a_remote_policy_overrides_what_it_sets() {
        let mut local: Policy = serde_json::from_str(
            r#"{"organization": "Local", "required_apps": ["photocraft"], "policy_url": "https://x/p.json", "settings": {"theme": "light"}}"#,
        )
        .unwrap();
        local.source = Some(PathBuf::from("/etc/craftspace/policy.json"));
        let remote: Policy =
            serde_json::from_str(r#"{"organization": "Central", "settings": {"auto_install_updates": true}}"#).unwrap();
        local.overlay(remote);
        assert_eq!(local.organization.as_deref(), Some("Central"));
        assert_eq!(local.required_apps, ["photocraft"], "kept where the remote policy says nothing");
        assert_eq!(local.settings.len(), 2, "settings are merged");
        assert!(local.source.is_some());
    }
}
