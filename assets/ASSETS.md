# Asset provenance

## assets/cat (the real cat)

Generated 2026-10-22 with **Dreamina** (ByteDance), image-to-video, from one generated reference image
(white cat with grey and black patches). Source clips were 832x1120, about 4 s at 24 fps, on a green backdrop.

Processing (`scripts/build_cat.py`): hue key with despill, generator badge and watermark corners masked
out, scaled to 256x256 frames, loop points found by frame matching, exported as lossless WebP with alpha.

| Animation | Source |
|---|---|
| walk, sleep, leap, peek | second batch of clips |
| carry | end of a clip where the cat turns and walks with its head lowered, stabilised in place |
| sit, lie | first batch of clips (the sitting and lying parts) |

**Licence status: NOT VERIFIED.** The clips came from Dreamina's free tier with its watermark, which the processing
masks out. Before this repo is made public or released, check Dreamina's current terms for (a) redistribution in an
open-source repo, (b) commercial use, (c) removing the watermark, and record the answer here. If the terms do not
allow it, regenerate the clips with a watermark-free export or another tool.

## assets/placeholder

Flat generated shapes (not a cat), produced by `cargo run -p marmalade-sprites --example gen_placeholders`.
Used only as a fallback when `assets/cat` is missing.
