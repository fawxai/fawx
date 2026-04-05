
---

## Example Scene-State Contract

This is illustrative, not final:

```json
{
  "profile": "ui/v1",
  "schema_version": "1.0.0",
  "timestamp_ms": 1711234567890,
  "frame_id": 42,
  "belief_state_id": "ui_42_a",
  "prediction_error": 0.41,
  "goal_relevance": 0.92,
  "health": {"observer_ok": true, "latency_ms": 21},
  "uncertainty": {"global": 0.18, "ocr": 0.07, "layout": 0.14},
  "calibration": {"ece": 0.09, "status": "calibrated"},
  "probe_budget_remaining": 2,
  "recent_action": {
    "type": "click",
    "target_hint": "search_field",
    "age_ms": 180
  },
  "entities": [
    {
      "id": "el_101",
      "type": "text_field",
      "label": "Search",
      "bounds": {"x": 330, "y": 120, "w": 420, "h": 34},
      "state": {"focused": true, "enabled": true, "visible": true},
      "affordances": ["type", "paste"],
      "confidence": {
        "detection": 0.97,
        "state": 0.95,
        "semantic": 0.93
      }
    },
    {
      "id": "el_102",
      "type": "list_item",
      "label": "Jack — signed contract",
      "bounds": {"x": 300, "y": 210, "w": 580, "h": 52},
      "state": {"selected": false, "visible": true},
      "affordances": ["open", "hover"],
      "confidence": {
        "detection": 0.91,
        "state": 0.88,
        "semantic": 0.72
      }
    }
  ],
  "text_regions": [
    {
      "id": "txt_9",
      "text": "2 results",
      "bounds": {"x": 305, "y": 182, "w": 80, "h": 18},
      "confidence": {
        "detection": 0.99,
        "semantic": 0.98
      }
    }
  ],
  "watches": [
    {"name": "attachment_loaded", "status": "pending"}
  ],
  "probe_request": {
    "type": "hover",
    "target_id": "el_102",
    "reason": "semantic confidence 0.72 — metadata occluded, label may be truncated",
    "priority": "normal"
  },
  "scene_summary": "Mail results visible; likely matching message from Jack with signed contract"
}
```

### Contract versioning

- semver at the schema level
- stable core envelope
- additive changes within a major version
- profile-specific payloads evolve independently from the core envelope when needed
