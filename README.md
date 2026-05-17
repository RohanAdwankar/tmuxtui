# tmux-tui

https://github.com/user-attachments/assets/301d441f-d26a-40b3-9750-8c105c280dc2

`tmux-tui` gives you the key benefits of tmux with a simple UI and intuitive vim motions.

as of 0.2.1 pbcopy is added to the tmux config so only macs can use the copying unless you alias xclip on linux 

## Install

```
cargo install tmuxtui
```

## Commands

Run `tmuxtui --config` to rewrite and reload the managed tmux config without opening the TUI.

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
| `o` | create the next smaller item for the current selection |
| `O` | create a same-level item for the current selection |
| `x` | cut the selected window or pane |
| `p` | paste into the next smaller level for the current selection |
| `P` | paste into the same level as the current selection |
| `r` | rename the selected session, window, or pane |
| `d` | kill the selected session, selected pane, or first pane of a split window |
| `D` | kill the selected session or the full selected window |
| `a` | archive the selected session, selected pane, or first pane of a split window |
| `A` | archive the selected session or the full selected window |
| `s` | split the selected pane into top and bottom panes |
| `S` | split the selected pane into left and right panes |
| `z` | toggle zoom on the selected pane |
| `R` | refresh tmux state |
| `q` | quit `tmux-tui` |
| `Ctrl-q` | detach tmux and return to `tmux-tui` |

### Search And Command Line

| Keys | Action |
| --- | --- |
| `/` | start a non-filtering search over visible rows |
| `n` / `N` | jump to the next or previous search match |
| `f` | start filtering the visible tree |
| `:` | open the command line |
| `Enter` | confirm the current filter, prompt, or command |
| `Esc` | cancel the current filter, prompt, or command |
| `Backspace` | delete one character while typing |

### `:` Commands

| Command | Action |
| --- | --- |
| `:pin`, `:p` | pin the selected pane so future attaches join it on the right |
| `:unpin`, `:up` | clear the pinned pane |
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
| pinned pane | the tree shows `⌖` in the left marker column for the pinned pane |

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
