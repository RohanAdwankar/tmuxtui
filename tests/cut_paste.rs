use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    PathBuf::from(format!(
        "/tmp/tt-cut-paste-{}-{nanos}-{counter}",
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

fn run_harness(tmux_tmpdir: &Path, keys: &str) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cut_paste.exp");
    let status = Command::new("expect")
        .arg(script)
        .arg(env!("CARGO_BIN_EXE_tmuxtui"))
        .arg(tmux_tmpdir)
        .arg(keys)
        .stdin(Stdio::null())
        .env_remove("TMUX")
        .status()
        .expect("failed to run expect harness");
    assert!(status.success(), "expect harness failed with {status}");
}

#[test]
fn lowercase_p_pastes_pane_as_window_on_selected_session() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");

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
            "split-window",
            "-h",
            "-t",
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

    let pane_id = tmux(
        &tmux_tmpdir,
        &["list-panes", "-t", "alpha", "-F", "#{pane_id}"],
    )
    .lines()
    .last()
    .expect("alpha second pane")
    .to_owned();

    run_harness(&tmux_tmpdir, "jxjp");

    let beta_windows = tmux(
        &tmux_tmpdir,
        &["list-windows", "-t", "beta", "-F", "#{window_id}"],
    );
    assert_eq!(beta_windows.lines().count(), 2);

    let beta_panes = tmux(
        &tmux_tmpdir,
        &[
            "list-panes",
            "-a",
            "-F",
            "#{session_name}:#{window_id}:#{pane_id}",
        ],
    );
    assert!(
        beta_panes
            .lines()
            .any(|line| line.starts_with("beta:") && line.ends_with(&pane_id))
    );

    cleanup();
}

#[test]
fn lowercase_p_pastes_window_as_window_on_selected_session() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");

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
            "-n",
            "one",
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
            "two",
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

    run_harness(&tmux_tmpdir, "jxjp");

    let beta_windows = tmux(
        &tmux_tmpdir,
        &["list-windows", "-t", "beta", "-F", "#{window_name}"],
    );
    assert!(beta_windows.lines().any(|line| line == "two"));
    assert_eq!(beta_windows.lines().count(), 2);

    cleanup();
}

#[test]
fn uppercase_p_pastes_pane_as_new_session_on_selected_session() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");

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
            "split-window",
            "-h",
            "-t",
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

    let pane_id = tmux(
        &tmux_tmpdir,
        &["list-panes", "-t", "alpha", "-F", "#{pane_id}"],
    )
    .lines()
    .last()
    .expect("alpha second pane")
    .to_owned();

    run_harness(&tmux_tmpdir, "jxjP");

    let sessions = tmux(&tmux_tmpdir, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.lines().count(), 3);

    let panes = tmux(
        &tmux_tmpdir,
        &["list-panes", "-a", "-F", "#{session_name}:#{pane_id}"],
    );
    assert!(panes.lines().any(|line| !line.starts_with("alpha:")
        && !line.starts_with("beta:")
        && line.ends_with(&pane_id)));

    cleanup();
}

#[test]
fn uppercase_p_pastes_window_as_new_session_on_selected_session() {
    let root = unique_temp_dir();
    let tmux_tmpdir = root.join("tmux");
    let alpha_dir = root.join("alpha");
    let beta_dir = root.join("beta");

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
            "-n",
            "one",
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
            "two",
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

    run_harness(&tmux_tmpdir, "jxjP");

    let sessions = tmux(&tmux_tmpdir, &["list-sessions", "-F", "#{session_name}"]);
    assert_eq!(sessions.lines().count(), 3);

    let windows = tmux(
        &tmux_tmpdir,
        &["list-windows", "-a", "-F", "#{session_name}:#{window_name}"],
    );
    assert!(windows.lines().any(|line| !line.starts_with("alpha:")
        && !line.starts_with("beta:")
        && line.ends_with(":two")));

    cleanup();
}
