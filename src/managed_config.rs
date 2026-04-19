use std::{fs, path::PathBuf};

use anyhow::{Context, Result};

const MANAGED_TMUX_CONF: &str = r##"set -g prefix C-a
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
setw -g aggressive-resize on
set -g detach-on-destroy off
set -g status off
set -g set-clipboard on
set -g default-terminal "screen-256color"
set -as terminal-features ",xterm-256color:RGB,screen-256color:RGB,tmux-256color:RGB"
set -ga update-environment "NVIM"

bind -n C-h if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-h' 'select-pane -L'
bind -n C-j if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-j' 'select-pane -D'
bind -n C-k if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-k' 'select-pane -U'
bind -n C-l if-shell 'tmux display-message -p "#{m:#{pane_current_command},*vim}" | grep -q 1' 'send-keys C-l' 'select-pane -R'
"##;

#[derive(Clone, Debug)]
pub struct ManagedConfig {
    pub tmux_conf: PathBuf,
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

        let tmux_conf = config_dir.join("tmux.conf");
        let existing = fs::read_to_string(&tmux_conf).unwrap_or_default();
        if existing != MANAGED_TMUX_CONF {
            fs::write(&tmux_conf, MANAGED_TMUX_CONF).with_context(|| {
                format!(
                    "failed to write managed tmux config at {}",
                    tmux_conf.display()
                )
            })?;
        }

        Ok(Self { tmux_conf })
    }
}
