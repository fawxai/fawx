# Spec: fawx-skill-browser (Web Browser Control)

**Status:** Draft  
**Date:** 2026-03-08  
**Repo:** `fawxai/fawx-skill-browser`

---

## 1. Problem

Fawx can fetch web pages (fawx-skill-web-fetch) but can't interact with them — no JavaScript execution, no clicking, no form filling, no screenshots.

## 2. Approach

WASM skill that controls a headless Chromium browser via the Chrome DevTools Protocol (CDP). Connects to an existing browser instance or launches one.

## 3. Tools

```json
{
  "name": "browser_navigate",
  "description": "Navigate to a URL and return the page content",
  "parameters": {
    "url": "string — URL to navigate to",
    "wait_for": "string — CSS selector to wait for (optional)",
    "timeout_seconds": "integer — max wait time (default: 30)"
  }
}

{
  "name": "browser_snapshot",
  "description": "Get the current page's accessible content as structured text",
  "parameters": {
    "selector": "string — CSS selector to scope snapshot (optional)"
  }
}

{
  "name": "browser_action",
  "description": "Perform an action on the page (click, type, etc.)",
  "parameters": {
    "action": "string — click|type|select|scroll|screenshot",
    "selector": "string — CSS selector for target element",
    "value": "string — text to type or option to select (for type/select actions)"
  }
}

{
  "name": "browser_screenshot",
  "description": "Take a screenshot of the current page",
  "parameters": {
    "full_page": "boolean — capture full page or viewport only (default: false)",
    "output_path": "string — where to save (default: ~/.fawx/media/screenshots/)"
  }
}
```

## 4. Implementation

### CDP Connection

The skill connects to a running Chrome/Chromium instance via WebSocket:

```
chrome --headless --remote-debugging-port=9222
```

The skill sends CDP commands (JSON-RPC over WebSocket) via host_api HTTP/WebSocket.

### Config

```toml
[browser]
enabled = true
cdp_url = "ws://127.0.0.1:9222"
# Or launch Chrome automatically:
# chrome_path = "/usr/bin/chromium"
# headless = true
```

## 5. Dependencies

- Chrome/Chromium installed on the host
- CDP protocol (no Selenium/WebDriver needed)

## 6. Testing

- Mock CDP WebSocket server
- Navigate returns page content
- Click triggers navigation
- Type fills input
- Screenshot saves file
- Timeout handling
- Connection error handling
