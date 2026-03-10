# Spec: fawx-skill-canvas (Visual Presentation)

**Status:** Draft  
**Date:** 2026-03-08  
**Repo:** `fawxai/fawx-skill-canvas`

---

## 1. Problem

Fawx can only respond with text. For dashboards, reports, diagrams, and rich visualizations, it needs a way to generate and present visual content.

## 2. Approach

WASM skill that generates HTML content, optionally renders to PNG via a headless browser, and serves/sends it via channels.

## 3. Tools

```json
{
  "name": "canvas_create",
  "description": "Create a visual presentation (HTML). Returns the file path.",
  "parameters": {
    "content": "string — HTML content to render",
    "title": "string — page title",
    "width": "integer — viewport width in pixels (default: 800)",
    "height": "integer — viewport height in pixels (default: 600)"
  }
}

{
  "name": "canvas_render",
  "description": "Render an HTML file to a PNG image",
  "parameters": {
    "html_path": "string — path to the HTML file",
    "output_path": "string — where to save the PNG (optional)",
    "width": "integer — viewport width (default: 800)",
    "height": "integer — viewport height (default: 600)"
  }
}
```

## 4. Implementation

### HTML Generation
- Write HTML to `~/.fawx/media/canvas/<id>.html`
- Include inline CSS (no external dependencies)
- Support dark theme by default

### PNG Rendering (optional, requires browser skill)
- If fawx-skill-browser is installed, use it to screenshot the HTML
- Otherwise, send the HTML file directly (Telegram can't display HTML inline, but can send as document)

### Telegram Delivery
- If rendered to PNG: send as photo via `telegram.send_photo()`
- If HTML only: send as document via `telegram.send_document()`

## 5. Testing

- HTML generation with valid content
- File written to correct location
- Rendering integration with mock browser
- Handle empty/invalid HTML
- Title included in output
