//! Where each ArtCraft app keeps its settings, read from the apps' sources: which files, which
//! keys are layouts or shortcuts, and which values belong to the computer (recent files, window
//! positions, folder paths, GPU, memory and audio devices) and never travel.
//!
//! Left out on purpose: PdfCraft (its one settings file holds signatures and digital IDs with
//! everything else), CADCraft (it saves no settings), and ArtCraft's sign-ins and API keys
//! (`~/Artcraft/credentials`), which no spec names.

use super::{AppSpec, Base, ItemSpec, Part, RootSpec, What};

/// A per-user config folder named after the app (`%APPDATA%\Name`, `~/Library/Application
/// Support/Name`, `~/.config/name`).
const fn config(name: &'static str, lower: &'static str, env: Option<&'static str>) -> RootSpec {
    RootSpec { key: "config", base: Base::Config, names: [name, name, lower], env, portable: None, setting: None }
}

const fn json(
    root: &'static str,
    path: &'static str,
    default: Part,
    keys: &'static [(Part, &'static [&'static str])],
    machine: &'static [&'static str],
) -> ItemSpec {
    ItemSpec { root, path, what: What::Json { default, keys, machine } }
}

const fn dir(root: &'static str, path: &'static str, part: Part) -> ItemSpec {
    ItemSpec { root, path, what: What::Dir(part) }
}

const fn file(root: &'static str, path: &'static str, part: Part) -> ItemSpec {
    ItemSpec { root, path, what: What::File(part) }
}

use Part::{Layouts, Preferences, Presets, Shortcuts};

pub(super) static SPECS: &[AppSpec] = &[
    AppSpec {
        app: "photocraft",
        roots: &[RootSpec {
            key: "config",
            base: Base::Config,
            names: ["Photocraft", "Photocraft", "photocraft"],
            env: Some("PHOTOCRAFT_CONFIG_DIR"),
            portable: Some("PhotoCraftData"),
            setting: None,
        }],
        items: &[
            json(
                "config",
                "preferences.json",
                Preferences,
                &[
                    (Layouts, &["workspaces", "workspaceLocked", "panelLayout", "menus", "toolbar"]),
                    (Shortcuts, &["shortcuts"]),
                    (Presets, &["presets"]),
                ],
                &[
                    "/fileHandling/recentFiles",
                    "/scratchDisks/disks",
                    "/plugIns/additionalPluginsFolder",
                    "/plugIns/useAdditionalPluginsFolder",
                    "/historyLog/filePath",
                    "/performance/memoryUsageMb",
                    "/performance/cacheLevels",
                    "/performance/cacheTileSize",
                    "/performance/useGpu",
                    "/performance/renderingMode",
                    "/performance/gpuBackend",
                    "/performance/linuxDisplayServer",
                    "/performance/effectCacheMb",
                    "/performance/legacyCompositing",
                    "/colorSettings/monitorProfile",
                    "/scriptEvents/bindings/*/script",
                ],
            ),
            // Brushes and actions.
            dir("config", "Presets", Presets),
        ],
        note: None,
    },
    AppSpec {
        app: "lightcraft",
        roots: &[
            config("LightCraft", "lightcraft", None),
            RootSpec {
                key: "library",
                base: Base::Home,
                names: ["Pictures/LightCraft Library", "Pictures/LightCraft Library", "Pictures/LightCraft Library"],
                env: Some("LIGHTCRAFT_LIBRARY"),
                portable: None,
                setting: Some(("config", "ui.json", "/settings/libraryPath")),
            },
        ],
        items: &[
            json(
                "config",
                "ui.json",
                Preferences,
                &[
                    (
                        Layouts,
                        &[
                            "leftPanel",
                            "right",
                            "leftWidth",
                            "rightWidth",
                            "presets",
                            "presetThumbs",
                            "filmstrip",
                            "openSections",
                            "openFlyouts",
                            "singlePanel",
                            "collapsedSidebar",
                            "filterBar",
                            "navigator",
                            "secondWindow",
                            "histogram",
                            "infoOverlay",
                        ],
                    ),
                    (Shortcuts, &["/settings/keymap"]),
                ],
                &[
                    "/settings/libraryPath",
                    "/settings/externalEditor",
                    "/settings/gpu",
                    "/settings/memoryMb",
                    "/settings/previewLimit",
                    "/localRoots",
                    "/hiddenLocations",
                    "/search",
                    "/renamingComponent",
                    "/pan",
                    "/zoom",
                    "/fullscreen",
                    "/view",
                    "/tool",
                ],
            ),
            dir("config", "camera-profiles", Presets),
            // Develop presets and favourites.
            file("library", "presets.json", Presets),
            // Export, metadata, filter, curve and keyword presets.
            json(
                "library",
                "prefs.json",
                Presets,
                &[],
                &[
                    "/lastExport",
                    "/import/autoFolder",
                    "/smartPreviewsDir",
                    "/lutProfiles",
                    "/exportPresets/*/params/dir",
                    "/cacheMb",
                    "/recentKeywords",
                ],
            ),
        ],
        note: None,
    },
    AppSpec {
        app: "vectorcraft",
        roots: &[config("VectorCraft", "vectorcraft", None)],
        items: &[
            json(
                "config",
                "ui.json",
                Preferences,
                &[
                    (
                        Layouts,
                        &[
                            "dock_tab",
                            "dock_collapsed",
                            "control_bar",
                            "toolbar",
                            "toolbar_double",
                            "toolbar_advanced",
                            "task_bar",
                            "slot_tool",
                            "group_tool",
                            "floating_flyouts",
                            "floating_panels",
                            "toolbar_pos",
                            "status_bar",
                            "dock",
                            "workspace",
                            "custom_workspaces",
                            "layers_panel",
                        ],
                    ),
                    (Shortcuts, &["shortcut_overrides", "shortcut_set"]),
                    (Presets, &["action_sets"]),
                ],
                &[
                    "/window",
                    "/recent_files",
                    "/open_panel",
                    "/dialog",
                    "/flyout",
                    "/status",
                    "/palette_open",
                    "/palette_query",
                    "/about",
                    "/engine_prefs/fontsFolder",
                    "/engine_prefs/pluginsFolder",
                    "/engine_prefs/scratchPrimary",
                    "/engine_prefs/scratchSecondary",
                    "/engine_prefs/recoveryFolder",
                    "/engine_prefs/templatesFolder",
                    "/engine_prefs/gpuPerformance",
                    "/engine_prefs/gpuPreference",
                    "/engine_prefs/renderThreads",
                ],
            ),
            dir("config", "Swatches", Presets),
            dir("config", "Graphic Styles", Presets),
            dir("config", "Libraries", Presets),
        ],
        note: None,
    },
    AppSpec {
        app: "designcraft",
        roots: &[config("DesignCraft", "designcraft", None)],
        items: &[
            json(
                "config",
                "ui.json",
                Preferences,
                &[
                    (
                        Layouts,
                        &[
                            "controlBar",
                            "toolsDoubleColumn",
                            "dockTab",
                            "openPanel",
                            "floating",
                            "dockExpanded",
                            "workspace",
                            "customWorkspaces",
                            "taskBar",
                            "hiddenPanels",
                            "hiddenMenuItems",
                            "showFullMenus",
                        ],
                    ),
                    (Shortcuts, &["shortcuts"]),
                    (Presets, &["scripts"]),
                ],
                &["/about", "/pendingUrls"],
            ),
            json("config", "prefs.json", Preferences, &[], &[]),
        ],
        note: None,
    },
    AppSpec {
        app: "wordcraft",
        roots: &[config("WordCraft", "wordcraft", None)],
        items: &[json(
            "config",
            "ui.json",
            Preferences,
            &[(Layouts, &["tab", "ribbonCollapsed", "navTab"])],
            &["/recent", "/window", "/backstage", "/backstagePage"],
        )],
        note: Some("WordCraft has no custom keyboard shortcuts yet."),
    },
    AppSpec {
        app: "deckcraft",
        roots: &[config("DeckCraft", "deckcraft", None)],
        items: &[
            json(
                "config",
                "ui.json",
                Preferences,
                &[(
                    Layouts,
                    &[
                        "tab",
                        "ribbonCollapsed",
                        "view",
                        "notes",
                        "notesHeight",
                        "thumbsWidth",
                        "pane",
                        "paneWidth",
                        "formatTab",
                    ],
                )],
                &["/recent", "/zoom", "/sorterZoom"],
            ),
            json("config", "prefs.json", Preferences, &[], &[]),
        ],
        note: Some("DeckCraft has no custom keyboard shortcuts yet."),
    },
    AppSpec {
        app: "gridcraft",
        roots: &[config("GridCraft", "gridcraft", None)],
        items: &[
            json(
                "config",
                "ui.json",
                Preferences,
                &[(Layouts, &["ribbonTab", "ribbonCollapsed", "formulaBar", "formulaBarExpanded", "statusBar"])],
                &["/recent"],
            ),
            // With the user dictionary.
            json("config", "prefs.json", Preferences, &[], &[]),
        ],
        note: Some("GridCraft has no custom keyboard shortcuts yet."),
    },
    AppSpec {
        app: "filmcraft",
        roots: &[RootSpec {
            key: "data",
            base: Base::Data,
            names: ["FilmCraft", "FilmCraft", "filmcraft"],
            env: Some("FILMCRAFT_DATA_DIR"),
            portable: Some("data"),
            setting: None,
        }],
        items: &[
            json(
                "data",
                "preferences.json",
                Preferences,
                &[],
                &[
                    "/general/recentProjects",
                    "/audioHardware",
                    "/voiceOver/source",
                    "/voiceOver/inputChannel",
                    "/mediaCache/location",
                    "/mediaCache/databaseLocation",
                    "/mediaBrowser/favorites",
                    "/mediaBrowser/recent",
                    "/mediaBrowser/lastDir",
                    "/memory/ramReservedGb",
                    "/memory/frameCacheMb",
                    "/playback/hardwareDecoding",
                    "/media/hardwareDecoding",
                    "/media/proresHardwareEncoding",
                    "/audio/multithreadedWaveforms",
                    "/color/displayColorManagement",
                    "/color/extendedDynamicRange",
                ],
            ),
            file("data", "workspaces.json", Layouts),
            dir("data", "Keyboard Shortcuts", Shortcuts),
            file("data", "effect-presets.json", Presets),
            file("data", "export-presets.json", Presets),
            dir("data", "Templates", Presets),
            dir("data", "Graphics Templates", Presets),
        ],
        note: None,
    },
    AppSpec {
        app: "effectcraft",
        roots: &[
            config("EffectCraft", "effectcraft", Some("EFFECTCRAFT_CONFIG_DIR")),
            RootSpec {
                key: "documents",
                base: Base::Home,
                names: ["Documents/EffectCraft", "Documents/EffectCraft", "Documents/EffectCraft"],
                env: None,
                portable: None,
                setting: None,
            },
        ],
        items: &[
            json(
                "config",
                "prefs.json",
                Preferences,
                &[],
                &[
                    "/recentProjects",
                    "/recentFootage",
                    "/recentPresets",
                    "/recentFonts",
                    "/audio/outputDevice",
                    "/audio/outputLeft",
                    "/audio/outputRight",
                    "/video/device",
                    "/video/enableOutput",
                    "/project/templatePath",
                    "/autoSave/folder",
                    "/autoSave/location",
                    "/export/defaultOutputFolder",
                    "/disk/diskCacheFolder",
                    "/disk/mediaCacheFolder",
                    "/disk/conformedMediaFolder",
                    "/disk/diskCacheMaxGb",
                    "/previews/displayProfile",
                    "/startup/windowGraphics",
                    "/memory",
                    "/composition/hardwareAcceleratePanels",
                    "/customRgb/icc",
                ],
            ),
            file("config", "shortcuts.json", Shortcuts),
            file("config", "script_settings.json", Presets),
            file("config", "scriptui_panels.json", Layouts),
            dir("config", "Scripts", Presets),
            dir("config", "Plug-ins", Presets),
            dir("config", "Templates", Presets),
            // Animation presets.
            dir("documents", "Presets", Presets),
        ],
        note: Some("EffectCraft doesn't save workspace layouts between launches yet, so only its open ScriptUI panels come over."),
    },
    AppSpec {
        app: "soundcraft",
        roots: &[config("SoundCraft", "soundcraft", None)],
        items: &[
            json(
                "config",
                "ui.json",
                Layouts,
                &[(Preferences, &["video_burn_in"])],
                &[
                    "/workspace_dir",
                    "/plugin_windows",
                    "/status",
                    "/show_about",
                    "/audiosuite",
                    "/configurations/*/1/workspace_dir",
                    "/configurations/*/1/plugin_windows",
                    "/configurations/*/1/status",
                    "/configurations/*/1/audiosuite",
                ],
            ),
            // Plug-in presets.
            dir("config", "Presets", Presets),
        ],
        note: Some("SoundCraft has no custom keyboard shortcuts yet."),
    },
    AppSpec {
        app: "artcraft",
        roots: &[RootSpec {
            key: "home",
            base: Base::Home,
            names: ["Artcraft", "Artcraft", "Artcraft"],
            env: None,
            portable: None,
            setting: None,
        }],
        items: &[
            json("home", "settings/app_preferences.json", Preferences, &[], &["/preferred_download_directory"]),
            file("home", "settings/provider_preferences.json", Preferences),
        ],
        note: Some("Most of ArtCraft's settings live inside its window and can't be carried over yet. Sign-ins and API keys are never included."),
    },
];
