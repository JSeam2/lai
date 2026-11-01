use std::process::Command;

#[test]
fn key_commands_list_recognized_providers() {
    for action in ["set", "unset", "rotate"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lai"))
            .args(["key", action, "--help"])
            .output()
            .expect("test setup or operation should succeed");
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("possible values: huggingface"), "{help}");
    }
}

#[test]
fn key_commands_reject_unrecognized_providers() {
    for action in ["set", "unset", "rotate"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lai"))
            .args(["key", action, "unknown"])
            .output()
            .expect("test setup or operation should succeed");
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("huggingface"), "{error}");
    }
}
