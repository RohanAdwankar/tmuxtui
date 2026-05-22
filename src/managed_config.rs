use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

const DEFAULT_SHOW_HINTS: bool = false;
const DEFAULT_SHOW_STATUS: bool = true;
const DEFAULT_SIDEBAR_PERCENT: u8 = 12;
const DEFAULT_SIDEBAR_AUTO: bool = false;

#[derive(Clone, Debug)]
pub struct Settings {
    pub show_hints: bool,
    pub show_status: bool,
    pub sidebar_percent: u8,
    pub sidebar_auto: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_hints: DEFAULT_SHOW_HINTS,
            show_status: DEFAULT_SHOW_STATUS,
            sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
            sidebar_auto: DEFAULT_SIDEBAR_AUTO,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ManagedConfig {
    pub tmux_conf: PathBuf,
    settings_path: PathBuf,
    settings: Settings,
}

impl ManagedConfig {
    pub fn bootstrap() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("could not resolve config directory")?
            .join("tmuxtui");
        fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "failed to create config directory at {}",
                config_dir.display()
            )
        })?;

        let settings_path = config_dir.join("settings.conf");
        let tmux_conf = config_dir.join("tmux.conf");
        let settings = read_settings(&settings_path)?;

        let managed = Self {
            tmux_conf,
            settings_path,
            settings,
        };
        managed.sync_files()?;
        Ok(managed)
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn archive_dir(&self) -> PathBuf {
        self.tmux_conf
            .parent()
            .map(|path| path.join("archive"))
            .unwrap_or_else(|| PathBuf::from("archive"))
    }

    pub fn set_show_hints(&mut self, show_hints: bool) -> Result<()> {
        self.settings.show_hints = show_hints;
        self.sync_files()
    }

    pub fn set_show_status(&mut self, show_status: bool) -> Result<()> {
        self.settings.show_status = show_status;
        self.sync_files()
    }

    pub fn set_sidebar_percent(&mut self, sidebar_percent: u8) -> Result<()> {
        self.settings.sidebar_percent = sidebar_percent.min(100);
        self.settings.sidebar_auto = false;
        self.sync_files()
    }

    pub fn set_sidebar_auto(&mut self) -> Result<()> {
        self.settings.sidebar_auto = true;
        self.sync_files()
    }

    fn sync_files(&self) -> Result<()> {
        fs::write(&self.settings_path, render_settings(&self.settings)).with_context(|| {
            format!(
                "failed to write settings config at {}",
                self.settings_path.display()
            )
        })?;
        fs::write(&self.tmux_conf, render_tmux_conf(&self.settings)).with_context(|| {
            format!(
                "failed to write managed tmux config at {}",
                self.tmux_conf.display()
            )
        })?;
        Ok(())
    }
}

fn read_settings(path: &PathBuf) -> Result<Settings> {
    let mut settings = Settings::default();
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(settings),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read settings config at {}", path.display()));
        }
    };

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let parsed = matches!(value.trim(), "true");
        match key.trim() {
            "show_hints" => settings.show_hints = parsed,
            "show_status" => settings.show_status = parsed,
            "sidebar_percent" => {
                if let Ok(percent) = value.trim().parse::<u8>() {
                    settings.sidebar_percent = percent.min(100);
                }
            }
            "sidebar_auto" => settings.sidebar_auto = parsed,
            _ => {}
        }
    }

    Ok(settings)
}

fn render_settings(settings: &Settings) -> String {
    format!(
        "show_hints={}\nshow_status={}\nsidebar_percent={}\nsidebar_auto={}\n",
        settings.show_hints, settings.show_status, settings.sidebar_percent, settings.sidebar_auto
    )
}

fn render_tmux_conf(settings: &Settings) -> String {
    let status_lines = if settings.show_status {
        r##"set -g status on
set -g status-position bottom
set -g status-justify left
set -g status-left-length 80
set -g status-right-length 120
set -g status-left "#S | #{window_name} | #{pane_current_command}"
set -g status-right "#{pane_current_path} | %H:%M"
"##
    } else {
        "set -g status off\n"
    };

    let mut tmux_conf = String::from(
        r##"set -g prefix C-g
unbind C-b
bind C-g send-prefix

set -g mouse on
set -g mode-keys vi
set -sg escape-time 10
set -g focus-events on
set -g history-limit 50000
set -g base-index 1
setw -g pane-base-index 1
set -g renumber-windows on
setw -g automatic-rename on
setw -g automatic-rename-format "#{pane_current_command}"
setw -g aggressive-resize on
set -g detach-on-destroy on
set -g set-clipboard on
set -g default-terminal "screen-256color"
set -s terminal-features "xterm*:clipboard:ccolour:cstyle:focus:title:RGB,screen*:title:RGB,tmux-256color:RGB,rxvt*:ignorefkeys"
set -g update-environment "DISPLAY KRB5CCNAME SSH_ASKPASS SSH_AUTH_SOCK SSH_AGENT_PID SSH_CONNECTION WINDOWID XAUTHORITY NVIM"
"##,
    );
    tmux_conf.push_str(status_lines);
    tmux_conf.push_str(
        r##"bind -n C-h if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-h' 'select-pane -L'
bind -n C-j if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-j' 'select-pane -D'
bind -n C-k if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-k' 'select-pane -U'
bind -n C-l if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-l' 'select-pane -R'
unbind -T copy-mode-vi MouseDragEnd1Pane
bind -T copy-mode-vi MouseDragEnd1Pane send-keys -X copy-pipe-and-cancel "pbcopy"
bind -T copy-mode-vi Enter send-keys -X copy-pipe-and-cancel "pbcopy"
bind -T copy-mode-vi y send-keys -X copy-pipe-and-cancel "pbcopy"
bind -n C-q run-shell "tmux set-option -gq @tmuxtui-session '#{session_id}'; tmux set-option -gq @tmuxtui-window '#{window_id}'; tmux set-option -gq @tmuxtui-pane '#{pane_id}'; tmux detach-client"
"##,
    );
    tmux_conf
}

#[cfg(test)]
mod tests {
    use super::{Settings, render_settings, render_tmux_conf};

    #[test]
    fn renders_settings_file() {
        let settings = Settings {
            show_hints: false,
            show_status: true,
            sidebar_percent: 24,
            sidebar_auto: true,
        };

        assert_eq!(
            render_settings(&settings),
            "show_hints=false\nshow_status=true\nsidebar_percent=24\nsidebar_auto=true\n"
        );
    }

    #[test]
    fn renders_tmux_status_line_when_enabled() {
        let settings = Settings {
            show_hints: true,
            show_status: true,
            sidebar_percent: 24,
            sidebar_auto: false,
        };

        let tmux_conf = render_tmux_conf(&settings);
        assert!(tmux_conf.contains("set -g status on"));
        assert!(tmux_conf.contains("status-left"));
        assert!(tmux_conf.contains("status-right"));
    }

    #[test]
    fn renders_prefix_that_leaves_readline_start_key_alone() {
        let tmux_conf = render_tmux_conf(&Settings::default());

        assert!(tmux_conf.contains("set -g prefix C-g"));
        assert!(tmux_conf.contains("bind C-g send-prefix"));
        assert!(!tmux_conf.contains("set -g prefix C-a"));
    }

    #[test]
    fn renders_detach_binding_that_persists_live_target() {
        let tmux_conf = render_tmux_conf(&Settings::default());

        assert!(tmux_conf.contains("@tmuxtui-session"));
        assert!(tmux_conf.contains("@tmuxtui-window"));
        assert!(tmux_conf.contains("@tmuxtui-pane"));
        assert!(tmux_conf.contains("detach-client"));
    }

    #[test]
    fn renders_idempotent_tmux_options() {
        let tmux_conf = render_tmux_conf(&Settings::default());

        assert!(tmux_conf.contains("set -s terminal-features"));
        assert!(tmux_conf.contains("xterm*:clipboard:ccolour:cstyle:focus:title:RGB"));
        assert!(tmux_conf.contains("screen*:title:RGB"));
        assert!(tmux_conf.contains("set -g update-environment"));
        assert!(!tmux_conf.contains("set -as terminal-features"));
        assert!(!tmux_conf.contains("set -ga update-environment"));
    }
}
