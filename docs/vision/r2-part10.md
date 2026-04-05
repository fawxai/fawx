
---

## Privacy and Threat Model

"Capture the minimum necessary causal trace" is the north star. This section defines what that means concretely, against named threats.

### Threat models

**1. Device compromise (malware, theft).** Raw screen captures are the highest-value target — they contain everything the user sees. Defense: encryption at rest, OS-level protections, session key rotation, and short retention. Data that is purged cannot be exfiltrated after the fact.

**2. Legal compulsion (subpoena, warrant).** Retention policy is the primary defense. Data that does not exist cannot be compelled. Default retention for raw captures: 7 days, configurable down to 24 hours. Derived training data (model weights, embeddings) are retained longer but are not reconstructible to raw input.

**3. User regret (opted in, wants out).** Full wipe surface for raw captures AND derived training data, distinct from the AX ripcord. Wipe is destructive and immediate — the observer returns to the general prior after wipe. The user should not have to trust that purging "really worked." The wipe operation should be verifiable: the system confirms the storage regions are zeroed or freed.

**4. Insider threat (compromised update, malicious skill).** The observer sidecar runs in a separate process with no network access. Training data never leaves the device through any Fawx-controlled path. Skills cannot access raw captures through the observer ABI — they only see the typed belief state. A malicious skill can observe what the observer publishes but cannot reach the underlying video, accessibility snapshots, or action logs.

### What "minimum necessary" means concretely

- **Screen video:** retained for the training pipeline, auto-purged after the retention window (default 7 days). Not accessible to skills or the reasoning layer at runtime.
- **Accessibility snapshots:** same retention as screen video. Used only for training-time supervision.
- **Action context:** abstract action type + target hint + timing + outcome. NOT raw keystrokes, NOT clipboard contents, NOT password field values. Sensitive fields (password inputs, private browsing) trigger capture pause or redaction.
- **Derived features:** latent representations and model weights are retained indefinitely but are not invertible to raw input. This is the key privacy property of training: the model learns patterns, not recordings.
- **Nothing is exported off-device** through any Fawx-controlled path by default.

### Capture pause and redaction

- **Automatic pause:** password fields focused, private/incognito windows, user-designated sensitive apps (configurable allowlist/blocklist)
- **Manual pause:** global hotkey, menu bar toggle, voice command
- **Retroactive redaction:** if capture was running during a sensitive moment, the user can purge a specific time range
- **Pause state is visible in the UI at all times.** The user should never wonder whether capture is running. The indicator is not hideable.

### Sensitive field handling

The action context logger must recognize sensitive input contexts:

- Password fields: no capture of any kind (video frame is either skipped or the field region is masked)
- Credit card / SSN fields: same treatment as password fields
- Private browsing windows: full capture pause (no video, no accessibility, no action context)
- User-designated apps: configurable per-app capture policy

This is not perfect. Some sensitive information will appear in non-sensitive contexts (e.g., someone types a password into a plain text field). The system cannot catch every case. The retention policy and wipe surface are the backstop.

### Privacy is not optional or deferrable

The capture pipeline and the privacy controls ship together. Bite 1 includes retention enforcement, pause/resume, and retroactive purge as exit criteria, not as follow-up work.
