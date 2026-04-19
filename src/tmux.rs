use std::{
    ffi::OsStr,
    process::{Command, ExitStatus},
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
    pub title: String,
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

impl Tmux {
    pub fn new(managed: ManagedConfig) -> Self {
        Self { managed }
    }

    pub fn ensure_ready(&self) -> Result<()> {
        self.run_with_config(["start-server"])?;
        self.reload_config()?;
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
        for session in &mut sessions {
            session
                .windows
                .sort_by(|left, right| left.name.cmp(&right.name));
            for window in &mut session.windows {
                window.panes.sort_by(|left, right| left.id.cmp(&right.id));
            }
        }

        Ok(Snapshot { sessions })
    }

    pub fn capture_pane(&self, pane_id: &str) -> Result<String> {
        self.run(["capture-pane", "-J", "-p", "-t", pane_id, "-S", "-120"])
    }

    pub fn create_session(&self, name: &str) -> Result<()> {
        if name.is_empty() {
            self.run(["new-session", "-d"]).map(|_| ())
        } else {
            self.run(["new-session", "-d", "-s", name]).map(|_| ())
        }
    }

    pub fn rename_session(&self, session_id: &str, name: &str) -> Result<()> {
        self.run(["rename-session", "-t", session_id, name])
            .map(|_| ())
    }

    pub fn kill_session(&self, session_id: &str) -> Result<()> {
        self.run(["kill-session", "-t", session_id]).map(|_| ())
    }

    pub fn new_window(&self, session_id: &str, name: &str) -> Result<String> {
        let output = if name.is_empty() {
            self.run([
                "new-window",
                "-P",
                "-F",
                "#{window_id}",
                "-d",
                "-t",
                session_id,
            ])
        } else {
            self.run([
                "new-window",
                "-P",
                "-F",
                "#{window_id}",
                "-d",
                "-t",
                session_id,
                "-n",
                name,
            ])
        }?;
        Ok(output.trim().to_owned())
    }

    pub fn rename_window(&self, window_id: &str, name: &str) -> Result<()> {
        self.run(["rename-window", "-t", window_id, name])
            .map(|_| ())
    }

    pub fn kill_window(&self, window_id: &str) -> Result<()> {
        self.run(["kill-window", "-t", window_id]).map(|_| ())
    }

    pub fn split_pane(&self, pane_id: &str) -> Result<()> {
        self.run([
            "split-window",
            "-t",
            pane_id,
            "-c",
            "#{pane_current_path}",
            "-v",
        ])
        .map(|_| ())
    }

    pub fn rename_pane(&self, pane_id: &str, name: &str) -> Result<()> {
        self.run(["select-pane", "-t", pane_id, "-T", name])
            .map(|_| ())
    }

    pub fn kill_pane(&self, pane_id: &str) -> Result<()> {
        self.run(["kill-pane", "-t", pane_id]).map(|_| ())
    }

    pub fn toggle_zoom(&self, pane_id: &str) -> Result<()> {
        self.run(["resize-pane", "-t", pane_id, "-Z"]).map(|_| ())
    }

    pub fn attach(&self, target: &TargetKind) -> Result<()> {
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

    pub fn last_target(&self) -> Option<(String, String, String)> {
        let output = self.run(["show-options", "-gqv", "@tmuxtui-return"]).ok()?;
        let mut parts = output.split_whitespace();
        Some((
            parts.next()?.to_owned(),
            parts.next()?.to_owned(),
            parts.next()?.to_owned(),
        ))
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

    fn exec_attach<const N: usize>(&self, args: [&str; N]) -> Result<()> {
        let status = Command::new("tmux")
            .args(args)
            .status()
            .context("failed to execute tmux attach")?;
        if status.success() {
            Ok(())
        } else {
            Err(command_error("tmux", &status, "attach failed"))
        }
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> Result<String> {
        run_command("tmux", args)
    }

    fn reload_config(&self) -> Result<()> {
        self.run_or_empty([
            "source-file",
            self.managed.tmux_conf.to_string_lossy().as_ref(),
        ])?;
        Ok(())
    }

    fn run_or_empty<const N: usize>(&self, args: [&str; N]) -> Result<String> {
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

fn command_error(program: &str, status: &ExitStatus, context: &str) -> anyhow::Error {
    anyhow!("{program} exited with {status}: {context}")
}

fn is_no_server_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    message.contains("no server running") || message.contains("failed to connect to server")
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
            Some(Pane {
                window_id: parts.next()?.to_owned(),
                id: parts.next()?.to_owned(),
                title: parts.next()?.to_owned(),
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
}
