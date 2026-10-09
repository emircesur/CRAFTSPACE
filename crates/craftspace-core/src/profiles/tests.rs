use super::*;

const ROOTS_A: &[RootSpec] = &[RootSpec {
    key: "config",
    base: Base::Config,
    names: ["TestCraft", "TestCraft", "testcraft"],
    env: Some("CRAFTSPACE_TEST_PROFILE_A"),
    portable: Some("TestCraftData"),
    setting: None,
}];

const ITEMS: &[ItemSpec] = &[
    ItemSpec {
        root: "config",
        path: "preferences.json",
        what: What::Json {
            default: Part::Preferences,
            keys: &[(Part::Layouts, &["workspaces", "panelLayout"]), (Part::Shortcuts, &["shortcuts"])],
            machine: &["/fileHandling/recentFiles", "/performance/gpuBackend", "/window"],
        },
    },
    ItemSpec { root: "config", path: "Presets", what: What::Dir(Part::Presets) },
    ItemSpec { root: "config", path: "notes.txt", what: What::File(Part::Preferences) },
];

const SPEC_A: AppSpec = AppSpec { app: "testcraft", roots: ROOTS_A, items: ITEMS, note: None };

fn ctx() -> Context {
    Context { name: "TestCraft".into(), ..Default::default() }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn profiles_carry_settings_but_not_the_computers_own() {
    let tmp = tempfile::tempdir().unwrap();
    let (teacher, student) = (tmp.path().join("teacher"), tmp.path().join("student"));
    write(
        &teacher.join("preferences.json"),
        r#"{"workspaces":{"Painting":[1,2]},"panelLayout":"wide","shortcuts":{"file.new":"Ctrl+Alt+N"},
            "general":{"theme":"dark"},"fileHandling":{"recentFiles":["/home/t/secret.psd"],"maxRecent":20},
            "performance":{"gpuBackend":"vulkan","history":50},"window":[0,0,800,600]}"#,
    );
    write(&teacher.join("Presets/brushes-1.pcbrushes"), "{}");
    write(&teacher.join("Presets/tips/a.pctip"), "tip");
    write(&teacher.join("Recovery/doc.json"), "not part of a profile");
    let profile = tmp.path().join("class.craftprofile");

    // Exporting.
    std::env::set_var("CRAFTSPACE_TEST_PROFILE_A", &teacher);
    let report = export(&SPEC_A, &ctx(), &Part::ALL, Some("1.2.0".into()), &profile).unwrap();
    assert_eq!(report.files, 3);
    let manifest = read_manifest(&profile).unwrap();
    assert_eq!(manifest.app, "testcraft");
    assert_eq!(manifest.parts, Part::ALL);
    assert!(manifest.files.iter().all(|f| !f.path.starts_with("Recovery")));
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&profile).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("files/config/preferences.json").unwrap().read_to_string(&mut text).unwrap();
    assert!(!text.contains("secret.psd") && !text.contains("vulkan") && !text.contains("800"), "{text}");
    assert!(text.contains("maxRecent") && text.contains("history"));

    // Importing only layouts and shortcuts on another computer keeps its own preferences and
    // machine values.
    write(
        &student.join("preferences.json"),
        r#"{"panelLayout":"narrow","general":{"theme":"light"},"fileHandling":{"recentFiles":["/home/s/mine.psd"]},
            "performance":{"gpuBackend":"dx12","history":10},"window":[5,5,100,100]}"#,
    );
    std::env::set_var("CRAFTSPACE_TEST_PROFILE_A", &student);
    let report = import(&SPEC_A, &ctx(), &profile, &[Part::Layouts, Part::Shortcuts]).unwrap();
    assert_eq!(report.changed.len(), 1);
    let prefs = read_json(&student.join("preferences.json"));
    assert_eq!(prefs["panelLayout"], "wide");
    assert_eq!(prefs["workspaces"]["Painting"], serde_json::json!([1, 2]));
    assert_eq!(prefs["shortcuts"]["file.new"], "Ctrl+Alt+N");
    assert_eq!(prefs["general"]["theme"], "light");
    assert!(!student.join("Presets").exists());

    // Everything: preferences come over, this computer's values stay.
    import(&SPEC_A, &ctx(), &profile, &Part::ALL).unwrap();
    let prefs = read_json(&student.join("preferences.json"));
    assert_eq!(prefs["general"]["theme"], "dark");
    assert_eq!(prefs["fileHandling"]["recentFiles"], serde_json::json!(["/home/s/mine.psd"]));
    assert_eq!(prefs["fileHandling"]["maxRecent"], 20);
    assert_eq!(prefs["performance"]["gpuBackend"], "dx12");
    assert_eq!(prefs["performance"]["history"], 50);
    assert_eq!(prefs["window"], serde_json::json!([5, 5, 100, 100]));
    assert_eq!(std::fs::read_to_string(student.join("Presets/tips/a.pctip")).unwrap(), "tip");
    std::env::remove_var("CRAFTSPACE_TEST_PROFILE_A");
}

#[test]
fn portable_copies_keep_settings_beside_the_program() {
    let tmp = tempfile::tempdir().unwrap();
    let program = tmp.path().join("usb/TestCraft/testcraft.exe");
    write(&program, "");
    let mut context = ctx();
    context.programs = vec![program.clone()];
    let root = &ROOTS_A[0];
    // Not portable: the per-user folder.
    assert_ne!(
        root_dir(&RootSpec { env: None, ..*root }, &context).unwrap(),
        tmp.path().join("usb/TestCraft/TestCraftData")
    );
    write(&program.with_file_name("portable.txt"), "");
    assert_eq!(
        root_dir(&RootSpec { env: None, ..*root }, &context).unwrap(),
        tmp.path().join("usb/TestCraft/TestCraftData")
    );
}

#[test]
fn profiles_for_another_app_or_with_unsafe_names_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("cfg");
    write(&dir.join("notes.txt"), "hello");
    std::env::set_var("CRAFTSPACE_TEST_PROFILE_B", &dir);
    let roots: &'static [RootSpec] =
        Box::leak(Box::new([RootSpec { env: Some("CRAFTSPACE_TEST_PROFILE_B"), ..ROOTS_A[0] }]));
    let spec = AppSpec { roots, ..SPEC_A };
    let profile = tmp.path().join("p.craftprofile");
    export(&spec, &ctx(), &[Part::Preferences], None, &profile).unwrap();
    let other = AppSpec { app: "othercraft", ..spec };
    assert!(import(&other, &ctx(), &profile, &Part::ALL).unwrap_err().to_string().contains("for TestCraft"));
    assert!(safe_relative("../../.ssh/id_rsa").is_err());
    assert!(safe_relative("Presets/a.json").is_ok());
    assert!(export(&spec, &ctx(), &[Part::Shortcuts], None, &tmp.path().join("empty.craftprofile")).is_err());
    std::env::remove_var("CRAFTSPACE_TEST_PROFILE_B");
}

#[test]
fn parts_parse() {
    assert_eq!(Part::parse_list("layouts, shortcuts").unwrap(), [Part::Layouts, Part::Shortcuts]);
    assert_eq!(Part::parse_list("").unwrap(), Part::ALL);
    assert!(Part::parse_list("everything").is_err());
}

#[test]
fn every_app_spec_is_consistent() {
    for spec in SPECS {
        assert!(crate::catalog::Catalog::builtin().app(spec.app).is_some(), "{} isn't an app", spec.app);
        for item in spec.items {
            assert!(spec.roots.iter().any(|r| r.key == item.root), "{}: unknown root {}", spec.app, item.root);
            if let What::Json { machine, .. } = item.what {
                assert!(machine.iter().all(|p| p.starts_with('/')), "{}: {machine:?}", spec.app);
            }
        }
    }
}
