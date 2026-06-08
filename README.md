# chronox-tui

A standalone [ratatui](https://ratatui.rs) terminal UI for
[chronox](https://github.com/bakedbean/chronox): browse the newest-first
timeline of file changes a Claude Code agent made in a worktree, with a
syntax-highlighted diff of the selected change. The timeline updates live while
a session is running.

## Run

```bash
cargo run                      # current directory
cargo run -- /path/to/worktree # an explicit worktree
```

The worktree must have Claude Code session logs
(`~/.claude/projects/<encoded-worktree>/*.jsonl`) — run a Claude Code session in
it (and make a few edits) first, or you'll see the empty state.

## Keys

| Key | Action |
|-----|--------|
| `↑`/`↓`, `k`/`j` | List: move selection · Diff: scroll one line |
| `PgUp`/`PgDn` | Diff: scroll a page |
| `g`/`G`, `Home`/`End` | List: jump to top / bottom |
| `Tab` | Toggle focus between the list and the diff |
| `Enter` | Focus the diff pane |
| `[` / `]` | Nudge the split divider left / right |
| mouse drag on the divider | Resize the split |
| mouse wheel over the diff | Scroll the diff |
| `q`, `Esc`, `Ctrl-C` | Quit |

## Note on mouse capture

The app captures the mouse (to support drag-resize), so your terminal's native
text selection needs the usual modifier (often `Shift`) while chronox-tui is
running.
