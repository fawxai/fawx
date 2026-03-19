# Spec: Streaming Scroll Smoothing

**Problem:** When GPT-5.4 or other fast models stream tokens, the TUI auto-scroll updates on every token, causing visual whiplash and janky reading UX. The markdown re-renders and scroll position jumps faster than the eye can track.

**Goal:** Smooth, readable streaming output that feels like fast typing, not an instant dump.

---

## Requirements

### 1. Render Batching (50ms coalesce)

Instead of re-rendering the markdown view on every incoming token, buffer tokens and flush renders on a fixed cadence.

- Accumulate incoming tokens into a buffer string
- Flush the buffer and re-render at most every **50ms** (20fps for text is perceptually smooth)
- On stream end, flush immediately (no stale tail)
- This reduces layout thrash from hundreds of renders/sec to ~20

**Implementation notes:**
- Use a `DispatchSourceTimer` or `Task.sleep(for: .milliseconds(50))` coalesce loop
- The render flush appends the buffered text to the displayed message and triggers a single markdown re-parse
- Keep the raw accumulated text as the source of truth; the rendered view is derived

### 2. Smart Auto-Scroll (sticky bottom)

Auto-scroll should only engage when the user is already reading at the bottom. If they've scrolled up to re-read earlier content, don't fight them.

**Rules:**
- **Pinned mode** (default): user is within ~50px of the bottom → auto-scroll on each render flush
- **Detached mode**: user has scrolled up beyond the threshold → stop auto-scrolling
- **Re-pin**: when user scrolls back to bottom (within threshold), re-engage auto-scroll
- Show a subtle "↓ New content" indicator when detached and new content arrives (optional, nice-to-have)

**Implementation notes:**
- Track scroll position in the `ScrollView` / `ScrollViewReader`
- On each render flush, check if pinned before calling `scrollTo(bottom)`
- The threshold should account for line height variance (50px or ~3 lines)

### 3. Smooth Scroll Animation

When auto-scrolling, don't snap — animate.

- Use `withAnimation(.easeOut(duration: 0.15))` around `scrollTo` calls
- The short duration (150ms) keeps it feeling responsive while eliminating the jarring jump
- Combined with 50ms render batching, this means scroll animations overlap naturally

### 4. Optional: Typewriter Speed Cap

For users who want a more deliberate reading pace:

- Config option: `display.max_tokens_per_second` (default: unlimited, suggested cap: 120)
- When enabled, tokens exceeding the cap are queued and released at the capped rate
- This is purely a display-side delay — the actual stream completes at full speed in the background
- **Defer this to a future PR** unless trivial to add alongside the batching

---

## Architecture

```
Token Stream (SSE) 
    → TokenBuffer (accumulates raw text)
    → RenderTimer (50ms tick)
        → flush: append buffer to display text, re-render markdown
        → if pinned: smoothly scroll to bottom
```

### Key Types

```swift
/// Manages token buffering and render coalescing for streaming responses.
class StreamingDisplayController {
    /// Accumulated tokens not yet flushed to display
    private var pendingTokens: String = ""
    
    /// Whether the user is pinned to bottom (auto-scroll active)
    private(set) var isPinnedToBottom: Bool = true
    
    /// Timer for coalescing renders
    private var renderTimer: Task<Void, Never>?
    
    /// Call on each incoming token from the SSE stream
    func appendToken(_ token: String)
    
    /// Call when stream ends — flush any remaining tokens
    func streamDidEnd()
    
    /// Call when user scrolls — update pinned state
    func userDidScroll(distanceFromBottom: CGFloat, threshold: CGFloat = 50)
}
```

### Integration Points

- **Chat view**: Replace direct token→render with `StreamingDisplayController.appendToken()`
- **Scroll view**: Wire scroll position changes to `userDidScroll()`
- **Stream completion**: Call `streamDidEnd()` from the SSE handler's completion path

---

## Testing

1. **Render batching**: Simulate rapid token arrival (100 tokens in 50ms), verify only 1-2 render flushes occur
2. **Sticky scroll**: Verify auto-scroll stops when simulating a scroll-up event, resumes on scroll-to-bottom
3. **Stream end flush**: Verify no tokens are lost when stream ends between render ticks
4. **Animation**: Visual QA — streaming should feel smooth, not jumpy (manual test)

---

## Acceptance Criteria

- [ ] Streaming long responses from GPT-5.4 on xhigh is visually smooth
- [ ] User can scroll up mid-stream without the view fighting them
- [ ] Scrolling back to bottom re-engages auto-scroll
- [ ] No tokens lost or duplicated
- [ ] No perceptible input lag (the buffer is display-only, not blocking the stream)
- [ ] Works with both short (1-line) and long (100+ line) responses

---

## Non-Goals

- Server-side throttling (stream at full speed, buffer client-side)
- Changing the SSE protocol or token format
- Typewriter speed cap (defer to future PR)
