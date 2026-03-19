# Known Issues — Tracked During UI Ship Testing

Last updated: 2026-03-16

---

## Ship Blockers (RESOLVED)

- [x] **Server lock during experiments** — #1464 merged (background spawn)
- [x] **Push error UX** — structured error messages with human-readable suggestions
- [ ] **TUI experiments invisible to GUI** — background spawn prevents lock, but experiment registry integration still needed. Experiments started via TUI don't appear in Experiment Monitor.
- [ ] **Session lost on server restart during experiment** — partially mitigated by background spawn (server no longer crashes), but session persistence during long operations still needs work.

## Bugs Found During Testing

- [ ] **Fleet heartbeat "56 yr. ago"** — nodes with 0 ms heartbeat timestamp show epoch math instead of "Never". Fix: check for 0/null heartbeat and display "Never" or "No heartbeat received".
- [ ] **Fleet join fails — server binding** — MacBook couldn't join fleet from Tailscale network. `fawx fleet join` got "worker registration request failed: error sending request". May be a binding issue or URL mismatch. Needs investigation.
- [ ] **Fleet capabilities flag missing from CLI** — `fawx fleet add --capabilities` flag doesn't exist yet. Can't specify node capabilities during registration.
- [ ] **Fleet token exchange UX** — current flow requires copying long tokens between machines via terminal. Unusable for non-technical users.

## Missing Features (Backlog)

### Git Pane
- [ ] Side-by-side diff view
- [ ] Branch picker — needs `GET /git/branches` + `POST /git/checkout` endpoints
- [ ] Working directory setting in Settings
- [ ] Commit detail view (tap a commit to see full diff)
- [ ] `git push -u` equivalent (set upstream) from GUI

### Fleet Dashboard  
- [ ] GUI fleet node registration (QR/pairing code like device pairing)
- [ ] Node capability editing from GUI
- [ ] Remove node from GUI

### Experiment Monitor
- [ ] "New Experiment" button — create experiments from GUI
- [ ] Live progress updates during running experiments
- [ ] Wire TUI experiments into ExperimentRegistry so GUI can see them

### Telemetry
- [ ] Telemetry Settings UI (pending — endpoints on dev, Codex can build)

### Settings
- [ ] Working directory display + configuration
- [ ] Status bar should show configured Tailscale URL, not hardcoded 127.0.0.1

## UX Polish (Nice-to-haves)

- [ ] Push error overlay should be a compact toast, not a giant red bubble
- [ ] Stage/unstage could use swipe gestures (iOS)
- [ ] Experiment monitor auto-refresh during running experiments
- [ ] Fleet dashboard node detail — expandable capabilities chips
- [ ] Dark mode audit on all new screens
- [ ] Dynamic Type / accessibility audit on all new screens
