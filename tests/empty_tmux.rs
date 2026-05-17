use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    PathBuf::from(format!("/tmp/tt-empty-{}-{nanos}", std::process::id()))
}

fn run_harness(tmux_tmpdir: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/empty_tmux.exp");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

#[test]
fn opens_with_no_tmux_server() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");

    fs::create_dir_all(&tmux_tmpdir).expect("create tmux tmpdir");

    run_harness(&tmux_tmpdir);

    let _ = Command::new("tmux")
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", &tmux_tmpdir)
        .args(["kill-server"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = fs::remove_dir_all(&root);
}
