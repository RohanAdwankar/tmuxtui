# tmux-tui

https://github.com/user-attachments/assets/301d441f-d26a-40b3-9750-8c105c280dc2

`tmux-tui` gives you the key benefits of tmux with a simple UI and intuitive vim motions.

## Install

```
cargo install tmuxtui
```

## Commands

### Navigation

| Keys | Action |
| --- | --- |
| `j` / `Down` | move down |
| `k` / `Up` | move up |
| `gg` | jump to the first visible item |
| `G` | jump to the last visible item |
| `count` + `j` / `k` | move by a vim-style count, for example `5j` |
| `count` + `G` | jump to a specific visible row, for example `12G` |

### Session And Pane Actions

| Keys | Action |
| --- | --- |
| `Enter` | attach to the selected session, window, or pane |
| `n` / `O` | create a new session |
| `w` / `o` | create a new window in the selected session |
| `r` | rename the selected session, window, or pane |
| `d` | kill the selected session, window, or pane |
| `s` | split the selected pane into top and bottom panes |
| `S` | split the selected pane into left and right panes |
| `z` | toggle zoom on the selected pane |
| `R` | refresh tmux state |
| `q` | quit `tmux-tui` |
| `Ctrl-q` | detach tmux and return to `tmux-tui` |

### Search And Command Line

| Keys | Action |
| --- | --- |
| `/` | start filtering the visible tree |
| `:` | open the command line |
| `Enter` | confirm the current filter, prompt, or command |
| `Esc` | cancel the current filter, prompt, or command |
| `Backspace` | delete one character while typing |

### `:` Commands

| Command | Action |
| --- | --- |
| `:hidehints` | hide footer hints and keep the bottom bar command-oriented |
| `:showhints` | show footer hints again |
| `:hidestatus` | hide tmux's in-session status line |
| `:showstatus` | show tmux's in-session status line |

## Naming Behavior

| Situation | Behavior |
| --- | --- |
| new session with blank name | tmux auto-generates the session name |
| new window with blank name | tmux creates it unnamed and automatic rename uses the first real command |
| pane labels | panes are shown as `1`, `2`, and so on within each window |
| rename window to blank | the window falls back to the active pane command when possible |

## Defaults

The managed tmux config includes:

| Default | Effect |
| --- | --- |
| `Ctrl-h/j/k/l` pane navigation | works cleanly with vim and nvim panes |
| `Ctrl-q` detach | returns from tmux back into `tmux-tui` |
| vi mode keys | tmux copy mode behaves more like vim |
| mouse on | standard tmux mouse behavior inside tmux |
| automatic window rename | unnamed windows pick up the pane command |
| managed status line | can be toggled with `:showstatus` and `:hidestatus` |
