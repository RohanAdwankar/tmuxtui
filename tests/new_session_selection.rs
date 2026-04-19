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
    PathBuf::from(format!("/tmp/tt-session-{}-{nanos}", std::process::id()))
}

fn tmux(tmpdir: &Path, args: &[&str]) -> String {
    let output = Command::new("tmux")
        .env_remove("TMUX")
        .env("TMUX_TMPDIR", tmpdir)
        .args(args)
        .output()
        .expect("failed to run tmux");
    assert!(
        output.status.success(),
        "tmux {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn run_harness(tmux_tmpdir: &Path, session_name: &str, result_file: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/new_session_selection.exp");
    let launch_dir = result_file.parent().expect("result parent");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(session_name)
        .arg(result_file)
        .current_dir(launch_dir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

#[test]
fn new_session_becomes_current_selection_for_attach() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let work_dir = root.join("work");
    let result_file = root.join("session.txt");
    let session_name = "fresh";

    fs::create_dir_all(&tmux_tmpdir).expect("create tmux tmpdir");
    fs::create_dir_all(&work_dir).expect("create work dir");

    let cleanup = || {
        let _ = Command::new("tmux")
            .env_remove("TMUX")
            .env("TMUX_TMPDIR", &tmux_tmpdir)
            .args(["kill-server"])
            .status();
        let _ = fs::remove_dir_all(&root);
    };

    tmux(
        &tmux_tmpdir,
        &[
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "work",
            "-c",
            work_dir.to_str().expect("work dir utf-8"),
        ],
    );

    run_harness(&tmux_tmpdir, session_name, &result_file);

    let attached_session = fs::read_to_string(&result_file).expect("read result file");
    assert_eq!(attached_session.trim(), session_name);

    cleanup();
}
