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
    PathBuf::from(format!("/tmp/tt-kill-{}-{nanos}", std::process::id()))
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

fn set_last_target(tmpdir: &Path, target: &str) {
    let session_id = tmux(
        tmpdir,
        &["display-message", "-p", "-t", target, "#{session_id}"],
    );
    let window_id = tmux(
        tmpdir,
        &["display-message", "-p", "-t", target, "#{window_id}"],
    );
    let pane_id = tmux(
        tmpdir,
        &["display-message", "-p", "-t", target, "#{pane_id}"],
    );

    tmux(
        tmpdir,
        &["set-option", "-gq", "@tmuxtui-session", &session_id],
    );
    tmux(
        tmpdir,
        &["set-option", "-gq", "@tmuxtui-window", &window_id],
    );
    tmux(tmpdir, &["set-option", "-gq", "@tmuxtui-pane", &pane_id]);
}

fn run_harness(tmux_tmpdir: &Path, result_file: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/kill_window_selection.exp");
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
fn killing_window_in_other_session_keeps_selection_in_that_session() {
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
            "new-session",
            "-d",
            "-s",
            "beta",
            "-c",
            beta_dir.to_str().expect("beta dir utf-8"),
        ],
    );
    tmux(
        &tmux_tmpdir,
        &[
            "new-window",
            "-d",
            "-t",
            "beta",
            "-n",
            "two",
            "-c",
            beta_dir.to_str().expect("beta dir utf-8"),
        ],
    );
    tmux(&tmux_tmpdir, &["rename-window", "-t", "beta:0", "one"]);

    let target = tmux(
        &tmux_tmpdir,
        &["display-message", "-p", "-t", "beta:1", "#{pane_id}"],
    );
    set_last_target(&tmux_tmpdir, &target);

    run_harness(&tmux_tmpdir, &result_file);

    let attached = fs::read_to_string(&result_file).expect("read result file");
    assert_eq!(attached.trim(), "beta:one");

    cleanup();
}
