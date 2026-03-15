# Proposal Gate Hardening Specification

**Status:** DRAFT  
**Phase:** Phase 5  
**Priority:** Medium — UX work, depends on proposal gate being in the Swift app  
**Parent doc:** `docs/architecture/open-core-security-model.md`

---

## 1. Goal

Make the proposal gate resistant to social engineering attacks where the agent crafts proposals that mislead users into approving dangerous actions. The gate should make it easy to approve safe actions and hard to accidentally approve risky ones.

---

## 2. Problem

The proposal gate is the most realistic attack vector in the Fawx security model. The agent cannot bypass enforcement, but it can craft proposals that:

1. **Mislead through framing** — "I need to update your config" could mean anything
2. **Exploit approval fatigue** — users stop reading after approving 20 benign requests
3. **Hide in batches** — one dangerous action among many routine ones
4. **Escalate gradually** — each individual request seems reasonable, but the sequence achieves something the user wouldn't approve if asked directly

---

## 3. Risk-Tiered Approval UX

### Tier classification (compile-time)

Every path and operation is classified at compile time:

| Tier | Risk | Examples | UX treatment |
|------|------|---------|-------------|
| Routine | None | Read files, search memory, web requests | No gate (auto-approve) |
| Standard | Low | Write files in workspace, install signed skills | Single approve button |
| Elevated | Medium | System commands, config changes, unsigned skill operations | Highlighted panel + approve |
| Sensitive | High | Auth/credential operations, TIER2 path writes, network listener setup | Two-step confirmation |

### Two-step confirmation for Sensitive tier

1. First screen: "Fawx wants to [action]. This affects [resource]." + "Review Details" button
2. Second screen: Full diff/detail view + "I understand, approve" button with 3-second delay before it becomes clickable

The delay is not security theater — it forces the user to actually see the detail screen rather than muscle-memory tapping through.

---

## 4. Batch Approval Restrictions

### Problem
If Fawx submits 10 proposals at once, a user might "approve all" without reviewing each one.

### Rules

1. **No "approve all" for mixed tiers.** If a batch contains Standard and Elevated items, each Elevated item must be approved individually.
2. **No "approve all" for Sensitive tier.** Sensitive items always require individual two-step approval.
3. **Batch size limit.** Maximum 5 proposals in a single batch. More than 5 requires multiple submission rounds. This is a compiled const.
4. **Visual grouping.** Batch proposals show each item as a separate card with its own tier badge and approve/deny controls.

---

## 5. Proposal Context and Framing

### Required metadata

Every proposal must include:
- **Action verb** — what operation (write, execute, delete, modify, install)
- **Target path/resource** — exactly what's affected (full path, not summarized)
- **Diff preview** — for file writes, show the actual diff (not agent's description of it)
- **Tier badge** — visual indicator of risk level
- **Why** — agent's stated reason (displayed but clearly labeled as agent-provided, not system-verified)

### Anti-framing rules

1. The proposal UI renders the **actual operation**, not the agent's description of it. The agent can provide a "reason" field, but it's displayed separately and labeled "Agent's reason:" in a distinct visual style.
2. File paths are shown in full — no truncation, no `~/.../<file>` abbreviation in the detail view.
3. Shell commands are shown verbatim — no paraphrasing by the agent.

---

## 6. Escalation Detection

### Sequence monitoring (runtime, not compiled)

The proposal gate maintains a sliding window of recent approvals (last 10 minutes). If the sequence matches known escalation patterns, it adds a warning banner:

- **Path escalation** — requests gradually moving from safe dirs toward sensitive ones
- **Permission escalation** — read → write → execute on the same resource
- **Frequency spike** — sudden increase in proposal rate after a period of low activity

This is advisory — it adds a warning, not a block. The user can still approve. But the warning resets the fatigue counter and forces attention.

---

## 7. Swift App UI Requirements

### Approval sheet (iOS)

- Bottom sheet presentation (not alert — alerts are too easy to dismiss)
- Tier-colored header strip (green=standard, amber=elevated, red=sensitive)
- Scrollable diff/detail area
- Approve and Deny buttons at bottom, deny is default (left position on iOS)
- For Sensitive: "Review Details" intermediate step with 3-second delay on final approve

### Approval panel (macOS)

- Inspector-style panel or modal sheet
- Same tier coloring and layout as iOS
- Keyboard shortcuts: ⌘D = deny (default), ⌘A = approve (not default)
- For Sensitive: same two-step flow

### TUI

- Existing proposal gate UX, enhanced with:
- Tier-colored borders (green/amber/red)
- Batch display as numbered list with individual accept/reject
- `y` = approve, `n` = deny, `d` = show diff detail

---

## 8. Testing

### Unit tests
- Tier classification for all path/operation combinations
- Batch size enforcement (reject >5)
- Mixed-tier batch prevents "approve all" for elevated+
- Two-step flow enforced for Sensitive tier

### Integration tests
- End-to-end: agent submits proposal → user sees correct tier → approval/denial flows correctly
- Batch flow: 3 Standard + 1 Elevated → Elevated requires individual approval
- Sequence detection: escalating path pattern triggers warning

### Adversarial tests
- Agent submits misleading "reason" field — verify actual operation is displayed prominently
- Agent submits rapid-fire proposals — verify rate display and fatigue warning
- Agent submits a Sensitive request framed as routine — verify tier badge is correct regardless of framing

---

## 9. Acceptance Criteria

1. All proposals display the actual operation, not the agent's description
2. Sensitive operations require two-step confirmation with delay
3. Batch proposals cannot "approve all" when mixed tiers are present
4. Escalation patterns trigger visible warnings
5. Tier classification is a compiled const — the agent cannot influence its own proposal's tier
6. Full test coverage including adversarial cases
