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
    PathBuf::from(format!("/tmp/tt-reattach-{}-{nanos}", std::process::id()))
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

fn run_harness(
    tmux_tmpdir: &Path,
    result_file: &Path,
    ready_file: &Path,
    continue_file: &Path,
) -> std::process::Child {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/reattach_recent_session.exp");
    let launch_dir = result_file.parent().expect("result parent");
    Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(result_file)
        .arg(ready_file)
        .arg(continue_file)
        .current_dir(launch_dir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .spawn()
        .expect("failed to run expect harness")
}

#[test]
fn detach_and_reattach_returns_to_recent_session_directory() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");
    let result_file = root.join("pwd.txt");
    let ready_file = root.join("ready");
    let continue_file = root.join("continue");

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
    let beta_session_id = tmux(
        &tmux_tmpdir,
        &["display-message", "-p", "-t", "beta", "#{session_id}"],
    );
    let beta_window_id = tmux(
        &tmux_tmpdir,
        &["display-message", "-p", "-t", "beta", "#{window_id}"],
    );

    let mut child = run_harness(&tmux_tmpdir, &result_file, &ready_file, &continue_file);

    for _ in 0..120 {
        if ready_file.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(
        ready_file.exists(),
        "expect harness did not reach detach point"
    );

    let session_id = tmux(&tmux_tmpdir, &["show-options", "-gqv", "@tmuxtui-session"]);
    let window_id = tmux(&tmux_tmpdir, &["show-options", "-gqv", "@tmuxtui-window"]);
    assert_eq!(session_id, beta_session_id);
    assert_eq!(window_id, beta_window_id);

    fs::write(&continue_file, "").expect("write continue file");

    let status = child.wait().expect("wait for expect harness");
    assert!(status.success(), "expect harness failed with {status}");

    let pwd = fs::read_to_string(&result_file).expect("read result file");
    assert!(
        pwd.trim() != "missing",
        "expected marker in the reattached session directory"
    );
    let actual_dir = fs::canonicalize(pwd.trim()).expect("canonicalize actual dir");
    let expected_dir = fs::canonicalize(&beta_dir).expect("canonicalize expected dir");
    assert_eq!(actual_dir, expected_dir);

    cleanup();
}
