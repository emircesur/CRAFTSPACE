//! The engine behind CraftSpace, the installer and update manager for ArtCraft creative apps.
//!
//! Start with [`manager::Manager`]: it ties together the app [`catalog`], release discovery on
//! [`github`], [`download`]s with checksum verification, unpacking ([`archive`]), desktop
//! [`integrate`]ion and the record of what is installed ([`state`]).

pub mod addons;
pub mod app_data;
pub mod archive;
pub mod catalog;
pub mod detect;
pub mod download;
pub mod file_types;
pub mod files;
pub mod github;
pub mod http;
pub mod integrate;
pub mod manager;
pub mod paths;
pub mod platform;
pub mod profiles;
pub mod selfupdate;
pub mod settings;
pub mod sources;
pub mod state;
pub mod version;

pub use catalog::{AppEntry, Catalog};
pub use download::{Progress, ProgressEvent, Stage};
pub use manager::{AppState, Manager, Plan};
pub use settings::Settings;
pub mod autostart;
pub mod fonts;
pub mod icons;
pub mod news;
pub mod policy;
pub mod running;
pub mod thumbs;
pub mod tour;
pub mod zsync;
