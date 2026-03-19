# UI Ship Checklist — New Features

Test each screen on **both macOS and iOS**. Every item must pass before we ship.

---

## 1. Fleet Dashboard

### Data Loading
- [ ] Screen loads without crash when Fawx server is running
- [ ] Shows "No fleet nodes registered" empty state when no nodes exist
- [ ] Shows fleet summary header (X nodes, Y online, Z stale)
- [ ] Node list populates with real data when nodes are registered
- [ ] Pull-to-refresh works (or auto-refresh fires)

### Node Display
- [ ] Each node shows: name, status badge, capabilities, last heartbeat
- [ ] Status badges correct colors: Online=green, Busy=orange, Stale=yellow, Offline=gray
- [ ] Last heartbeat shows relative time (e.g., "2 min ago"), not raw ms
- [ ] Tap node → detail sheet opens with full info

### Node Detail
- [ ] Shows endpoint, capabilities list, registration time
- [ ] "Dispatch Task" button visible (can be non-functional if no task UI yet)
- [ ] Back navigation works cleanly

### Error Handling
- [ ] Shows error state if server is unreachable (not a crash)
- [ ] Handles 401 unauthorized gracefully (expired/wrong token)
- [ ] Handles slow network (loading indicator shown)

---

## 2. Experiment Monitor

### Data Loading
- [ ] Screen loads without crash
- [ ] Shows "No experiments yet" empty state
- [ ] Experiment list shows most recent first
- [ ] Pull-to-refresh works

### Experiment List
- [ ] Each experiment shows: name/signal, status badge, created date
- [ ] Status badges: Running=blue, Completed=green, Failed=red, Cancelled=gray
- [ ] Running experiments show visual pulse/activity indicator
- [ ] Tap experiment → detail view opens

### Experiment Detail
- [ ] Shows signal info (name, description, severity)
- [ ] Shows timing (created, started, completed)
- [ ] Completed experiments show results table (candidates, scores, winner highlighted)
- [ ] Running experiments show "Stop" button
- [ ] Stop button actually calls POST /experiments/{id}/stop and updates status
- [ ] Failed experiments show error message

### Error Handling
- [ ] Graceful error if experiment ID not found (404)
- [ ] Handles server disconnect during detail view

---

## 3. Git Pane

### Status View
- [ ] Shows current branch name
- [ ] Shows clean/dirty indicator
- [ ] File list shows correct statuses (modified, added, deleted, untracked, renamed)
- [ ] Files grouped by staged/unstaged
- [ ] Empty state when repo is clean ("Working tree clean")

### Stage/Unstage
- [ ] Tap file → toggles staged/unstaged state
- [ ] "Stage All" button stages all files
- [ ] "Unstage All" button unstages all files  
- [ ] UI updates immediately after stage/unstage (optimistic or refetch)
- [ ] Staging an already-staged file is a no-op (doesn't error)

### Commit
- [ ] Commit message text field present
- [ ] "Commit" button disabled when message is empty
- [ ] "Commit" button disabled when no staged files
- [ ] Successful commit shows confirmation (toast or status update)
- [ ] Commit clears the message field and refreshes status
- [ ] Empty commit (nothing staged) shows appropriate error

### Push/Pull/Fetch
- [ ] Push button works, shows success/failure
- [ ] Pull button works, shows summary
- [ ] Pull with conflicts shows conflict indicator (conflicts: true)
- [ ] Fetch button works, shows summary
- [ ] All three show loading state during operation
- [ ] Push with no upstream shows meaningful error (not generic 400)

### Diff Viewer
- [ ] Shows raw diff text
- [ ] Monospace font
- [ ] Added lines visually distinct (green or + prefix)
- [ ] Removed lines visually distinct (red or - prefix)
- [ ] Empty diff shows "No changes" or similar

### Recent Commits
- [ ] Shows last N commits from /git/log
- [ ] Each commit shows: short hash, message, author, relative time
- [ ] Handles empty repo (no commits) gracefully

### Error Handling
- [ ] Not-a-git-repo shows clear message (not crash)
- [ ] Server unreachable → error state
- [ ] Large diff doesn't hang the UI (test with 1000+ line diff)

---

## 4. Telemetry Settings (after endpoints are wired)

### Consent UI
- [ ] Master toggle (on/off) — defaults to OFF
- [ ] 6 category toggles, each with description text
- [ ] Category toggles disabled when master is off
- [ ] Enabling master → categories become toggleable (but not auto-enabled)
- [ ] "Enable All" convenience button
- [ ] Changes persist (navigate away and back → same state)
- [ ] Changes hit PATCH /v1/telemetry/consent

### Privacy Info
- [ ] "What we collect" section explains categories in plain language
- [ ] "What we never collect" section present
- [ ] Links to privacy policy (once it exists)

---

## 5. Cross-Screen Integration

- [ ] New screens accessible from main navigation (tab bar / sidebar)
- [ ] Navigation between screens doesn't lose state
- [ ] Rotating device doesn't crash (iOS)
- [ ] Dark mode looks correct on all new screens
- [ ] Font sizes respect Dynamic Type / accessibility settings
- [ ] No hardcoded colors that break in dark mode

---

## 6. Performance / Polish

- [ ] No visible layout jumps on load (skeleton/loading states)
- [ ] Network calls don't block the main thread
- [ ] Scrolling is smooth in long lists (fleet nodes, experiments, git files)
- [ ] Memory usage doesn't spike with large data (50+ commits, 100+ files)
- [ ] App backgrounding and foregrounding doesn't lose state or crash

---

## Sign-off

| Screen | macOS | iOS | Tester | Date |
|--------|-------|-----|--------|------|
| Fleet Dashboard | ☐ | ☐ | | |
| Experiment Monitor | ☐ | ☐ | | |
| Git Pane | ☐ | ☐ | | |
| Telemetry Settings | ☐ | ☐ | | |
| Cross-Screen | ☐ | ☐ | | |
| Performance | ☐ | ☐ | | |

**Ship gate:** All boxes checked, both platforms. No exceptions.
