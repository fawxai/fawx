# Perception Engine — Bite 2: Florence-2 Evaluation

**Status:** Implementation-ready spec
**Date:** 2026-03-26
**Parent doc:** `docs/vision/perception-engine.md`
**Depends on:** Nothing (runs on manual screenshots; Bite 1 integration is optional validation)

---

## Goal

Answer one question: can Florence-2 extract usable UI element data from Mac screenshots at the resolution and quality Bite 1 will produce?

Specifically:
1. Does it detect buttons, text fields, menus, labels?
2. Does its OCR read UI text accurately?
3. How fast does inference run on Mac GPU (CoreML/MPS) vs CPU?
4. What's the quality difference between half-retina and quarter-retina input?

---

## Scope

Python evaluation script. No model fine-tuning. No integration with Fawx engine. Input: JPEG screenshots. Output: annotated images + metrics JSON.

---

## Architecture: Single Python Package

```
fawx-perception-eval/
  pyproject.toml              — dependencies (transformers, torch, Pillow)
  README.md                   — setup + usage
  src/
    eval_florence2.py          — main evaluation script
    capture_samples.py         — helper to grab manual screenshots
    visualize.py               — draw bounding boxes on frames
    metrics.py                 — detection quality metrics
  samples/                     — manual screenshots (gitignored)
  results/                     — evaluation output (gitignored)
```

---

## Sample Collection (capture_samples.py)

Grab 30-50 diverse screenshots manually:

```bash
# Half-retina capture (Bite 1's target resolution)
screencapture -x -t jpg -R 0,0,1728,1117 /tmp/frame.jpg
# Or full capture, resize in script
```

### Target diversity:
- **Browsers:** Safari, Chrome (tabs, forms, content pages)
- **Productivity:** Mail, Calendar, Notes, Reminders
- **Dev tools:** Xcode, Terminal, VS Code
- **Communication:** Messages, Slack, Discord
- **System:** Finder, System Settings, Spotlight
- **Dense UI:** spreadsheets, multi-panel IDEs
- **Sparse UI:** full-screen video, reading mode

Each screenshot gets a companion annotation file (manual ground truth for a subset):
```json
{
  "file": "safari_github_pr.jpg",
  "app": "com.apple.Safari",
  "ground_truth": [
    {"type": "button", "label": "Merge pull request", "approx_bounds": [680, 420, 180, 36]},
    {"type": "text_field", "label": "Add a comment", "approx_bounds": [100, 600, 800, 120]},
    {"type": "tab", "label": "Files changed", "approx_bounds": [300, 80, 120, 32]}
  ]
}
```

Only annotate 10-15 screenshots for quantitative metrics. The rest are for visual inspection.

---

## Florence-2 Evaluation (eval_florence2.py)

### Model variants to test:
- `microsoft/Florence-2-large` (770M params) — primary candidate
- `microsoft/Florence-2-base` (230M params) — speed comparison

### Tasks to evaluate:

**1. Object Detection (OD)**
```python
prompt = "<OD>"
# Returns bounding boxes + labels for detected objects
```

**2. Dense Region Captioning**
```python
prompt = "<DENSE_REGION_CAPTION>"
# Returns regions with natural language descriptions
```

**3. OCR**
```python
prompt = "<OCR>"
# Returns all detected text
```

**4. OCR with Regions**
```python
prompt = "<OCR_WITH_REGION>"
# Returns text + bounding boxes
```

**5. Caption + Grounding**
```python
prompt = "<CAPTION_TO_PHRASE_GROUNDING>buttons and text fields"
# Returns bounding boxes for described elements
```

**6. Referring Expression (targeted detection)**
```python
prompt = "<REFERRING_EXPRESSION_SEGMENTATION>the submit button"
# Returns segmentation mask for specific element
```

### For each screenshot, run all 6 tasks and save:
```json
{
  "file": "safari_github_pr.jpg",
  "model": "Florence-2-large",
  "inference_ms": 340,
  "tasks": {
    "OD": {"boxes": [...], "labels": [...]},
    "DENSE_REGION_CAPTION": {"regions": [...]},
    "OCR": {"text": "..."},
    "OCR_WITH_REGION": {"text_boxes": [...]},
    "CAPTION_GROUNDING": {"boxes": [...], "phrases": [...]},
    "REFERRING": {"masks": [...]}
  }
}
```

---

## Inference Backends to Test

### 1. PyTorch CPU (baseline)
```python
model = AutoModelForCausalLM.from_pretrained("microsoft/Florence-2-large", torch_dtype=torch.float32)
```

### 2. PyTorch MPS (Mac GPU)
```python
model = AutoModelForCausalLM.from_pretrained("microsoft/Florence-2-large", torch_dtype=torch.float16).to("mps")
```

### 3. CoreML (if conversion works)
- Use `coremltools` to convert
- This is the target for production (fawx-eyes sidecar)
- May not work cleanly with Florence-2's architecture; document blockers if conversion fails

### Record for each backend:
- First-inference latency (includes model load)
- Warm inference latency (average of 10 runs)
- Peak memory usage
- CPU/GPU utilization during inference

---

## Resolution Comparison

Run evaluation at both resolutions on the same screenshots:

| Resolution | Description | Expected Size |
|-----------|-------------|---------------|
| 1728x1117 | Half retina (Bite 1 default) | ~60-80KB JPEG 60% |
| 864x559 | Quarter retina | ~20-30KB JPEG 60% |

Compare detection quality (IoU, text accuracy) between the two. This directly informs the Bite 1 config decision.

---

## Metrics (metrics.py)

For annotated screenshots (ground truth subset):

**Detection quality:**
- IoU (Intersection over Union) between predicted and ground truth boxes
- Precision/Recall at IoU > 0.5
- Mean Average Precision (mAP)

**OCR quality:**
- Character-level accuracy vs ground truth text
- Word-level accuracy
- Missed text regions (false negatives)

**Practical quality (visual inspection on all screenshots):**
- Can you click every detected element? (bounds accuracy)
- Does OCR capture all visible text? (completeness)
- Are interactive elements (buttons, links, fields) reliably detected?
- How does it handle overlapping/nested UI elements?

---

## Visualization (visualize.py)

For each screenshot, produce an annotated version:
- Bounding boxes color-coded by task (OD = red, OCR = blue, grounding = green)
- Labels with confidence scores
- Side-by-side: original | annotated | ground truth (where available)

Save to `results/<model>/<screenshot_name>_annotated.jpg`

---

## Output Structure

```
results/
  Florence-2-large/
    metrics.json              — aggregate metrics
    per_image/
      safari_github_pr.json   — per-image task results
      safari_github_pr_annotated.jpg
      ...
  Florence-2-base/
    metrics.json
    per_image/
      ...
  comparison.json             — large vs base, half vs quarter retina
  summary.md                  — human-readable findings
```

---

## Build & Run

```bash
cd fawx-perception-eval

# Setup
python3 -m venv .venv
source .venv/bin/activate
pip install -e .

# Capture samples
python -m src.capture_samples --count 30 --output samples/

# Run evaluation
python -m src.eval_florence2 --samples samples/ --output results/

# Visualize
python -m src.visualize --results results/ --output results/annotated/
```

---

## Success Criteria

**Must-have (proceed to Bite 3 if met):**
- [ ] Florence-2-large detects >70% of interactive UI elements (buttons, fields, links) at half-retina
- [ ] OCR reads >90% of visible text correctly
- [ ] Warm inference <500ms per frame on Mac GPU (MPS)
- [ ] Bounding boxes accurate enough to click (IoU >0.5 vs ground truth)

**Nice-to-have:**
- [ ] Florence-2-base is usable (>60% detection, <300ms) — cheaper option
- [ ] Quarter-retina quality within 10% of half-retina — would halve Bite 1 storage
- [ ] CoreML conversion works — path to production inference

**Red flags (pivot model if seen):**
- Detection <50% on standard Mac apps
- OCR misses >30% of text
- Inference >2s per frame on MPS
- Systematic failure on specific element types (e.g., never detects dropdown menus)

---

## What We Learn

- Whether Florence-2 is the right base model or if we need alternatives
- Optimal input resolution (half vs quarter retina)
- Inference speed target for production sidecar
- Which UI element types are easy/hard for the model
- Whether fine-tuning is needed (Bite 3) or zero-shot is sufficient

---

## Open Questions

1. **Florence-2 on MPS:** Does the HuggingFace implementation work on Apple Silicon MPS out of the box? May need `PYTORCH_ENABLE_MPS_FALLBACK=1` for unsupported ops.

2. **Model download size:** Florence-2-large is ~1.5GB. First run will download. Cache in standard HF cache dir.

3. **Video memory:** Florence-2-large at float16 needs ~1.5GB VRAM. Mac Mini M2 Pro has 16GB unified memory. Should be fine but verify.

4. **Annotation effort:** Manually annotating 10-15 screenshots is 1-2 hours of work. Worth it for quantitative metrics, or rely on visual inspection only?
