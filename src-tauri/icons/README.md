# Ferrous Frog app icon

`icon.png` is the original, square RGBA source for the graphite-and-mint frog mark. Its transparent outer margin works on light and dark desktops. The design is original and does not use competitor assets.

The desktop sizes, Windows `icon.ico` and macOS `icon.icns` are generated from this source with Tauri's installed icon command:

```sh
npm run tauri -- icon src-tauri/icons/icon.png --output /tmp/ferrous-frog-icons
```

Copy `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.ico` and `icon.icns` from that output into this directory, preserving the original `icon.png`. Copy `128x128@2x.png` to `public/brand/ferrous-frog.png` for the toolbar and splash, and `32x32.png` to `public/favicon.png`. The additional mobile and Windows Store outputs are not used by the current desktop targets.

## Generation prompt

Created with the built-in imagegen tool. The source is the selected original output; the prompt's requested size is a design target, and Tauri creates the exact required export dimensions.

```text
Use case: logo-brand.
Asset type: production desktop application icon, single square 1024 x 1024 PNG with a real transparent background outside the icon.
Primary request: design a beautiful original icon for Ferrous Frog, a professional Rust desktop SEO crawling and website inspection application. This is the actual final usable asset, not a presentation sheet.
Subject: a memorable, elegantly geometric FRONT-FACING FROG HEAD, using one bold mint-green silhouette. A broad compact head with two subtly raised rounded eye lobes, two confident dark inset eyes, and a simple angular negative-space mouth. Refined optical balance, softly chamfered geometry, clear thick shapes, restrained friendly character, precise tool-like design. It must instantly read as a frog at 32 pixels, with no tiny details.
Backdrop: a near-black graphite rounded-square app tile with smoothly continuous corners. The tile occupies approximately 90% of the square canvas, centered with equal transparent margins. A very subtle graphite edge and barely perceptible top-to-bottom shading give the tile excellent definition on both white and black desktops. The frog is optically centered and occupies about 67% of the tile width, large and confident.
Color palette: charcoal #111715 and luminous muted mint #77DFC0, with at most a very subtle mint tonal shift. Crisp, premium, understated.
Style: expertly art-directed contemporary software icon, vector-like smooth precise edges; primarily flat with only restrained material depth. Minimal and elegant, not a cartoon mascot illustration.
Text: none. No letters, no wordmark, no FF.
Constraints: exactly one icon. Strict straight-on orthographic view. True transparent outer background, NOT a white or checkerboard background. Original clean-room brand, do not reference or imitate any existing SEO competitor logo. No circular logo container, no full frog body, no limbs, no frog jumping, no lens, no magnifying glass, no spider, no circuit-board decoration, no network dots, no badge, no external shadow, no glow, no neon, no chrome, no clutter, no watermark.
```
