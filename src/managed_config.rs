use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

const DEFAULT_SHOW_HINTS: bool = true;
const DEFAULT_SHOW_STATUS: bool = true;
const DEFAULT_SIDEBAR_PERCENT: u8 = 24;

#[derive(Clone, Debug)]
pub struct Settings {
    pub show_hints: bool,
    pub show_status: bool,
    pub sidebar_percent: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_hints: DEFAULT_SHOW_HINTS,
            show_status: DEFAULT_SHOW_STATUS,
            sidebar_percent: DEFAULT_SIDEBAR_PERCENT,
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
            _ => {}
        }
    }

    Ok(settings)
}

fn render_settings(settings: &Settings) -> String {
    format!(
        "show_hints={}\nshow_status={}\nsidebar_percent={}\n",
        settings.show_hints, settings.show_status, settings.sidebar_percent
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
        r##"set -g prefix C-a
unbind C-b
bind C-a send-prefix

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
set -g detach-on-destroy off
set -g set-clipboard on
set -g default-terminal "screen-256color"
set -as terminal-features ",xterm-256color:RGB,screen-256color:RGB,tmux-256color:RGB"
set -ga update-environment "NVIM"
"##,
    );
    tmux_conf.push_str(status_lines);
    tmux_conf.push_str(
        r##"bind -n C-h if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-h' 'select-pane -L'
bind -n C-j if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-j' 'select-pane -D'
bind -n C-k if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-k' 'select-pane -U'
bind -n C-l if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-l' 'select-pane -R'
bind -n C-q run-shell 'tmux set-option -gq @tmuxtui-return "#{session_id} #{window_id} #{pane_id}"' \; detach-client
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
        };

        assert_eq!(
            render_settings(&settings),
            "show_hints=false\nshow_status=true\nsidebar_percent=24\n"
        );
    }

    #[test]
    fn renders_tmux_status_line_when_enabled() {
        let settings = Settings {
            show_hints: true,
            show_status: true,
            sidebar_percent: 24,
        };

        let tmux_conf = render_tmux_conf(&settings);
        assert!(tmux_conf.contains("set -g status on"));
        assert!(tmux_conf.contains("status-left"));
        assert!(tmux_conf.contains("status-right"));
    }
}
