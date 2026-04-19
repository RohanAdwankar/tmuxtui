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
    PathBuf::from(format!("/tmp/tt-rmsession-{}-{nanos}", std::process::id()))
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

fn run_harness(tmux_tmpdir: &Path, result_file: &Path) {
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/removed_session_window_selection.exp");
    let launch_dir = result_file.parent().expect("result parent");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(result_file)
        .current_dir(launch_dir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

#[test]
fn removed_session_from_selected_window_keeps_selection_on_adjacent_session() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");
    let result_file = root.join("selection.txt");

    fs::create_dir_all(&tmux_tmpdir).expect("create tmux tmpdir");
    fs::create_dir_all(&alpha_dir).expect("create alpha dir");
    fs::create_dir_all(&beta_dir).expect("create beta dir");

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
            "alpha",
            "-c",
            alpha_dir.to_str().expect("alpha dir utf-8"),
        ],
    );
    tmux(
        &tmux_tmpdir,
        &[
            "new-window",
            "-d",
            "-t",
            "alpha",
            "-n",
            "recent",
            "-c",
            alpha_dir.to_str().expect("alpha dir utf-8"),
        ],
    );
    tmux(
        &tmux_tmpdir,
        &[
            "new-session",
            "-d",
            "-s",
            "beta",
            "-c",
            beta_dir.to_str().expect("beta dir utf-8"),
        ],
    );

    run_harness(&tmux_tmpdir, &result_file);

    let attached = fs::read_to_string(&result_file).expect("read result file");
    assert!(
        attached.trim().starts_with("alphazzz:"),
        "expected adjacent session row after session removal, got {}",
        attached.trim()
    );

    cleanup();
}
