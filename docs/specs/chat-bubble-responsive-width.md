# Spec: Chat Bubble Responsive Width

## Problems

1. **Fixed max width:** `FawxSpacing.maxMessageWidth` is hardcoded at `720px`. On a wide monitor, messages are a narrow column with huge gutters. On a narrow window, 720px might overflow.
2. **Odd spacing on resize:** When the window is resized, chat bubbles don't reflow smoothly. The fixed width creates jarring spacing jumps.

## Root Cause

In `MessageBubble.swift`:
```swift
.frame(
    maxWidth: FawxSpacing.maxMessageWidth,  // 720 hardcoded
    alignment: role == .user ? .trailing : .leading
)
```

And in `bubbleLabel`:
```swift
.frame(maxWidth: FawxSpacing.maxMessageWidth, alignment: .leading)
```

## Solution

### Make maxMessageWidth responsive to container width

Replace the hardcoded 720 with a proportion of the available width, with a reasonable min/max:

In `Spacing.swift`, change `maxMessageWidth` from a constant to a function:

```swift
// REMOVE:
static let maxMessageWidth: CGFloat = 720

// ADD:
static func maxMessageWidth(for containerWidth: CGFloat) -> CGFloat {
    let proportional = containerWidth * 0.85
    return min(max(proportional, 400), 1200)
}
```

- **85% of container width:** Messages use most of the space
- **Min 400px:** Prevents crushing on very narrow windows
- **Max 1200px:** Prevents unreadably wide lines on ultrawide monitors

### Pass container width through to MessageBubble

Use a `GeometryReader` at the scroll view level in `ChatDetailView` to measure available width, then pass it down:

Option A (preferred): Use an environment value.

```swift
private struct ContainerWidthKey: EnvironmentKey {
    static let defaultValue: CGFloat = 720
}

extension EnvironmentValues {
    var containerWidth: CGFloat {
        get { self[ContainerWidthKey.self] }
        set { self[ContainerWidthKey.self] = newValue }
    }
}
```

In `ChatDetailView`, wrap the transcript scroll view:
```swift
GeometryReader { proxy in
    scrollViewContent
        .environment(\.containerWidth, proxy.size.width)
}
```

In `MessageBubble`, read and use it:
```swift
@Environment(\.containerWidth) private var containerWidth

// Then:
.frame(
    maxWidth: FawxSpacing.maxMessageWidth(for: containerWidth),
    alignment: role == .user ? .trailing : .leading
)
```

### Also update ToolCallCard

`ToolCallCard.swift` also uses `FawxSpacing.maxMessageWidth`:
```swift
.frame(maxWidth: FawxSpacing.maxMessageWidth, alignment: .leading)
```

Same fix: read `containerWidth` from environment.

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/Theme/Spacing.swift` | `maxMessageWidth` → function with proportional calculation |
| `app/Fawx/Theme/ContainerWidth.swift` | New: environment key for container width |
| `app/Fawx/Views/Shared/MessageBubble.swift` | Read `containerWidth`, use dynamic max width |
| `app/Fawx/Views/Shared/ToolCallCard.swift` | Same: dynamic max width |
| `app/Fawx/Views/Shared/ChatDetailView.swift` | GeometryReader + environment injection |

## Testing

### Unit Tests
1. `maxMessageWidth(for: 800)` returns `680` (800 * 0.85)
2. `maxMessageWidth(for: 300)` returns `400` (min clamp)
3. `maxMessageWidth(for: 2000)` returns `1200` (max clamp)

### Manual Testing
1. Resize window narrow → bubbles shrink proportionally, no horizontal overflow
2. Resize window wide → bubbles expand to use space, max 1200px
3. No odd spacing gaps at any width
4. Tool call cards also resize proportionally
5. User bubbles still right-aligned, assistant bubbles left-aligned
6. iOS: verify no regression (screen width is fixed)
