# Track G-1: Skills Marketplace Stub Endpoints

**Status:** SPEC
**Priority:** Medium — unblocks Swift Skills screen
**Endpoints:** GET `/v1/skills/search`, POST `/v1/skills/install`, DELETE `/v1/skills/{name}`

---

## Overview

Add stub marketplace endpoints that return reasonable default/empty responses. These will be wired to a real `api.fawx.ai` marketplace API later. For now they allow the Swift app to build the Skills UI without blocking on the backend marketplace.

---

## Endpoints

### GET /v1/skills/search?q={query}

Stub that returns empty results.

Response 200:
```json
{
  "query": "portfolio",
  "skills": [],
  "total": 0,
  "marketplace_available": false,
  "message": "Marketplace not yet connected"
}
```

### POST /v1/skills/install

Stub that returns 503 (marketplace not available).

Request:
```json
{
  "name": "portfolio-tracker"
}
```

Response 503:
```json
{
  "error": "Marketplace not yet available. Install skills via CLI: fawx skills install <name>"
}
```

### DELETE /v1/skills/{name}

Actually functional — removes a locally installed skill via the existing SkillRegistry.

Response 200:
```json
{
  "name": "portfolio-tracker",
  "removed": true
}
```

Response 404:
```json
{
  "error": "Skill 'portfolio-tracker' not found"
}
```

---

## Implementation

Simple handlers, no new crates needed:

1. **Create `engine/crates/fx-api/src/handlers/marketplace.rs`**
2. **Add to handlers/mod.rs**: `pub mod marketplace;`
3. **Wire routes** in router.rs
4. **DELETE** should interact with the existing skill loading infrastructure if available, or return a stub

---

## Tests

1. `search_returns_empty_results` — GET returns empty array + marketplace_available: false
2. `install_returns_503` — POST returns 503
3. `delete_unknown_skill_returns_404` — DELETE nonexistent skill
4. Response serialization round-trips

---

## Acceptance Criteria
- All three endpoints respond correctly
- Search always returns empty (stub)
- Install always returns 503 (stub)
- Delete returns 404 for unknown skills
- Clippy clean, tests pass
