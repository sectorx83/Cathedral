use std::{fs, process::Command};

fn cathedral(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_cathedral"))
        .arg("--home")
        .arg(home)
        .args(args)
        .output()
        .expect("Cathedral CLI should start")
}

#[test]
fn cli_can_initialize_create_inspect_list_and_delete() {
    let home = tempfile::tempdir().unwrap();

    let init = cathedral(home.path(), &["init"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );

    let create = cathedral(home.path(), &["create", "integration-lab"]);
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let inspect = cathedral(home.path(), &["inspect", "integration-lab"]);
    assert!(inspect.status.success());
    assert!(String::from_utf8_lossy(&inspect.stdout).contains("integration-lab"));

    let list = cathedral(home.path(), &["list"]);
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains("integration-lab"));

    let doctor = cathedral(home.path(), &["doctor"]);
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    assert!(String::from_utf8_lossy(&doctor.stdout).contains("Experiment consistency: OK"));

    let delete = cathedral(home.path(), &["delete", "integration-lab"]);
    assert!(delete.status.success());
    assert!(String::from_utf8_lossy(&delete.stdout).contains("Deleted"));

    let empty = cathedral(home.path(), &["list"]);
    assert!(empty.status.success());
    assert!(!String::from_utf8_lossy(&empty.stdout).contains("integration-lab"));
}

#[test]
fn cli_rejects_manifest_name_mismatch() {
    let home = tempfile::tempdir().unwrap();
    let manifest = home.path().join("input.toml");
    fs::write(
        &manifest,
        "manifest_version = 1\n[experiment]\nname = \"different-name\"",
    )
    .unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_cathedral"))
        .arg("--home")
        .arg(home.path())
        .arg("create")
        .arg("requested-name")
        .arg("--manifest")
        .arg(&manifest)
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("does not match"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_dir(home.path().join("experiments"))
            .map(|entries| entries.count())
            .unwrap_or(0),
        0
    );
}

#[test]
fn cli_rejects_invalid_names_without_creating_a_workspace() {
    let home = tempfile::tempdir().unwrap();
    let result = cathedral(home.path(), &["create", "Uppercase"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid experiment name"));
    assert_eq!(
        fs::read_dir(home.path().join("experiments"))
            .map(|entries| entries.count())
            .unwrap_or(0),
        0
    );
}

#[test]
fn cli_rejects_duplicate_names_without_creating_an_orphan_workspace() {
    let home = tempfile::tempdir().unwrap();
    assert!(cathedral(home.path(), &["create", "duplicate-lab"])
        .status
        .success());

    let duplicate = cathedral(home.path(), &["create", "duplicate-lab"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already exists"));
    assert_eq!(
        fs::read_dir(home.path().join("experiments"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn doctor_reports_missing_registered_workspaces() {
    let home = tempfile::tempdir().unwrap();
    let create = cathedral(home.path(), &["create", "missing-workspace"]);
    assert!(create.status.success());

    let workspace = fs::read_dir(home.path().join("experiments"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::remove_dir_all(workspace).unwrap();

    let doctor = cathedral(home.path(), &["doctor"]);
    assert!(!doctor.status.success());
    assert!(String::from_utf8_lossy(&doctor.stderr).contains("doctor found 1 consistency problem"));
}

#[test]
fn cli_uses_cathedral_home_environment_override() {
    let home = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_cathedral"))
        .env("CATHEDRAL_HOME", home.path())
        .arg("init")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(home.path().join("cathedral.sqlite3").exists());
}

#[test]
fn doctor_reports_unregistered_workspace_directories() {
    let home = tempfile::tempdir().unwrap();
    assert!(cathedral(home.path(), &["init"]).status.success());
    fs::create_dir(home.path().join("experiments").join("orphaned-workspace")).unwrap();

    let doctor = cathedral(home.path(), &["doctor"]);
    assert!(!doctor.status.success());
    assert!(String::from_utf8_lossy(&doctor.stderr).contains("unregistered workspace"));
}
