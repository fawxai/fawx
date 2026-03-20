# Spec: macOS App Icon Fix

## Problem
The app icon shows on iOS but not macOS. The asset catalog has a single 1024x1024 PNG referenced for all mac icon sizes (16-512 @1x and @2x). macOS requires properly sized variants; it won't downscale from 1024.

## Fix

### Option A: Generate sized variants (recommended)

Use `sips` to generate properly sized PNGs from the 1024x1024 source, then update `Contents.json` to reference each one:

Required sizes:
- 16x16 @1x (16px), @2x (32px)
- 32x32 @1x (32px), @2x (64px)
- 128x128 @1x (128px), @2x (256px)
- 256x256 @1x (256px), @2x (512px)
- 512x512 @1x (512px), @2x (1024px)

Generate them:
```bash
cd app/Fawx/Assets.xcassets/AppIcon.appiconset
for size in 16 32 64 128 256 512 1024; do
  sips -z $size $size AppIcon.png --out AppIcon-${size}.png
done
```

Update `Contents.json` so each entry points to the correctly sized file:
```json
{ "filename": "AppIcon-16.png", "idiom": "mac", "scale": "1x", "size": "16x16" },
{ "filename": "AppIcon-32.png", "idiom": "mac", "scale": "2x", "size": "16x16" },
{ "filename": "AppIcon-32.png", "idiom": "mac", "scale": "1x", "size": "32x32" },
{ "filename": "AppIcon-64.png", "idiom": "mac", "scale": "2x", "size": "32x32" },
{ "filename": "AppIcon-128.png", "idiom": "mac", "scale": "1x", "size": "128x128" },
{ "filename": "AppIcon-256.png", "idiom": "mac", "scale": "2x", "size": "128x128" },
{ "filename": "AppIcon-256.png", "idiom": "mac", "scale": "1x", "size": "256x256" },
{ "filename": "AppIcon-512.png", "idiom": "mac", "scale": "2x", "size": "256x256" },
{ "filename": "AppIcon-512.png", "idiom": "mac", "scale": "1x", "size": "512x512" },
{ "filename": "AppIcon-1024.png", "idiom": "mac", "scale": "2x", "size": "512x512" },
{ "filename": "AppIcon.png", "idiom": "universal", "platform": "ios", "size": "1024x1024" }
```

## Files Changed

| File | Change |
|------|--------|
| `app/Fawx/Assets.xcassets/AppIcon.appiconset/Contents.json` | Per-size filenames |
| `app/Fawx/Assets.xcassets/AppIcon.appiconset/AppIcon-*.png` | New sized variants |

## Testing
- Build macOS app, check icon in Dock and Finder
- Build iOS app, verify icon unchanged
- Check all sizes render: Cmd+I on .app in Finder shows icon at multiple sizes
