use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{fs, path::Path, process::Command};

use lai::pet::lifecycle::{detached_arguments, run_pet_lifecycle, wait_for_pet_ui};

#[test]
fn server_panic_signals_shutdown_and_is_reported() {
    let result = run_pet_lifecycle(
        |_| panic!("server failure"),
        |shutdown, done| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            assert!(done.load(Ordering::Acquire));
            Ok(())
        },
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("server thread panicked")
    );
}

#[test]
fn detached_arguments_preserve_pet_and_port() {
    assert_eq!(
        detached_arguments(1234, true),
        ["serve", "--port", "1234", "--ready-signal", "--pet"]
    );
    assert_eq!(
        detached_arguments(1234, false),
        ["serve", "--port", "1234", "--ready-signal"]
    );
}

#[test]
fn ui_failure_requests_server_shutdown() {
    let result = run_pet_lifecycle(
        |shutdown| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            Ok(())
        },
        |_, _| anyhow::bail!("UI failed"),
    );
    assert!(result.unwrap_err().to_string().contains("UI failed"));
}

#[test]
fn server_failure_signals_ui_and_is_reported() {
    let result = run_pet_lifecycle(
        |_| anyhow::bail!("server failed"),
        |shutdown, done| {
            while !shutdown.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            assert!(done.load(Ordering::Acquire));
            Ok(())
        },
    );
    assert!(result.unwrap_err().to_string().contains("server failed"));
}

#[test]
fn detached_readiness_waits_for_pet_ui_or_shutdown() {
    let ready = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let r = Arc::clone(&ready);
    let s = Arc::clone(&shutdown);
    let worker = std::thread::spawn(move || tx.send(wait_for_pet_ui(&r, &s)).unwrap());
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(50))
            .is_err()
    );
    ready.store(true, Ordering::Release);
    assert!(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap());
    worker.join().unwrap();
    ready.store(false, Ordering::Release);
    shutdown.store(true, Ordering::Release);
    assert!(!wait_for_pet_ui(&ready, &shutdown));
}

#[test]
fn custom_pet_assets_override_embedded_model_and_configuration() {
    let store = lai::pet::assets::AssetStore::new(
        Some(b"glTFcustom".to_vec()),
        br#"{"idle":"wave"}"#.to_vec(),
    );
    assert_eq!(store.get("/pet.glb").unwrap().1, b"glTFcustom");
    assert_eq!(store.get("/config.json").unwrap().1, br#"{"idle":"wave"}"#);
    assert_eq!(store.get("/pet.js").unwrap().0, "text/javascript");
}

#[test]
fn bundled_pet_assets_have_expected_content_types() {
    let store = lai::pet::assets::AssetStore::new(None, vec![]);
    for (path, mime) in [
        ("/", "text/html"),
        ("/pet.js", "text/javascript"),
        ("/vendor/three.module.js", "text/javascript"),
        ("/vendor/GLTFLoader.js", "text/javascript"),
        ("/vendor/BufferGeometryUtils.js", "text/javascript"),
        ("/pet.glb", "model/gltf-binary"),
    ] {
        let (content_type, content) = store.get(path).expect("embedded asset must exist");
        assert_eq!(content_type, mime);
        assert!(!content.is_empty());
    }
    assert!(store.get("/missing").is_none());
    assert!(store.get("/../pet.glb").is_none());
    assert!(store.get("/cat4.glb").is_none());
    assert_eq!(&store.get("/pet.glb").unwrap().1[..4], b"glTF");
}

fn lai(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lai"))
        .args(args)
        .env("HOME", home)
        .output()
        .expect("CLI should start")
}

fn read_config(home: &Path) -> toml_edit::DocumentMut {
    fs::read_to_string(home.join(".lai/config.toml"))
        .expect("config should exist")
        .parse()
        .expect("config should be valid TOML")
}

fn show(home: &Path) -> serde_json::Value {
    let output = lai(home, &["config", "pet", "show"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("show should return JSON")
}

#[test]
fn show_uses_default_pet_settings_without_a_config() {
    let home = tempfile::tempdir().unwrap();
    let config = show(home.path());
    assert!(config["model"].is_null());
    assert_eq!(config["animations"]["idle"], "1_normal");
    assert_eq!(config["animations"]["waiting"], "6_curious");
    assert_eq!(config["animations"]["reply"], "2_happy");
    assert_eq!(config["animations"]["error"], "3_sad");
    assert_eq!(config["animations"]["shutdown"], "3_sad");
}

#[test]
fn show_loads_partial_overrides_and_a_relative_model() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("cat.glb"), b"glTF\x02\0\0\0\x0c\0\0\0").unwrap();
    fs::write(dir.join("config.toml"), "model = 'owner/model'\n[pet]\nmodel = 'cat.glb'\n[pet.animations]\nidle = 'Blink'\nshutdown = 'Sleep'\n").unwrap();
    let config = show(home.path());
    assert_eq!(config["model"], "cat.glb");
    assert_eq!(config["animations"]["idle"], "Blink");
    assert_eq!(config["animations"]["waiting"], "6_curious");
    assert_eq!(config["animations"]["reply"], "2_happy");
    assert_eq!(config["animations"]["error"], "3_sad");
    assert_eq!(config["animations"]["shutdown"], "Sleep");
}

#[test]
fn show_reports_invalid_pet_config_and_invalid_or_missing_model() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("bad.glb"), b"not a GLB").unwrap();
    for contents in [
        "[pet.animations]\nidle = 3",
        "[pet.animations]\nidle = '  '",
        "[pet.animations]\nidell = 'normal'",
        "[pet]\nanimations = 7",
        "[pet.animations\n",
        "[pet]\nmodel = 'missing.glb'",
        "[pet]\nmodel = 'bad.glb'",
    ] {
        fs::write(dir.join("config.toml"), contents).unwrap();
        let output = lai(home.path(), &["config", "pet", "show"]);
        assert!(
            !output.status.success(),
            "unexpected success for {contents}"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("config.toml"));
    }
}

#[test]
fn serve_pet_flags_are_documented_and_accepted() {
    let home = tempfile::tempdir().unwrap();
    let output = lai(home.path(), &["serve", "--help"]);
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--pet") && help.contains("-p"), "{help}");

    for flag in ["--pet", "-p"] {
        let output = lai(home.path(), &["serve", flag]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("lai config model set"));
    }
}

#[test]
fn pet_settings_share_config_and_preserve_other_settings() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    fs::write(
        dir.join("config.toml"),
        "model = 'owner/model'\n[other]\nvalue = 42\n",
    )
    .unwrap();
    fs::write(dir.join("cat.glb"), b"glTF\x02\0\0\0\x0c\0\0\0").unwrap();

    for args in [
        &["config", "pet", "model", "set", "cat.glb"][..],
        &["config", "pet", "animation", "set", "idle", "Blink"][..],
        &["config", "pet", "animation", "set", "shutdown", "Sleep"][..],
    ] {
        let output = lai(home.path(), args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config = read_config(home.path());
    assert_eq!(config["model"].as_str(), Some("owner/model"));
    assert_eq!(config["other"]["value"].as_integer(), Some(42));
    assert_eq!(config["pet"]["model"].as_str(), Some("cat.glb"));
    assert_eq!(config["pet"]["animations"]["idle"].as_str(), Some("Blink"));
    assert_eq!(
        config["pet"]["animations"]["shutdown"].as_str(),
        Some("Sleep")
    );
    assert!(!dir.join("pet.toml").exists());
}

#[test]
fn unsetting_individual_pet_settings_preserves_the_rest() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("config.toml"), "model = 'owner/model'\n[pet]\nmodel = 'cat.glb'\n[pet.animations]\nidle = 'Blink'\nshutdown = 'Sleep'\n").unwrap();
    for args in [
        &["config", "pet", "model", "unset"][..],
        &["config", "pet", "animation", "unset", "idle"][..],
    ] {
        let output = lai(home.path(), args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let config = read_config(home.path());
    assert_eq!(config["model"].as_str(), Some("owner/model"));
    assert!(config["pet"].get("model").is_none());
    assert!(config["pet"]["animations"].get("idle").is_none());
    assert_eq!(
        config["pet"]["animations"]["shutdown"].as_str(),
        Some("Sleep")
    );
}

#[test]
fn setting_animation_handles_an_inline_pet_table() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("config.toml"), "model = 'owner/model'\npet = {}\n").unwrap();
    let output = lai(
        home.path(),
        &["config", "pet", "animation", "set", "idle", "Blink"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config = read_config(home.path());
    assert_eq!(config["model"].as_str(), Some("owner/model"));
    assert_eq!(config["pet"]["animations"]["idle"].as_str(), Some("Blink"));
}

#[test]
fn rejects_invalid_pet_settings_without_damaging_config() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".lai");
    fs::create_dir(&dir).unwrap();
    let original = "model = 'owner/model'\n";
    fs::write(dir.join("config.toml"), original).unwrap();
    for args in [
        &["config", "pet", "animation", "set", "unknown", "Wave"][..],
        &["config", "pet", "animation", "set", "idle", "  "][..],
        &["config", "pet", "model", "set", "missing.glb"][..],
    ] {
        let output = lai(home.path(), args);
        assert!(!output.status.success(), "invalid pet setting should fail");
        assert_eq!(
            fs::read_to_string(dir.join("config.toml")).unwrap(),
            original
        );
    }
    fs::write(dir.join("bad.glb"), b"not a GLB").unwrap();
    let output = lai(home.path(), &["config", "pet", "model", "set", "bad.glb"]);
    assert!(!output.status.success());
    assert_eq!(
        fs::read_to_string(dir.join("config.toml")).unwrap(),
        original
    );
}
