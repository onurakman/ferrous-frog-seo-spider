# Workspace previews

- `workspace-dark.png` and `workspace-light.png`: matching 3072 × 1920 captures of the React workspace with synthetic crawl data, using the existing Tauri IPC smoke-test harness. No user crawl data is included.
- `workspace-themes.png`: README composition made with the built-in imagegen tool. Original screenshots remain available separately for full-resolution inspection.

## Composition prompt

Image 1 refers to the first composition; images 2 and 3 refer to the original dark and light screenshots.

```text
Use case: compositing, precise correction.
Image 1 is the composite to correct. Image 2 is the unmodified dark screenshot. Image 3 is the unmodified light screenshot.
Keep the complete 8:5 screenshot composition, with dark left and light right. Make the single dividing cut EXACTLY 30 degrees away from vertical (60 degrees from horizontal): at an output size of 1600 x 1000, it MUST run from x=1089 at the top to x=511 at the bottom. Do not use the shallower diagonal in image 1. It is a straight mask edge.
The UI content must be copied from images 2 and 3, never redrawn. The left and right screenshot coordinate systems are identical. In particular, correct the garbled bottom-left status in image 1: the exact text there is "Finished https://example.test/". Correct "Type" in the lower table header from the original source. Preserve every URL, number, label, icon, and all horizontal grid alignment across the split.
No styling changes, no extra text, no new UI, no margins, no glow, no perspective. A crisp, accurate cut-and-join of the supplied real screenshots. High resolution 1600 x 1000 or larger at the same 8:5 ratio.
```
