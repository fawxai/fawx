# Backlog: Skills Installed vs Marketplace UI Consistency

## Summary

Defer unifying the Installed and Marketplace skill presentations in the Skills UI.

This is not a functional install-state bug. Installed skills are loading correctly, but the two tabs render different visual language and different naming sources, which makes the product feel inconsistent.

## Tracking

- GitHub issue: not filed
- Status: local backlog for post-loop-refactor UX cleanup
- Related backlog: `docs/backlog/skill-configuration-management-ui.md`
- Local reference: `docs/backlog/skills-installed-marketplace-ui-consistency.md`

## Why Deferred

The current implementation uses separate view/data pipelines:

1. Installed skills render from runtime/server data in `app/Fawx/Views/Shared/SkillsView.swift`.
2. Marketplace skills render from marketplace metadata in `app/Fawx/Views/Shared/MarketplaceView.swift`.

That split is partly intentional because the Installed tab needs runtime actions like permissions and removal, while the Marketplace tab needs publisher and verification metadata.

Even so, the current result is visually and semantically inconsistent:

1. Installed cards use raw runtime names like `browser` and `cron`, while Marketplace cards use curated titles like `Browser` and `Scheduler`.
2. Installed cards use a different icon/tile treatment than Marketplace cards.
3. Installed state is presented with different badge styles across the two tabs.
4. Similar skills can look like different entities depending on which tab the user is viewing.

Some of this inconsistency may be better addressed as part of first-class configuration/readiness UX rather than as purely cosmetic card unification.

## Acceptance Criteria

1. Define a shared normalized skill presentation model that both Installed and Marketplace views can consume.
2. Ensure installed skills and marketplace entries use consistent naming, iconography, and badge language where they represent the same skill.
3. Preserve tab-specific controls and metadata:
   Installed keeps runtime actions like permissions and remove.
   Marketplace keeps marketplace metadata like publisher and verification.
4. Make installed-state styling consistent between tabs so a skill does not appear visually reclassified when the user switches views.
5. Add UI coverage for at least one skill that appears in both Installed and Marketplace sources.
