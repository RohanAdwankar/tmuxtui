use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let counter = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    PathBuf::from(format!(
        "/tmp/tt-split-{}-{nanos}-{counter}",
        std::process::id()
    ))
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

fn run_harness(tmux_tmpdir: &Path, split_key: &str, result_file: &Path) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/split_orientation.exp");
    let launch_dir = result_file.parent().expect("result parent");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(split_key)
        .arg(result_file)
        .current_dir(launch_dir)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

fn parse_panes(result_file: &Path) -> Vec<(i32, i32)> {
    fs::read_to_string(result_file)
        .expect("read result file")
        .lines()
        .map(|line| {
            let mut parts = line.splitn(3, ' ');
            let left = parts
                .next()
                .expect("pane left")
                .parse::<i32>()
                .expect("left int");
            let top = parts
                .next()
                .expect("pane top")
                .parse::<i32>()
                .expect("top int");
            (left, top)
        })
        .collect()
}

fn assert_split(split_key: &str, expect_same_left: bool, expect_same_top: bool) {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let work_dir = root.join("work");
    let result_file = root.join("panes.txt");

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
    let work_pane_id = tmux(
        &tmux_tmpdir,
        &["list-panes", "-t", "work", "-F", "#{pane_id}"],
    );
    set_last_target(
        &tmux_tmpdir,
        work_pane_id.lines().next().expect("work pane id"),
    );

    run_harness(&tmux_tmpdir, split_key, &result_file);

    let panes = parse_panes(&result_file);
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0].0 == panes[1].0, expect_same_left);
    assert_eq!(panes[0].1 == panes[1].1, expect_same_top);

    cleanup();
}

#[test]
fn lowercase_s_creates_stacked_split_and_numbered_panes() {
    assert_split("s", true, false);
}

#[test]
fn uppercase_s_creates_side_by_side_split_and_numbered_panes() {
    assert_split("S", false, true);
}
