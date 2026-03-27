# Backlog: Skill Configuration Management UI

## Summary

Add a first-class UI for viewing, adding, updating, and validating skill-related configuration values such as API keys and other stored settings.

This should be prioritized ahead of cosmetic Skills UI consistency work because missing configuration management blocks real skill usage and makes installed skills feel broken or incomplete.

## Tracking

- GitHub issue: not filed
- Status: prioritized local backlog for post-loop-refactor product work
- Related backlog: `docs/backlog/skills-installed-marketplace-ui-consistency.md`
- Local reference: `docs/backlog/skill-configuration-management-ui.md`

## Why Deferred

The current product exposes installed skills and marketplace skills, but it does not provide a clear UI path to inspect or update the configuration those skills depend on.

That creates several UX problems:

1. A skill can appear installed and loaded while still being unusable because a required key has not been configured.
2. Users have no obvious place to discover which keys already exist for a skill or which ones are missing.
3. Runtime setup currently falls back to command-line or out-of-band configuration, which breaks the mental model of the Skills UI as the place to manage skills.
4. Part of the Installed vs Marketplace visual inconsistency likely comes from the UI not representing configuration state as a first-class concept.

## Acceptance Criteria

1. Add a UI entry point from the Installed skill view to inspect and manage that skill's configuration.
2. Show existing configured values in a safe form:
   secrets are redacted or partially masked;
   non-secret values may be shown directly when appropriate.
3. Show which required configuration values are missing for a skill.
4. Allow users to add or update skill configuration values without leaving the app.
5. Reflect configuration readiness in the Skills UI so users can distinguish:
   installed and ready;
   installed but requires setup;
   installed with optional configuration available.
6. Keep the runtime source of truth and UI representation aligned so marketplace and installed views present the same readiness state for the same skill.
7. Add coverage for at least one key-backed skill such as Browser with a missing-key state and a configured-ready state.
