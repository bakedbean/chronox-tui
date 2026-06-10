# Grouped-by-file Timeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace chronox's flat, newest-first left pane with a grouped-by-file accordion timeline (file headers + nested edit rows, magnitude gauges, derived line counts), plus a status strip, single-frame split, and updated footer.

**Architecture:** Pure builders in `render.rs` (gauge, line counts, header/edit `Line`s); grouping + accordion + visible-row navigation + a memoized count cache in `app.rs` with a semantic-first selection that survives rebuilds; `ui.rs` iterates the visible-row list and draws the chrome. The right-pane diff is untouched.

**Tech Stack:** Rust, ratatui 0.29, crossterm, `sessionx` (Timeline / nav / change_detail_diff / syntax).

**Reference:** spec at `docs/superpowers/specs/2026-06-10-grouped-by-file-timeline-design.md`; design package at `~/Documents/chronox/design_handoff_grouped_timeline`.

---

## File structure

- `src/render.rs` — add `stat_bar`, `change_counts`, `header_line`, `edit_line`; remove `entry_lines`. Pure, no `App` dependency.
- `src/app.rs` — add `FileGroup`, `VisibleRow`, `SelTarget`; `build_groups`, `build_visible` (pure); `rebuild`, count memoization, semantic selection/repin, visible-row nav, totals/spinner accessors. New `App` fields.
- `src/ui.rs` — `render_list` over `visible`; status strip; single-frame split + titles; footer text.
- `src/input.rs` — unchanged (no new keys).

All tests are inline `#[cfg(test)]` modules. Run with `cargo test` from the worktree root `/home/eben/.local/state/wsx/worktrees/chronox/glossy-juniper`.

---

## Task 1: Magnitude gauge (`stat_bar`)

**Files:**
- Modify: `src/render.rs` (add fn + test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/render.rs`:

```rust
#[test]
fn stat_bar_splits_green_red_and_pads_empty() {
    // all adds -> 4 green, 0 red, 0 empty
    let b = stat_bar(10, 0, 4);
    assert_eq!(b[0].content.as_ref(), "▰▰▰▰");
    assert_eq!(b[0].style.fg, Some(Color::Green));
    assert_eq!(b[1].content.as_ref(), "");
    assert_eq!(b[2].content.as_ref(), "");

    // mixed -> at least one of each, total width 4
    let b = stat_bar(3, 1, 4);
    let g = b[0].content.chars().count();
    let r = b[1].content.chars().count();
    let e = b[2].content.chars().count();
    assert_eq!(g + r + e, 4);
    assert!(g >= 1 && r >= 1, "both sides represented when both nonzero");
    assert_eq!(b[1].style.fg, Some(Color::Red));
    assert_eq!(b[2].style.fg, Some(Color::DarkGray));

    // nothing -> all empty/faint
    let b = stat_bar(0, 0, 4);
    assert_eq!(b[2].content.chars().count(), 4);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet stat_bar`
Expected: FAIL — `cannot find function stat_bar`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/render.rs` (after `style_for`, before `entry_lines`):

```rust
/// Fixed-width magnitude bar: `add` green cells + `del` red cells in a
/// `width`-cell gauge, the remainder faint `▱`. Mirrors the design's `statBar`.
pub fn stat_bar(add: u32, del: u32, width: usize) -> Vec<Span<'static>> {
    let total = (add + del).max(1) as f64;
    let mut g = ((add as f64 / total) * width as f64).round() as usize;
    let mut r = ((del as f64 / total) * width as f64).round() as usize;
    if add > 0 && g == 0 {
        g = 1;
    }
    if del > 0 && r == 0 {
        r = 1;
    }
    while g + r > width {
        if r > g {
            r -= 1;
        } else {
            g -= 1;
        }
    }
    let empty = width - g - r;
    vec![
        Span::styled("▰".repeat(g), Style::default().fg(Color::Green)),
        Span::styled("▰".repeat(r), Style::default().fg(Color::Red)),
        Span::styled("▱".repeat(empty), Style::default().fg(Color::DarkGray)),
    ]
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet stat_bar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/render.rs
git commit -m "feat: add magnitude gauge (stat_bar) for grouped timeline"
```

---

## Task 2: Per-change line counts (`change_counts`)

**Files:**
- Modify: `src/render.rs` (add fn + test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/render.rs`:

```rust
#[test]
fn change_counts_counts_added_and_removed() {
    use sessionx::event::ChangeDetail;
    assert_eq!(
        change_counts(&ChangeDetail::Edit {
            old: "a\nb".into(),
            new: "x".into()
        }),
        (1, 2),
        "1 added line, 2 removed lines"
    );
    assert_eq!(
        change_counts(&ChangeDetail::Write {
            head: "a\nb\nc".into()
        }),
        (3, 0)
    );
    assert_eq!(change_counts(&ChangeDetail::None), (0, 0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet change_counts`
Expected: FAIL — `cannot find function change_counts`.

- [ ] **Step 3: Write minimal implementation**

The import line already pulls `DiffMarker` and `change_detail_diff`. Add to `src/render.rs`:

```rust
/// Count added vs removed lines for a change by running its bounded `detail`
/// through `change_detail_diff` and tallying markers. `base_line`/`lang` do not
/// affect counts, so we pass neutral values. Source A in the design (no I/O).
pub fn change_counts(detail: &sessionx::event::ChangeDetail) -> (u32, u32) {
    let mut add = 0;
    let mut del = 0;
    for dl in change_detail_diff(detail, 1, None) {
        match dl.marker {
            DiffMarker::Added => add += 1,
            DiffMarker::Removed => del += 1,
        }
    }
    (add, del)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet change_counts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/render.rs
git commit -m "feat: derive per-change line counts from change_detail_diff"
```

---

## Task 3: File grouping (`FileGroup` + `build_groups`)

**Files:**
- Modify: `src/app.rs` (add types + fn + test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/app.rs`:

```rust
#[test]
fn build_groups_orders_by_first_seen_and_rolls_up() {
    // newest-first input (as sessionx hands us): a, a, b, write-c
    let events = vec![
        ev(4, "/wt/a.rs", 1),
        ev(3, "/wt/a.rs", 2),
        ev(2, "/wt/b.rs", 3),
        write_ev(1, "/wt/c.rs", 4),
    ];
    // per-event (add, del)
    let counts = vec![(10, 1), (4, 0), (2, 2), (58, 0)];
    let groups = build_groups(&events, &counts);

    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].file, PathBuf::from("/wt/a.rs"));
    assert_eq!(groups[0].event_idxs, vec![0, 1]);
    assert_eq!((groups[0].add, groups[0].del), (14, 1));
    assert!(!groups[0].is_new);

    assert_eq!(groups[1].file, PathBuf::from("/wt/b.rs"));
    assert_eq!((groups[1].add, groups[1].del), (2, 2));

    assert_eq!(groups[2].file, PathBuf::from("/wt/c.rs"));
    assert!(groups[2].is_new, "single Write -> new file");
}
```

Also add a `write_ev` helper next to the existing `ev` helper in that test module:

```rust
fn write_ev(ts: i64, file: &str, line_index: usize) -> ChangeEvent {
    ChangeEvent {
        timestamp_ms: ts,
        tool: ChangeTool::Write,
        file_path: PathBuf::from(file),
        summary: String::new(),
        detail: ChangeDetail::Write { head: "x".into() },
        source: ChangeSource {
            session_file: PathBuf::from("s.jsonl"),
            line_index,
            index_in_line: 0,
        },
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet build_groups`
Expected: FAIL — `cannot find type FileGroup` / `cannot find function build_groups`.

- [ ] **Step 3: Write minimal implementation**

Add near the top of `src/app.rs` (after the `AppAction` enum, before `struct App`):

```rust
/// One file's worth of changes, newest-first, with rolled-up line counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGroup {
    pub file: PathBuf,
    pub event_idxs: Vec<usize>, // newest-first, into App.events
    pub add: u32,
    pub del: u32,
    pub is_new: bool, // single Write -> " new" tag
}

/// A row in the rendered list: a file header (always shown) or an edit row
/// (shown only under the active/expanded file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleRow {
    Header { group: usize }, // index into `groups`
    Edit { event: usize },   // index into App.events
}

/// Group `events` by `file_path`, preserving first-seen order. Because the
/// timeline is newest-first, this yields most-recently-touched file first with
/// edits newest-first inside each group. `counts[i]` is event `i`'s (add, del).
fn build_groups(events: &[ChangeEvent], counts: &[(u32, u32)]) -> Vec<FileGroup> {
    let mut groups: Vec<FileGroup> = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let (a, d) = counts.get(i).copied().unwrap_or((0, 0));
        match groups.iter_mut().find(|g| g.file == ev.file_path) {
            Some(g) => {
                g.event_idxs.push(i);
                g.add += a;
                g.del += d;
                g.is_new = false; // more than one change -> not a fresh new file
            }
            None => groups.push(FileGroup {
                file: ev.file_path.clone(),
                event_idxs: vec![i],
                add: a,
                del: d,
                is_new: ev.tool == ChangeTool::Write,
            }),
        }
    }
    groups
}
```

Add `ChangeTool` to the existing `sessionx` import in `app.rs` (it currently imports `ChangeEvent, ChangeSource, ...`):

```rust
use sessionx::{
    ChangeEvent, ChangeSource, ChangeTool, NavAction, NavKey, SideRow, Timeline,
    change_detail_side_by_side, lang_for_path,
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet build_groups`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: group change events by file with rolled-up counts"
```

---

## Task 4: Accordion visible-row builder (`build_visible`)

**Files:**
- Modify: `src/app.rs` (add fn + test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/app.rs`:

```rust
#[test]
fn build_visible_expands_only_active_group() {
    let events = vec![
        ev(3, "/wt/a.rs", 1),
        ev(2, "/wt/a.rs", 2),
        ev(1, "/wt/b.rs", 3),
    ];
    let counts = vec![(1, 0), (1, 0), (1, 0)];
    let groups = build_groups(&events, &counts);

    // active = group 1 (b.rs): headers for both files, b's single edit nested.
    let vis = build_visible(&groups, 1);
    assert_eq!(
        vis,
        vec![
            VisibleRow::Header { group: 0 },
            VisibleRow::Header { group: 1 },
            VisibleRow::Edit { event: 2 },
        ]
    );

    // active = group 0 (a.rs): a's two edits nested, b folded.
    let vis = build_visible(&groups, 0);
    assert_eq!(
        vis,
        vec![
            VisibleRow::Header { group: 0 },
            VisibleRow::Edit { event: 0 },
            VisibleRow::Edit { event: 1 },
            VisibleRow::Header { group: 1 },
        ]
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet build_visible`
Expected: FAIL — `cannot find function build_visible`.

- [ ] **Step 3: Write minimal implementation**

Add to `src/app.rs` after `build_groups`:

```rust
/// Flatten groups into the visible-row sequence: every header in order, with
/// the active group's edit rows inserted directly under its header.
fn build_visible(groups: &[FileGroup], active: usize) -> Vec<VisibleRow> {
    let mut out = Vec::new();
    for (gi, g) in groups.iter().enumerate() {
        out.push(VisibleRow::Header { group: gi });
        if gi == active {
            for &event in &g.event_idxs {
                out.push(VisibleRow::Edit { event });
            }
        }
    }
    out
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet build_visible`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat: build accordion visible-row list from groups"
```

---

## Task 5: Rewire `App` to the visible-row model

This is the central state change. `selected` becomes an index into `visible`; a semantic target survives rebuilds; the diff pane resolves the selected row to an event. Several steps, one commit (intermediate states would not compile). Write the updated tests first so the compile failures guide the change.

**Files:**
- Modify: `src/app.rs` (fields, `rebuild`, accessors, nav, refresh, tests)

- [ ] **Step 1: Add the new fields and a semantic-selection target**

In `src/app.rs`, add this enum after `VisibleRow`:

```rust
/// What the cursor is "on", independent of row indices, so selection survives a
/// rebuild that expands/collapses rows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SelTarget {
    File(PathBuf),
    Edit(ChangeSource),
}
```

Add fields to `struct App` (after `events: Vec<ChangeEvent>,`):

```rust
    groups: Vec<FileGroup>,
    visible: Vec<VisibleRow>,
    pub active_group: usize,
    counts: std::collections::HashMap<ChangeSource, (u32, u32)>,
    pub spinner_frame: usize,
```

Initialise them in `App::bare` (after `events: Vec::new(),`):

```rust
            groups: Vec::new(),
            visible: Vec::new(),
            active_group: 0,
            counts: std::collections::HashMap::new(),
            spinner_frame: 0,
```

- [ ] **Step 2: Add count memoization + the rebuild + accessors**

Add these methods inside an `impl App` block in `src/app.rs`:

```rust
    /// Memoized (add, del) for event `idx`, computed from its bounded detail.
    fn ensure_count(&mut self, idx: usize) {
        if let Some(ev) = self.events.get(idx) {
            let src = ev.source.clone();
            if !self.counts.contains_key(&src) {
                let c = crate::render::change_counts(&ev.detail);
                self.counts.insert(src, c);
            }
        }
    }

    /// Read cached counts for event `idx` (0,0 if absent — should not happen
    /// after `rebuild`, which populates every event).
    pub fn event_counts(&self, idx: usize) -> (u32, u32) {
        self.events
            .get(idx)
            .and_then(|ev| self.counts.get(&ev.source).copied())
            .unwrap_or((0, 0))
    }

    /// The semantic target currently under the cursor, read from current state.
    fn current_target(&self) -> Option<SelTarget> {
        match self.visible.get(self.selected)? {
            VisibleRow::Header { group } => {
                Some(SelTarget::File(self.groups.get(*group)?.file.clone()))
            }
            VisibleRow::Edit { event } => {
                Some(SelTarget::Edit(self.events.get(*event)?.source.clone()))
            }
        }
    }

    /// Which group a target belongs to (0 if not found / no target).
    fn group_for_target(&self, target: &Option<SelTarget>) -> usize {
        match target {
            Some(SelTarget::File(p)) => {
                self.groups.iter().position(|g| &g.file == p).unwrap_or(0)
            }
            Some(SelTarget::Edit(src)) => self
                .groups
                .iter()
                .position(|g| {
                    g.event_idxs
                        .iter()
                        .any(|&i| self.events.get(i).map(|e| &e.source) == Some(src))
                })
                .unwrap_or(0),
            None => 0,
        }
    }

    /// Index in `visible` matching the target, if present.
    fn locate(&self, target: &Option<SelTarget>) -> Option<usize> {
        let target = target.as_ref()?;
        self.visible.iter().position(|row| match (row, target) {
            (VisibleRow::Header { group }, SelTarget::File(p)) => {
                self.groups.get(*group).map(|g| &g.file) == Some(p)
            }
            (VisibleRow::Edit { event }, SelTarget::Edit(src)) => {
                self.events.get(*event).map(|e| &e.source) == Some(src)
            }
            _ => false,
        })
    }

    /// Rebuild groups + visible from the current events, keeping the cursor on
    /// the same semantic target (accordion: the target's file is active).
    fn rebuild(&mut self) {
        let target = self.current_target();
        // Populate the count cache for every event (memoized; cheap after first).
        for i in 0..self.events.len() {
            self.ensure_count(i);
        }
        let counts: Vec<(u32, u32)> =
            (0..self.events.len()).map(|i| self.event_counts(i)).collect();
        self.groups = build_groups(&self.events, &counts);
        self.active_group = self.group_for_target(&target);
        self.visible = build_visible(&self.groups, self.active_group);
        self.selected = self
            .locate(&target)
            .unwrap_or_else(|| self.selected.min(self.visible.len().saturating_sub(1)));
    }

    /// Event index the selected row resolves to: an edit's own event, or a
    /// header's group's newest event.
    pub fn selected_event_idx(&self) -> Option<usize> {
        match self.visible.get(self.selected)? {
            VisibleRow::Edit { event } => Some(*event),
            VisibleRow::Header { group } => {
                self.groups.get(*group)?.event_idxs.first().copied()
            }
        }
    }

    pub fn groups(&self) -> &[FileGroup] {
        &self.groups
    }

    pub fn visible(&self) -> &[VisibleRow] {
        &self.visible
    }

    /// Session line-count totals across all groups.
    pub fn session_totals(&self) -> (u32, u32) {
        self.groups
            .iter()
            .fold((0, 0), |(a, d), g| (a + g.add, d + g.del))
    }
```

- [ ] **Step 3: Route the diff pane through the resolved event**

In `src/app.rs`, change `selected_event` to use the resolver:

```rust
    pub fn selected_event(&self) -> Option<&ChangeEvent> {
        self.events.get(self.selected_event_idx()?)
    }
```

In `selected_path_and_line`, replace `let ev = self.events.get(self.selected)?;` with:

```rust
        let ev = self.events.get(self.selected_event_idx()?)?;
```

In `diff_lines` and `diff_side_rows`, replace each `self.events.get(self.selected)` (there are occurrences for both `src` lookup and the `build_*` call) with `self.events.get(self.selected_event_idx()?)`-equivalent. Concretely, in each method change the two `self.selected` event lookups to use a local:

```rust
        let idx = self.selected_event_idx();
        let src = idx.and_then(|i| self.events.get(i)).map(|e| e.source.clone());
        // ...
                    let lines = idx
                        .and_then(|i| self.events.get(i))
                        .map(build_diff_lines)
                        .unwrap_or_default();
```

(and the analogous `build_side_rows` in `diff_side_rows`).

- [ ] **Step 4: Rewrite nav over `visible` + rebuild after moves**

Replace the body of `on_nav` in `src/app.rs` with:

```rust
    fn on_nav(&mut self, key: NavKey) {
        if self.focus == Focus::Diff {
            match key {
                NavKey::Up => return self.scroll_diff(-1),
                NavKey::Down => return self.scroll_diff(1),
                NavKey::Esc => {
                    self.should_quit = true;
                    return;
                }
                _ => {}
            }
        }
        let (new_sel, act) = nav(self.selected, key, self.visible.len());
        match act {
            NavAction::Exit => self.should_quit = true,
            NavAction::Open(_) => self.focus = Focus::Diff,
            NavAction::None => {}
        }
        if new_sel != self.selected {
            self.selected = new_sel;
            self.diff_scroll = 0;
            self.rebuild(); // re-derive active file + visible rows
        }
    }
```

- [ ] **Step 5: Rebuild on refresh + advance spinner on tick**

In `refresh`, replace the repin tail. The new `refresh`:

```rust
    fn refresh(&mut self) {
        let files = claude_session_files(&self.worktree);
        self.timeline.refresh(&files);
        self.events = self.timeline.events().to_vec();
        self.rebuild();
    }
```

(`rebuild` captures the semantic target from the *pre-refresh* `visible`/`groups`, which still reflect the old events, then re-resolves it against the new ones — this is the new repin. Note `rebuild` reads `current_target` before overwriting `groups`/`visible`.)

In `apply`, change the `Tick` arm to also advance the spinner:

```rust
            AppAction::Tick => {
                self.spinner_frame = self.spinner_frame.wrapping_add(1);
                self.refresh();
            }
```

Remove the now-unused `repin` free function and its `pinned` usage. Delete the three `repin_*` tests (`repin_keeps_same_event_when_new_events_prepended`, `repin_clamps_when_event_gone`, `repin_empty_is_zero`) — repin is now exercised through `rebuild` (covered below).

- [ ] **Step 6: Update the existing nav/selection tests for visible-row semantics**

In `src/app.rs` tests, the `set_events_for_test_pub` seam must rebuild. Change it to:

```rust
    #[cfg(test)]
    pub(crate) fn set_events_for_test_pub(&mut self, events: Vec<ChangeEvent>) {
        self.events = events;
        self.rebuild();
    }
```

Replace `list_focus_moves_selection` with a visible-row version:

```rust
    #[test]
    fn list_focus_moves_over_visible_rows() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev(3, "/wt/a.rs", 1),
            ev(2, "/wt/b.rs", 2),
            ev(1, "/wt/c.rs", 3),
        ]);
        app.focus = Focus::List;
        // Starts on a.rs header (active); its single edit is event 0.
        assert_eq!(app.selected_event_idx(), Some(0));
        app.apply(AppAction::Nav(NavKey::Down)); // onto a.rs's edit
        assert_eq!(app.selected_event_idx(), Some(0));
        app.apply(AppAction::Nav(NavKey::Bottom)); // last visible row -> c.rs header
        assert_eq!(app.selected_event_idx(), Some(2));
        app.apply(AppAction::Nav(NavKey::Top)); // back to a.rs header
        assert_eq!(app.selected_event_idx(), Some(0));
    }
```

Replace `moving_selection_resets_diff_scroll`:

```rust
    #[test]
    fn moving_selection_resets_diff_scroll() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)]);
        app.focus = Focus::List;
        app.diff_scroll = 7;
        app.apply(AppAction::Nav(NavKey::Down));
        assert_eq!(app.diff_scroll, 0);
    }
```

The `diff_focus_routes_arrows_to_scroll`, `scroll_diff_floors_at_zero`, `toggle_focus_flips`, `esc_and_quit_set_should_quit`, resize/nudge tests, `new_on_empty_worktree_has_no_events`, `diff_lines_*`, `diff_side_rows_*`, `selected_path_and_line_*`, `status_*`, and `default_diff_view_*` tests are unaffected by signature changes and stay as-is (they exercise `selected`/`diff_scroll` directly, which still work). Verify they still compile after the field changes.

- [ ] **Step 7: Add a repin-via-rebuild test**

Add to the `tests` module in `src/app.rs`:

```rust
    #[test]
    fn rebuild_keeps_cursor_on_same_edit_when_new_change_prepended() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev(2, "/wt/a.rs", 1), ev(1, "/wt/b.rs", 2)]);
        app.focus = Focus::List;
        app.apply(AppAction::Nav(NavKey::Down)); // onto a.rs's edit (event 0, source line 1)
        assert_eq!(app.selected_event_idx(), Some(0));
        // A newer change to a different file is prepended; a.rs's edit shifts down.
        app.set_events_for_test_pub(vec![
            ev(3, "/wt/new.rs", 9),
            ev(2, "/wt/a.rs", 1),
            ev(1, "/wt/b.rs", 2),
        ]);
        // Still pinned to the same a.rs change (now event index 1).
        assert_eq!(app.selected_event_idx(), Some(1));
    }
```

- [ ] **Step 8: Run the full app test module**

Run: `cargo test --quiet --bin chronox app::`
Expected: PASS (all `app` tests green). If `ev`/`write_ev` helpers report unused-import or unused-variant warnings, leave them — warnings are fine; only failures block.

- [ ] **Step 9: Commit**

```bash
git add src/app.rs
git commit -m "refactor: drive selection over grouped visible rows with semantic repin"
```

---

## Task 6: Header + edit `Line` builders

**Files:**
- Modify: `src/render.rs` (add `header_line`, `edit_line`, internal `finish`; tests)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src/render.rs`:

```rust
#[test]
fn header_line_has_caret_path_gauge_and_counts() {
    let line = header_line("src/app.rs", 16, 3, 2, false, true, true, 44, false);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("▾ src/app.rs"), "expanded caret + path");
    assert!(text.contains("+16"));
    assert!(text.contains("-3"));
    assert!(text.trim_end().ends_with("2"), "edit count right-aligned");
    assert_eq!(line.spans[0].style.fg, Some(Color::Cyan), "expanded caret cyan");
}

#[test]
fn folded_header_uses_folded_caret_and_no_del_when_zero() {
    let line = header_line("Cargo.toml", 1, 0, 1, false, false, false, 44, false);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("▸ Cargo.toml"), "folded caret");
    assert!(text.contains("+1"));
    assert!(!text.contains("-0"), "zero removals omitted");
}

#[test]
fn new_file_header_shows_new_tag() {
    let line = header_line("src/theme.rs", 58, 0, 1, true, false, false, 44, false);
    let new = line.spans.iter().find(|s| s.content.as_ref() == " new");
    assert!(new.is_some(), "single Write shows ' new'");
    assert_eq!(new.unwrap().style.fg, Some(Color::Blue));
}

#[test]
fn edit_line_connector_time_stats_and_summary() {
    let line = edit_line(0, 12, 3, "guard repin()", false, false, 44);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.starts_with("  ├ 00:00"), "indent, branch connector, time");
    assert!(text.contains("+12"));
    assert!(text.contains("-3"));
    assert!(text.contains("guard repin()"));
}

#[test]
fn last_edit_uses_corner_connector_and_selection_brightens() {
    let line = edit_line(0, 4, 0, "cache rows", true, true, 44);
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("└ "), "last edit uses corner connector");
    assert!(!text.contains("-0"), "zero removals omitted");
    // selected: every span carries the blue selection background.
    let bg = ratatui::style::Color::Rgb(0x24, 0x30, 0x49);
    assert!(
        line.spans.iter().all(|s| s.style.bg == Some(bg)),
        "selection bar fills the row"
    );
    // summary brightened to White when selected.
    let sum = line.spans.iter().find(|s| s.content.as_ref() == "cache rows").unwrap();
    assert_eq!(sum.style.fg, Some(Color::White));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --quiet header_line edit_line`
Expected: FAIL — `cannot find function header_line` / `edit_line`.

- [ ] **Step 3: Write the implementation**

Add to `src/render.rs` (after `stat_bar`/`change_counts`, near `entry_lines`):

```rust
/// Pad a span list to `width` columns; when `selected`, fill the row with the
/// blue selection background. Char-based width (matches `clip_line_to_width`).
fn finish(mut spans: Vec<Span<'static>>, width: u16, selected: bool) -> Line<'static> {
    let width = width as usize;
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    if selected {
        let bg = Color::Rgb(0x24, 0x30, 0x49);
        for s in &mut spans {
            s.style = s.style.bg(bg);
        }
    }
    Line::from(spans)
}

/// A grouped file-header row: `<caret><path>[ new]<pad><gauge> +A[ -D]<pad><count>`.
#[allow(clippy::too_many_arguments)]
pub fn header_line(
    file_rel: &str,
    add: u32,
    del: u32,
    count: usize,
    is_new: bool,
    expanded: bool,
    active: bool,
    width: u16,
    selected: bool,
) -> Line<'static> {
    let dim = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);
    let (caret, caret_style) = if expanded {
        ("▾ ", Style::default().fg(Color::Cyan))
    } else {
        ("▸ ", dim)
    };
    let path_style = if active {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Right block: gauge + " " + "+A" [+ " -D"] + "  count".
    let mut right = stat_bar(add, del, 4);
    right.push(Span::raw(" "));
    right.push(Span::styled(format!("+{add}"), Style::default().fg(Color::Green)));
    if del > 0 {
        right.push(Span::styled(format!(" -{del}"), Style::default().fg(Color::Red)));
    }
    right.push(Span::styled(format!("  {count}"), dim));
    let right_len: usize = right.iter().map(|s| s.content.chars().count()).sum();

    let new_len = if is_new { 4 } else { 0 }; // " new"
    let caret_len = 2;
    let budget = (width as usize)
        .saturating_sub(caret_len + new_len + right_len + 1);
    let path = abbreviate_path(file_rel, budget);

    let left_len = caret_len + path.chars().count() + new_len;
    let gap = (width as usize)
        .saturating_sub(left_len + right_len)
        .max(1);

    let mut spans = vec![
        Span::styled(caret, caret_style),
        Span::styled(path, path_style),
    ];
    if is_new {
        spans.push(Span::styled(" new", Style::default().fg(Color::Blue)));
    }
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    finish(spans, width, selected)
}

/// A nested edit row under the active file:
/// `  <connector> <HH:MM>  +a[ -d]  <summary>`.
pub fn edit_line(
    timestamp_ms: i64,
    add: u32,
    del: u32,
    summary: &str,
    last: bool,
    selected: bool,
    width: u16,
) -> Line<'static> {
    let faint = Style::default().fg(Color::DarkGray);
    let dim = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);
    let connector = if last { "└ " } else { "├ " };
    let summary_style = if selected {
        Style::default().fg(Color::White)
    } else {
        dim
    };

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(connector, faint),
        Span::styled(hhmm(timestamp_ms), dim),
        Span::raw("  "),
        Span::styled(format!("+{add}"), Style::default().fg(Color::Green)),
    ];
    if del > 0 {
        spans.push(Span::styled(format!(" -{del}"), Style::default().fg(Color::Red)));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(summary.to_string(), summary_style));
    finish(spans, width, selected)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --quiet header_line edit_line`
Expected: PASS (all five tests).

- [ ] **Step 5: Commit**

```bash
git add src/render.rs
git commit -m "feat: header and edit Line builders for grouped timeline"
```

---

## Task 7: Render the grouped list in `ui.rs`

**Files:**
- Modify: `src/ui.rs` (`render_list`), then remove `entry_lines` + its tests from `src/render.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/ui.rs` (helpers `ev`, `draw_app`, `buffer_text` already exist):

```rust
    fn ev_named(file: &str, ts: i64, line_index: usize) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: ts,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: "tweak the thing".into(),
            detail: ChangeDetail::Edit {
                old: "old".into(),
                new: "new".into(),
            },
            source: ChangeSource {
                session_file: PathBuf::from("s.jsonl"),
                line_index,
                index_in_line: 0,
            },
        }
    }

    #[test]
    fn list_shows_file_header_and_nested_edit() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev_named("/wt/src/app.rs", 0, 1),
            ev_named("/wt/src/ui.rs", 0, 2),
        ]);
        let buf = draw_app(&mut app, 100, 12);
        let text = buffer_text(&buf);
        assert!(text.contains("src/app.rs"), "file header rendered");
        assert!(text.contains("▾"), "active file expanded");
        assert!(text.contains("▸"), "other file folded");
        assert!(text.contains("tweak the thing"), "active file's edit summary shown");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet list_shows_file_header`
Expected: FAIL — `entry_lines`-based list does not render headers/`▾`.

- [ ] **Step 3: Rewrite `render_list`**

Replace the body of `render_list` in `src/ui.rs` with the version below, and update the imports at the top of `ui.rs` (replace `entry_lines` with the new builders):

```rust
use crate::render::{clip_line_to_width, edit_line, header_line, relative_display, side_cell_to_line};
use crate::app::{App, DiffView, Focus, VisibleRow};
```

```rust
fn render_list(f: &mut Frame, area: Rect, app: &mut App) {
    let block = pane_block("chronox · by file", app.focus == Focus::List);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = inner.height as usize;
    let len = app.visible().len();
    let scroll = clamp_scroll(
        adjust_scroll(app.list_scroll, app.selected, rows, len),
        len,
        rows,
    );
    app.list_scroll = scroll;
    app.last_visible_rows = rows;

    let sel = app.selected;
    let active = app.active_group;
    let width = inner.width;
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in app.visible().iter().enumerate().skip(scroll).take(rows) {
        let line = match *row {
            VisibleRow::Header { group } => {
                let g = &app.groups()[group];
                let rel = relative_display(&g.file, &app.worktree);
                header_line(
                    &rel,
                    g.add,
                    g.del,
                    g.event_idxs.len(),
                    g.is_new,
                    group == active,
                    group == active,
                    width,
                    i == sel,
                )
            }
            VisibleRow::Edit { event } => {
                let g = &app.groups()[active];
                let last = g.event_idxs.last() == Some(&event);
                let (add, del) = app.event_counts(event);
                let ev = &app.events()[event];
                edit_line(
                    ev.timestamp_ms,
                    add,
                    del,
                    &ev.summary,
                    last,
                    i == sel,
                    width,
                )
            }
        };
        lines.push(clip_line_to_width(&line, width as usize));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
```

Note: the borrow checker needs `app.groups()`/`app.events()`/`app.event_counts()` to be immutable borrows that don't overlap a mutable one. `render_list` takes `&mut App` only to write `list_scroll`/`last_visible_rows`; do those writes first (as above), then build `lines` using immutable accessors. If the compiler complains about borrowing `app` immutably while `lines: Vec<Line>` holds references, note that `header_line`/`edit_line` return owned `Line<'static>` and `relative_display` returns an owned `String`, so no borrow escapes the loop body — the only care is not holding `&app.groups()[..]` across a later `&mut` use, which this ordering avoids.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet list_shows_file_header`
Expected: PASS.

- [ ] **Step 5: Remove the obsolete `entry_lines`**

Delete `entry_lines` from `src/render.rs` and its tests (`entry_is_a_single_header_line`, `selected_entry_reverses_its_spans`). Keep `hhmm`, `relative_display`, `abbreviate_path`, `ellipsize_start`, `clip_line_to_width`, `side_cell_to_line` — all still used.

- [ ] **Step 6: Run the full suite**

Run: `cargo test --quiet`
Expected: PASS. (Some `ui.rs` two-pane tests still pass here — they are updated in Task 9.)

- [ ] **Step 7: Commit**

```bash
git add src/ui.rs src/render.rs
git commit -m "feat: render grouped-by-file accordion in the list pane"
```

---

## Task 8: Status strip

**Files:**
- Modify: `src/ui.rs` (`render_title` → `render_status_strip`; tests)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/ui.rs`:

```rust
    #[test]
    fn status_strip_shows_live_totals_and_file_count() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![
            ev_named("/wt/src/app.rs", 0, 1),
            ev_named("/wt/src/ui.rs", 0, 2),
        ]);
        let buf = draw_app(&mut app, 100, 12);
        let top: String = (0..100u16).map(|x| buf[(x, 0u16)].symbol()).collect();
        assert!(top.contains("chronox"));
        assert!(top.contains("live"));
        assert!(top.contains("changes"));
        assert!(top.contains("files"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet status_strip_shows_live`
Expected: FAIL — the old title line has none of `live`/`changes`/`files`.

- [ ] **Step 3: Replace `render_title`**

In `src/ui.rs`, rename the call site in `draw` from `render_title(f, title_area, app);` to `render_status_strip(f, title_area, app);` and replace the function:

```rust
const SPINNER: [&str; 9] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇"];

fn render_status_strip(f: &mut Frame, area: Rect, app: &App) {
    let green = Style::default().fg(Color::Green);
    let dim = Style::default().fg(Color::DarkGray).add_modifier(Modifier::DIM);
    let (add, del) = app.session_totals();
    let n = app.events().len();
    let m = app.groups().len();
    let spin = SPINNER[app.spinner_frame % SPINNER.len()];

    let line = Line::from(vec![
        Span::styled("● ", green),
        Span::styled("chronox  ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(app.worktree.display().to_string(), dim),
        Span::raw("   "),
        Span::styled(format!("{spin} live"), green),
        Span::styled(" · polling 1s", dim),
        Span::raw("   "),
        Span::styled(format!("+{add}"), green),
        Span::styled(format!(" -{del}"), Style::default().fg(Color::Red)),
        Span::raw("   "),
        Span::styled(format!("{n} changes · {m} files"), dim),
    ]);
    f.render_widget(Paragraph::new(line), area);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet status_strip_shows_live`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui.rs
git commit -m "feat: status strip with live spinner, totals, and counts"
```

---

## Task 9: Single-frame split with `┬`/`┴` divider

**Files:**
- Modify: `src/ui.rs` (`draw` body, new `render_frame`, drop `pane_block` from the body path; update separator/title tests)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/ui.rs` and update the two existing two-pane tests:

```rust
    #[test]
    fn single_frame_has_top_and_bottom_divider_junctions() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        app.list_width = 30;
        let buf = draw_app(&mut app, 80, 12);
        // Status strip is y=0; the body frame spans y=1..=10, footer y=11.
        // Divider column = body.x + list_width = 30.
        assert_eq!(buf[(30u16, 1u16)].symbol(), "┬", "top divider junction");
        assert_eq!(buf[(30u16, 10u16)].symbol(), "┴", "bottom divider junction");
        assert_eq!(buf[(30u16, 5u16)].symbol(), "│", "divider body");
    }

    #[test]
    fn frame_titles_label_both_panes() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        let buf = draw_app(&mut app, 80, 12);
        let top: String = (0..80u16).map(|x| buf[(x, 1u16)].symbol()).collect();
        assert!(top.contains("chronox · by file"), "left title");
        assert!(top.contains("main.rs"), "right title shows the file");
    }
```

Replace `two_pane_layout_places_separator_at_list_width` with `single_frame_has_top_and_bottom_divider_junctions` (delete the old one). Update `focus_indicator_colors_active_pane_border`: with the single frame the focus signal is on the title, not a per-pane corner. Replace it with:

```rust
    #[test]
    fn focused_pane_title_is_cyan() {
        let mut app = App::bare(PathBuf::from("/wt"));
        app.set_events_for_test_pub(vec![ev_named("/wt/src/main.rs", 0, 1)]);
        app.focus = Focus::List;
        let buf = draw_app(&mut app, 80, 12);
        // The left title 'chronox · by file' starts at column 3 of the top row.
        assert_eq!(buf[(3u16, 1u16)].fg, Color::Cyan);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --quiet single_frame frame_titles focused_pane_title`
Expected: FAIL — the body is still two `pane_block`s with a separate `│` separator and no `┬`/`┴`.

- [ ] **Step 3: Rewrite the body of `draw` + add `render_frame`**

In `src/ui.rs`, replace the body-rendering portion of `draw` (the `cols = Layout::horizontal(...)` block and the `render_list`/`render_separator`/`render_diff` calls) with:

```rust
    let (left, right) = render_frame(f, body, app);
    render_list_inner(f, left, app);
    render_diff_inner(f, right, app);
```

Add the frame renderer. It draws all four borders + titles + the divider column directly into the buffer, then returns the two inner content rects:

```rust
fn render_frame(f: &mut Frame, body: Rect, app: &App) -> (Rect, Rect) {
    let faint = Style::default().fg(Color::DarkGray);
    let list_focus = app.focus == Focus::List;
    let left_title_style = if list_focus {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let right_title_style = if list_focus {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    };

    let x0 = body.x;
    let y0 = body.y;
    let w = body.width;
    let h = body.height;
    let right_col = x0 + w - 1;
    // Divider column: list_width cells in from the left edge (matches the mouse
    // hit-test in input.rs, which uses last_area.x + list_width).
    let dx = (x0 + app.list_width).min(right_col.saturating_sub(1)).max(x0 + 1);

    let left_title = "chronox · by file";
    let right_title = match app.selected_event() {
        Some(ev) => format!(
            "{} · {}",
            relative_display(&ev.file_path, &app.worktree),
            ev.tool.label()
        ),
        None => "—".to_string(),
    };

    // ── top border ────────────────────────────────────────────────────────
    // Built as two fixed-width segments so the `┬` always lands exactly on the
    // divider column `dx` (matching the bottom border and the body divider),
    // regardless of title length. Titles are clipped to their segment width.
    //
    //   left segment  (exactly `left_seg_w` cols):  "┌─ " <title> " " ──…
    //   right segment (exactly `right_seg_w` cols): "┬─ " <title> " " ──… "┐"
    let left_seg_w = (dx - x0) as usize;
    let title_budget = left_seg_w.saturating_sub(4); // "┌─ " (3) + trailing " " (1)
    let lt: String = left_title.chars().take(title_budget).collect();
    let fill_left = left_seg_w.saturating_sub(3 + lt.chars().count() + 1);
    let mut top: Vec<Span> = vec![
        Span::styled("┌─ ", faint),
        Span::styled(lt, left_title_style),
        Span::styled(" ", faint),
        Span::styled("─".repeat(fill_left), faint),
    ];

    let right_seg_w = (right_col - dx + 1) as usize;
    let rtitle_budget = right_seg_w.saturating_sub(5); // "┬─ " (3) + " " (1) + "┐" (1)
    let rt: String = right_title.chars().take(rtitle_budget).collect();
    let fill_right = right_seg_w.saturating_sub(3 + rt.chars().count() + 1 + 1);
    top.push(Span::styled("┬─ ", faint));
    top.push(Span::styled(rt, right_title_style));
    top.push(Span::styled(" ", faint));
    top.push(Span::styled("─".repeat(fill_right), faint));
    top.push(Span::styled("┐", faint));
    f.render_widget(Paragraph::new(Line::from(top)), Rect::new(x0, y0, w, 1));

    // ── bottom border: └─...─┴─...─┘ ──────────────────────────────────────
    let mut bottom: Vec<Span> = vec![Span::styled("└", faint)];
    bottom.push(Span::styled("─".repeat((dx - x0 - 1) as usize), faint));
    bottom.push(Span::styled("┴", faint));
    bottom.push(Span::styled("─".repeat((right_col - dx - 1) as usize), faint));
    bottom.push(Span::styled("┘", faint));
    f.render_widget(
        Paragraph::new(Line::from(bottom)),
        Rect::new(x0, y0 + h - 1, w, 1),
    );

    // ── side + divider columns for the body rows ──────────────────────────
    let divider_style = if app.resizing {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        faint
    };
    for y in (y0 + 1)..(y0 + h - 1) {
        let left_edge: Vec<Line> = vec![Line::from(Span::styled("│", faint))];
        f.render_widget(Paragraph::new(left_edge.clone()), Rect::new(x0, y, 1, 1));
        f.render_widget(
            Paragraph::new(vec![Line::from(Span::styled("│", divider_style))]),
            Rect::new(dx, y, 1, 1),
        );
        f.render_widget(
            Paragraph::new(left_edge),
            Rect::new(right_col, y, 1, 1),
        );
    }

    let left = Rect::new(x0 + 1, y0 + 1, dx - x0 - 1, h - 2);
    let right = Rect::new(dx + 1, y0 + 1, right_col - dx - 1, h - 2);
    (left, right)
}
```

- [ ] **Step 4: Split `render_list`/`render_diff` into inner variants**

`render_list` and `render_diff` currently draw their own `pane_block`. Add inner variants that take the already-bordered inner rect. Rename the body of the existing `render_list` (from Task 7) so it no longer draws a block — extract everything after the block into `render_list_inner(f, inner, app)`:

```rust
fn render_list_inner(f: &mut Frame, inner: Rect, app: &mut App) {
    let rows = inner.height as usize;
    let len = app.visible().len();
    let scroll = clamp_scroll(
        adjust_scroll(app.list_scroll, app.selected, rows, len),
        len,
        rows,
    );
    app.list_scroll = scroll;
    app.last_visible_rows = rows;

    let sel = app.selected;
    let active = app.active_group;
    let width = inner.width;
    let mut lines: Vec<Line> = Vec::new();
    for (i, row) in app.visible().iter().enumerate().skip(scroll).take(rows) {
        let line = match *row {
            VisibleRow::Header { group } => {
                let g = &app.groups()[group];
                let rel = relative_display(&g.file, &app.worktree);
                header_line(
                    &rel, g.add, g.del, g.event_idxs.len(), g.is_new,
                    group == active, group == active, width, i == sel,
                )
            }
            VisibleRow::Edit { event } => {
                let g = &app.groups()[active];
                let last = g.event_idxs.last() == Some(&event);
                let (add, del) = app.event_counts(event);
                let ev = &app.events()[event];
                edit_line(ev.timestamp_ms, add, del, &ev.summary, last, i == sel, width)
            }
        };
        lines.push(clip_line_to_width(&line, width as usize));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
```

For the diff side, extract the block-drawing from `render_diff` into `render_diff_inner(f, inner, app)` (the inner already-bordered rect), keeping the `DiffView` dispatch:

```rust
fn render_diff_inner(f: &mut Frame, inner: Rect, app: &mut App) {
    match app.diff_view {
        DiffView::Block => render_diff_block(f, inner, app),
        DiffView::SideBySide => render_diff_side_by_side(f, inner, app),
    }
}
```

Delete the now-unused `render_list` (block-drawing wrapper from Task 7), `render_diff`, `render_separator`, and `pane_block` if nothing else references them. (`render_diff_block` / `render_diff_side_by_side` are unchanged and still used.) Verify with `cargo build` that there are no unused-function errors (unused warnings are acceptable, but remove dead `pane_block`/`render_separator` to keep it clean).

- [ ] **Step 5: Run the targeted tests**

Run: `cargo test --quiet single_frame frame_titles focused_pane_title`
Expected: PASS.

- [ ] **Step 6: Run the full suite**

Run: `cargo test --quiet`
Expected: PASS. The side-by-side / block diff tests (`side_by_side_shows_old_left_and_new_right`, `block_view_still_renders_after_toggle`) still hold because the diff inner rendering is unchanged.

- [ ] **Step 7: Commit**

```bash
git add src/ui.rs
git commit -m "feat: single-frame split with internal divider and pane titles"
```

---

## Task 10: Footer hints

**Files:**
- Modify: `src/ui.rs` (`render_footer`; update test)

- [ ] **Step 1: Update the failing test**

In `src/ui.rs` tests, replace `footer_advertises_the_edit_key` with the new hint string check:

```rust
    #[test]
    fn footer_lists_grouped_timeline_hints() {
        let mut app = App::bare(PathBuf::from("/wt"));
        let buf = draw_app(&mut app, 100, 10);
        let text = buffer_text(&buf);
        assert!(text.contains("enter diff"));
        assert!(text.contains("e edit"));
        assert!(text.contains("tab focus"));
        assert!(!text.contains("space fold"), "no space key in accordion-only mode");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --quiet footer_lists_grouped`
Expected: FAIL — current footer has no `enter diff`.

- [ ] **Step 3: Update `render_footer`**

In `src/ui.rs`, change the hint strings in `render_footer`:

```rust
        None => match app.focus {
            Focus::List => " ↑↓ move · enter diff · d view · e edit · tab focus · q quit ",
            Focus::Diff => {
                " ↑↓/PgUp/PgDn scroll · d view · e edit · tab focus list · q quit "
            }
        },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --quiet footer_lists_grouped`
Expected: PASS. Also confirm `footer_shows_status_when_set` still passes (status path unchanged) — it asserts the status replaces the hint and that `e edit` is absent while the status shows; that still holds.

- [ ] **Step 5: Commit**

```bash
git add src/ui.rs
git commit -m "feat: update footer hints for grouped timeline"
```

---

## Task 11: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Format + lint**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings`
Expected: clean (no errors). Fix any clippy findings inline and re-run.

- [ ] **Step 2: Full test suite**

Run: `cargo test --quiet`
Expected: all tests PASS.

- [ ] **Step 3: Manual smoke (optional but recommended)**

Run: `cargo run -- .` inside a worktree that has a Claude Code session, or use the `run` skill. Confirm: status strip with spinner; grouped headers with `▾`/`▸`, gauges, `+A`/`-D`, counts; arrow keys expand/collapse the accordion; selecting a header shows that file's newest change; the single frame draws with `┬`/`┴`; footer reads the new hints.

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "chore: fmt + clippy clean for grouped timeline"
```

(Skip if Steps 1–2 produced no changes.)

---

## Self-review notes (coverage map)

- Spec §1 state model → Tasks 3, 4, 5.
- Spec §2 grouping/ordering/accordion → Tasks 3, 4 (+ nav in 5).
- Spec §3 counts (source A) → Task 2 (+ memoization in 5).
- Spec §4 navigation + selected-event resolution → Task 5.
- Spec §5 live refresh / repin → Task 5 (`rebuild` on refresh, repin test).
- Spec §6 rendering (header/edit/gauge/selection bar) → Tasks 1, 6, 7.
- Spec §7 chrome (status strip, single frame, footer) → Tasks 8, 9, 10.
- Spec §8 testing → tests embedded throughout; full pass in Task 11.
- Out-of-scope items (B2/tree/filter/space) → not implemented, by design.
