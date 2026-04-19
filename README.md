# tmux-tui

`tmux-tui` is a minimal tmux control surface for people who want tmux to feel closer to vim and closer to a single-screen tool.

It is built to make the common tmux flows easy without asking you to remember tmux prefix commands or maintain a separate pile of nvim-focused tmux tweaks.

## What The UI Shows

The left tree shows:

- sessions
- windows inside each session
- panes inside each window

The tree uses color to show state:

| Color Or Marker | Meaning |
| --- | --- |
| dark gray | default tree row |
| white | tmux-active row among sibling rows where active state matters |
| green highlight | the row currently selected in the TUI |
| pane `z` | the pane is zoomed |

Active-state color is only shown when there are sibling rows to disambiguate. For example, a lone pane in a window does not get a separate active marker.

The preview pane on the right shows:

- a status bar with the current session, window, command, and path
- a live capture of the selected pane

## Commands

### Navigation

| Keys | Action |
| --- | --- |
| `j` / `Down` | move down |
| `k` / `Up` | move up |
| `h` / `Left` | focus the tree |
| `l` / `Right` | focus the preview |
| `Tab` | toggle focus between tree and preview |
| `gg` | jump to the first visible item |
| `G` | jump to the last visible item |
| `count` + `j` / `k` | move by a vim-style count, for example `5j` |
| `count` + `G` | jump to a specific visible row, for example `12G` |

### Session And Pane Actions

| Keys | Action |
| --- | --- |
| `Enter` | attach to the selected session, window, or pane |
| `n` | create a new session |
| `w` | create a new window in the selected session |
| `r` | rename the selected session, window, or pane |
| `d` | kill the selected session, window, or pane |
| `s` | split the selected pane |
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
| `:showstus` | alias for `:showstatus` |

## Naming Behavior

| Situation | Behavior |
| --- | --- |
| new session with blank name | tmux auto-generates the session name |
| new window with blank name | tmux creates it unnamed and automatic rename uses the first real command |
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
