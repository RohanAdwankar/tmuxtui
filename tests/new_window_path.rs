use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    PathBuf::from(format!("/tmp/tt-{}-{nanos}", std::process::id()))
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

fn run_harness(launch_dir: &Path, tmux_tmpdir: &Path, result_file: &Path, touch_name: &str) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/new_window_path.exp");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(result_file)
        .arg(touch_name)
        .current_dir(launch_dir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

fn assert_new_window_directory(initial_dir: &Path, expected_dir: &Path) {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let launch_dir = root.join("launch");
    let result_file = root.join("pwd.txt");
    let touch_name = "marker";

    fs::create_dir_all(&tmux_tmpdir).expect("create tmux tmpdir");
    fs::create_dir_all(&launch_dir).expect("create launch dir");
    fs::create_dir_all(initial_dir).expect("create initial dir");

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
            initial_dir.to_str().expect("initial dir utf-8"),
        ],
    );
    let work_pane_id = tmux(
        &tmux_tmpdir,
        &["list-panes", "-t", "work", "-F", "#{pane_id}"],
    )
    .lines()
    .next()
    .expect("work pane id")
    .to_owned();

    if initial_dir != expected_dir {
        tmux(
            &tmux_tmpdir,
            &[
                "send-keys",
                "-t",
                &work_pane_id,
                &format!("cd '{}'", expected_dir.display()),
                "C-m",
            ],
        );
        thread::sleep(Duration::from_millis(300));
    }

    set_last_target(&tmux_tmpdir, &work_pane_id);

    run_harness(&launch_dir, &tmux_tmpdir, &result_file, touch_name);

    let pwd = fs::read_to_string(&result_file).expect("read result file");
    let actual_dir = fs::canonicalize(pwd.trim()).expect("canonicalize actual dir");
    let expected_dir = fs::canonicalize(expected_dir).expect("canonicalize expected dir");
    assert_eq!(actual_dir, expected_dir);
    assert!(
        expected_dir.join(touch_name).exists(),
        "touch marker was not created by the attached shell"
    );

    cleanup();
}

#[test]
fn new_window_uses_single_window_session_directory() {
    let root = unique_temp_dir();
    let initial_dir = root.join("work");
    assert_new_window_directory(&initial_dir, &initial_dir);
}

#[test]
fn new_window_uses_live_directory_after_cd() {
    let root = unique_temp_dir();
    let initial_dir = root.join("work");
    let expected_dir = initial_dir.join("nested");
    fs::create_dir_all(&expected_dir).expect("create nested dir");
    assert_new_window_directory(&initial_dir, &expected_dir);
}
