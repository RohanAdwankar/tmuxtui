use std::{
    ffi::OsStr,
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};

use crate::managed_config::ManagedConfig;

#[derive(Clone, Debug)]
pub struct Tmux {
    managed: ManagedConfig,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub sessions: Vec<Session>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub attached: bool,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug)]
pub struct Window {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub session_id: String,
    pub panes: Vec<Pane>,
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub id: String,
    pub current_command: String,
    pub current_path: String,
    pub active: bool,
    pub zoomed: bool,
    pub window_id: String,
}

#[derive(Clone, Debug)]
pub enum TargetKind {
    Session(String),
    Window {
        session_id: String,
        window_id: String,
    },
    Pane {
        session_id: String,
        window_id: String,
        pane_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastTarget {
    pub session_id: String,
    pub window_id: Option<String>,
    pub pane_id: Option<String>,
}

impl Tmux {
    pub fn new(managed: ManagedConfig) -> Self {
        Self { managed }
    }

    pub fn ensure_ready(&self) -> Result<()> {
        self.run_with_config(["start-server"])?;
        self.reload_config()?;
        self.clear_stale_pane_options()?;
        Ok(())
    }

    pub fn show_hints(&self) -> bool {
        self.managed.settings().show_hints
    }

    pub fn sidebar_percent(&self) -> u8 {
        self.managed.settings().sidebar_percent
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let sessions_raw = self.run_or_empty([
            "list-sessions",
            "-F",
            "#{session_id}\t#{session_name}\t#{session_attached}",
        ])?;

        let windows_raw = self.run_or_empty([
            "list-windows",
            "-a",
            "-F",
            "#{session_id}\t#{window_id}\t#{window_name}\t#{window_active}",
        ])?;

        let panes_raw = self.run_or_empty([
            "list-panes",
            "-a",
            "-F",
            "#{window_id}\t#{pane_id}\t#{pane_title}\t#{pane_current_command}\t#{pane_current_path}\t#{pane_active}\t#{window_zoomed_flag}",
        ])?;

        let mut sessions = parse_sessions(&sessions_raw);
        let mut windows = parse_windows(&windows_raw);
        let panes = parse_panes(&panes_raw);

        for pane in panes {
            if let Some(window) = windows
                .iter_mut()
                .find(|window| window.id == pane.window_id)
            {
                window.panes.push(pane);
            }
        }

        for window in windows {
            if let Some(session) = sessions
                .iter_mut()
                .find(|session| session.id == window.session_id)
            {
                session.windows.push(window);
            }
        }

        sessions.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(Snapshot { sessions })
    }

    pub fn capture_pane(&self, pane_id: &str) -> Result<String> {
        self.run(["capture-pane", "-J", "-p", "-t", pane_id, "-S", "-120"])
    }

    pub fn archive_panes(&self, name: &str, panes: &[(String, String)]) -> Result<String> {
        let archive_dir = self.managed.archive_dir();
        fs::create_dir_all(&archive_dir).with_context(|| {
            format!(
                "failed to create archive directory at {}",
                archive_dir.display()
            )
        })?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system time before unix epoch")?
            .as_secs();
        let path = archive_dir.join(format!("{timestamp}-{}.txt", archive_name(name)));
        let mut output = String::new();
        output.push_str(&format!("archive: {name}\n"));
        output.push_str(&format!("created: {timestamp}\n"));

        for (label, pane_id) in panes {
            output.push_str(&format!("\n--- {label} {pane_id} ---\n"));
            output.push_str(&self.capture_full_pane(pane_id)?);
            output.push('\n');
        }

        fs::write(&path, output)
            .with_context(|| format!("failed to write archive at {}", path.display()))?;
        Ok(path.display().to_string())
    }

    pub fn create_session(&self, name: &str) -> Result<String> {
        let output = if name.is_empty() {
            self.run(["new-session", "-P", "-F", "#{session_id}", "-d"])
        } else {
            self.run(["new-session", "-P", "-F", "#{session_id}", "-d", "-s", name])
        }?;
        Ok(output.trim().to_owned())
    }

    fn capture_full_pane(&self, pane_id: &str) -> Result<String> {
        self.run(["capture-pane", "-J", "-p", "-t", pane_id, "-S", "-"])
    }

    pub fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        self.run(["rename-session", "-t", session_id, name])
            .map(|_| ())
    }

    pub fn kill_session(&self, session_id: &str) -> Result<()> {
        self.run(["kill-session", "-t", session_id]).map(|_| ())
    }

    pub fn new_window(
        &self,
        session_id: &str,
        base_pane_id: Option<&str>,
        name: &str,
    ) -> Result<String> {
        let mut args = vec![
            "new-window",
            "-P",
            "-F",
            "#{window_id}",
            "-d",
            "-t",
            session_id,
        ];

        let start_path = match base_pane_id {
            Some(pane_id) => Some(self.pane_current_path(pane_id)?),
            None => Some(self.session_current_path(session_id)?),
        };
        if let Some(path) = start_path.as_deref() {
            args.push("-c");
            args.push(path);
        }
        if !name.is_empty() {
            args.push("-n");
            args.push(name);
        }

        let output = self.run(args)?;
        Ok(output.trim().to_owned())
    }

    pub fn rename_window(&self, window_id: &str, name: &str) -> Result<()> {
        self.run(["rename-window", "-t", window_id, name])
            .map(|_| ())
    }

    pub fn kill_window(&self, window_id: &str) -> Result<()> {
        self.run(["kill-window", "-t", window_id]).map(|_| ())
    }

    pub fn move_window_to_session(&self, window_id: &str, session_id: &str) -> Result<()> {
        let target = format!("{session_id}:");
        self.run(["move-window", "-s", window_id, "-t", &target])
            .map(|_| ())
    }

    pub fn move_window_to_new_session(&self, window_id: &str) -> Result<String> {
        let (session_id, dummy_window_id) = self.create_session_with_window("")?;
        self.move_window_to_session(window_id, &session_id)?;
        self.kill_window(&dummy_window_id)?;
        Ok(session_id)
    }

    pub fn split_pane(&self, pane_id: &str, vertical: bool) -> Result<String> {
        let output = self.run([
            "split-window",
            "-P",
            "-F",
            "#{pane_id}",
            "-t",
            pane_id,
            "-c",
            "#{pane_current_path}",
            if vertical { "-h" } else { "-v" },
        ])?;
        Ok(output.trim().to_owned())
    }

    fn pane_current_path(&self, pane_id: &str) -> Result<String> {
        self.run([
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{pane_current_path}",
        ])
        .map(|output| output.trim().to_owned())
    }

    fn session_current_path(&self, session_id: &str) -> Result<String> {
        self.run([
            "display-message",
            "-p",
            "-t",
            session_id,
            "#{pane_current_path}",
        ])
        .map(|output| output.trim().to_owned())
    }

    fn create_session_with_window(&self, name: &str) -> Result<(String, String)> {
        let output = if name.is_empty() {
            self.run([
                "new-session",
                "-P",
                "-F",
                "#{session_id}\t#{window_id}",
                "-d",
            ])
        } else {
            self.run([
                "new-session",
                "-P",
                "-F",
                "#{session_id}\t#{window_id}",
                "-d",
                "-s",
                name,
            ])
        }?;
        let mut parts = output.trim().split('\t');
        let session_id = parts
            .next()
            .context("missing created session id")?
            .to_owned();
        let window_id = parts
            .next()
            .context("missing created window id")?
            .to_owned();
        Ok((session_id, window_id))
    }

    pub fn rename_pane(&self, pane_id: &str, name: &str) -> Result<()> {
        self.run(["select-pane", "-t", pane_id, "-T", name])
            .map(|_| ())
    }

    pub fn kill_pane(&self, pane_id: &str) -> Result<()> {
        self.run(["kill-pane", "-t", pane_id]).map(|_| ())
    }

    pub fn move_pane_to_window(&self, pane_id: &str, target_pane_id: &str) -> Result<()> {
        self.run(["join-pane", "-h", "-f", "-s", pane_id, "-t", target_pane_id])
            .map(|_| ())
    }

    pub fn move_pane_to_new_window(&self, pane_id: &str, session_id: &str) -> Result<String> {
        let target = format!("{session_id}:");
        let output = self.run([
            "break-pane",
            "-d",
            "-P",
            "-F",
            "#{window_id}",
            "-s",
            pane_id,
            "-t",
            &target,
        ])?;
        Ok(output.trim().to_owned())
    }

    pub fn move_pane_to_new_session(&self, pane_id: &str) -> Result<String> {
        let (session_id, dummy_window_id) = self.create_session_with_window("")?;
        self.move_pane_to_new_window(pane_id, &session_id)?;
        self.kill_window(&dummy_window_id)?;
        Ok(session_id)
    }

    pub fn toggle_zoom(&self, pane_id: &str) -> Result<()> {
        self.run(["resize-pane", "-t", pane_id, "-Z"]).map(|_| ())
    }

    pub fn attach_remote_tmux(&self, pane_id: &str) -> Result<()> {
        self.run([
            "send-keys",
            "-t",
            pane_id,
            "tmux new-session -A -s tmuxtui",
            "C-m",
        ])
        .map(|_| ())
    }

    pub fn attach(&self, target: &TargetKind) -> Result<()> {
        self.apply_pinned_pane(target)?;
        match target {
            TargetKind::Session(session_id) => {
                self.exec_attach(["attach-session", "-t", session_id])
            }
            TargetKind::Window {
                session_id,
                window_id,
            } => {
                self.run(["select-window", "-t", window_id])?;
                self.exec_attach(["attach-session", "-t", session_id])
            }
            TargetKind::Pane {
                session_id,
                window_id,
                pane_id,
            } => {
                self.run(["select-window", "-t", window_id])?;
                self.run(["select-pane", "-t", pane_id])?;
                self.exec_attach(["attach-session", "-t", session_id])
            }
        }
    }

    pub fn has_tmux_binary(&self) -> Result<()> {
        let output = Command::new("tmux")
            .arg("-V")
            .output()
            .context("failed to execute tmux")?;
        if output.status.success() {
            Ok(())
        } else {
            bail!("tmux is installed but not usable")
        }
    }

    pub fn set_show_hints(&mut self, show_hints: bool) -> Result<()> {
        self.managed.set_show_hints(show_hints)?;
        self.reload_config()
    }

    pub fn set_show_status(&mut self, show_status: bool) -> Result<()> {
        self.managed.set_show_status(show_status)?;
        self.reload_config()
    }

    pub fn set_sidebar_percent(&mut self, sidebar_percent: u8) -> Result<()> {
        self.managed.set_sidebar_percent(sidebar_percent)?;
        Ok(())
    }

    pub fn set_last_target(&self, target: &TargetKind) -> Result<()> {
        let (session_id, window_id, pane_id) = match target {
            TargetKind::Session(session_id) => (session_id.as_str(), "", ""),
            TargetKind::Window {
                session_id,
                window_id,
            } => (session_id.as_str(), window_id.as_str(), ""),
            TargetKind::Pane {
                session_id,
                window_id,
                pane_id,
            } => (session_id.as_str(), window_id.as_str(), pane_id.as_str()),
        };

        self.run(["set-option", "-gq", "@tmuxtui-session", session_id])?;
        self.run(["set-option", "-gq", "@tmuxtui-window", window_id])?;
        self.run(["set-option", "-gq", "@tmuxtui-pane", pane_id])?;
        Ok(())
    }

    pub fn set_pinned_pane(&self, pane_id: Option<&str>) -> Result<()> {
        self.run([
            "set-option",
            "-gq",
            "@tmuxtui-pinned-pane",
            pane_id.unwrap_or(""),
        ])?;
        Ok(())
    }

    pub fn pinned_pane(&self) -> Option<String> {
        self.option_value("@tmuxtui-pinned-pane")
    }

    pub fn last_target(&self) -> Option<LastTarget> {
        let session_id = self.option_value("@tmuxtui-session")?;
        Some(LastTarget {
            window_id: self.option_value("@tmuxtui-window"),
            pane_id: self.option_value("@tmuxtui-pane"),
            session_id,
        })
    }

    fn exec_attach<const N: usize>(&self, args: [&str; N]) -> Result<()> {
        let status = Command::new("tmux")
            .args(args)
            .status()
            .context("failed to execute tmux attach")?;
        if status.success() || status.code() == Some(1) {
            Ok(())
        } else {
            Err(anyhow!("attach failed"))
        }
    }

    fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_command("tmux", args)
    }

    fn option_value(&self, option: &str) -> Option<String> {
        self.run(["show-options", "-gqv", option])
            .ok()
            .map(|output| output.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    fn reload_config(&self) -> Result<()> {
        self.run_or_empty([
            "source-file",
            self.managed.tmux_conf.to_string_lossy().as_ref(),
        ])?;
        Ok(())
    }

    fn clear_stale_pane_options(&self) -> Result<()> {
        if self
            .pinned_pane()
            .as_deref()
            .is_some_and(|pane_id| self.window_id_for_pane(pane_id).is_err())
        {
            self.set_pinned_pane(None)?;
        }

        if self
            .option_value("@tmuxtui-pane")
            .as_deref()
            .is_some_and(|pane_id| self.window_id_for_pane(pane_id).is_err())
        {
            self.run(["set-option", "-gq", "@tmuxtui-pane", ""])?;
        }

        Ok(())
    }

    fn apply_pinned_pane(&self, target: &TargetKind) -> Result<()> {
        let Some(pinned_pane_id) = self.pinned_pane() else {
            return Ok(());
        };

        let Some((target_window_id, target_pane_id)) = self.pin_destination(target)? else {
            return Ok(());
        };

        let pinned_window_id = match self.window_id_for_pane(&pinned_pane_id) {
            Ok(window_id) => window_id,
            Err(_) => {
                self.set_pinned_pane(None)?;
                return Ok(());
            }
        };

        if pinned_window_id == target_window_id {
            return Ok(());
        }

        self.move_pane_to_window(&pinned_pane_id, &target_pane_id)?;
        Ok(())
    }

    fn pin_destination(&self, target: &TargetKind) -> Result<Option<(String, String)>> {
        match target {
            TargetKind::Session(session_id) => {
                let target_pane_id = self.active_pane_for_target(session_id)?;
                let target_window_id = self.window_id_for_pane(&target_pane_id)?;
                Ok(Some((target_window_id, target_pane_id)))
            }
            TargetKind::Window { window_id, .. } => {
                let target_pane_id = self.active_pane_for_target(window_id)?;
                Ok(Some((window_id.clone(), target_pane_id)))
            }
            TargetKind::Pane {
                window_id, pane_id, ..
            } => Ok(Some((window_id.clone(), pane_id.clone()))),
        }
    }

    fn active_pane_for_target(&self, target: &str) -> Result<String> {
        self.run(["display-message", "-p", "-t", target, "#{pane_id}"])
            .map(|output| output.trim().to_owned())
    }

    fn window_id_for_pane(&self, pane_id: &str) -> Result<String> {
        self.run(["display-message", "-p", "-t", pane_id, "#{window_id}"])
            .map(|output| output.trim().to_owned())
    }

    fn run_or_empty<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        match self.run(args) {
            Ok(output) => Ok(output),
            Err(error) if is_no_server_error(&error) => Ok(String::new()),
            Err(error) => Err(error),
        }
    }

    fn run_with_config<const N: usize>(&self, args: [&str; N]) -> Result<String> {
        let mut command = Command::new("tmux");
        command.arg("-f").arg(&self.managed.tmux_conf).args(args);
        let output = command.output().with_context(|| {
            format!(
                "failed to execute tmux with {}",
                self.managed.tmux_conf.display()
            )
        })?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(anyhow!(stderr))
        }
    }
}

fn run_command<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(anyhow!(stderr))
    }
}

fn is_no_server_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("no server running") || message.contains("failed to connect to server")
}

fn archive_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = clean.trim_matches('-').chars().take(80).collect::<String>();
    if trimmed.is_empty() {
        String::from("archive")
    } else {
        trimmed
    }
}

fn parse_sessions(raw: &str) -> Vec<Session> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            Some(Session {
                id: parts.next()?.to_owned(),
                name: parts.next()?.to_owned(),
                attached: parts.next()? == "1",
                windows: Vec::new(),
            })
        })
        .collect()
}

fn parse_windows(raw: &str) -> Vec<Window> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            Some(Window {
                session_id: parts.next()?.to_owned(),
                id: parts.next()?.to_owned(),
                name: parts.next()?.to_owned(),
                active: parts.next()? == "1",
                panes: Vec::new(),
            })
        })
        .collect()
}

fn parse_panes(raw: &str) -> Vec<Pane> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let window_id = parts.next()?.to_owned();
            let id = parts.next()?.to_owned();
            parts.next()?;
            Some(Pane {
                window_id,
                id,
                current_command: parts.next()?.to_owned(),
                current_path: parts.next()?.to_owned(),
                active: parts.next()? == "1",
                zoomed: parts.next()? == "1",
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_panes, parse_sessions, parse_windows};

    #[test]
    fn parses_tmux_snapshot_rows() {
        let sessions = parse_sessions("$1\tdev\t1\n");
        let windows = parse_windows("$1\t@1\teditor\t1\n");
        let panes = parse_panes("@1\t%1\tmain\tnvim\t/Users/me/project\t1\t0\n");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "dev");
        assert!(sessions[0].attached);

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].session_id, "$1");
        assert_eq!(windows[0].name, "editor");
        assert!(windows[0].active);

        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].window_id, "@1");
        assert_eq!(panes[0].current_command, "nvim");
        assert_eq!(panes[0].current_path, "/Users/me/project");
        assert!(panes[0].active);
        assert!(!panes[0].zoomed);
    }

    #[test]
    fn keeps_tmux_pane_listing_order() {
        let panes = parse_panes("@1\t%10\tmain\tzsh\t/tmp\t1\t0\n@1\t%2\tmain\tzsh\t/tmp\t0\t0\n");

        assert_eq!(panes[0].id, "%10");
        assert_eq!(panes[1].id, "%2");
    }

    #[test]
    fn keeps_tmux_window_listing_order() {
        let windows = parse_windows("$1\t@10\tzsh\t1\n$1\t@2\tagent\t0\n");

        assert_eq!(windows[0].id, "@10");
        assert_eq!(windows[0].name, "zsh");
        assert_eq!(windows[1].id, "@2");
        assert_eq!(windows[1].name, "agent");
    }
}
