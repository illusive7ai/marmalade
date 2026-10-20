# Marmalade cat: asset spec

How to make the clips for the realistic cat. Follow this once and every animation matches.
Marmalade's code never needs a specific cat: any clips that meet this spec plug in.

## 0. The one rule

**Same cat in every clip.** Pick one reference image (white cat with grey/black patches,
per the original brief) and use it as the starting frame / image reference for every clip.
If your generator supports a seed, character reference or "consistent character" feature,
use it. Different-looking cats across animations is the most common way this goes wrong.

## 1. Clip requirements (all animations)

| Item | Requirement |
|---|---|
| Background | Flat, solid chroma green `#00B140` (or pure `#00FF00`). No gradient, no floor, no props, no cast shadow. The cat must not have green on it. |
| Camera | Locked off. No zoom, pan, shake or cut. Side-on view, eye level, as if the camera is orthographic. |
| Framing | Whole cat visible with margin on every side, including tail and ears. Cat fills roughly 50-65% of frame height. Same scale in every clip. |
| Lighting | Soft, even, no hard shadows, no colored light, no motion blur. |
| Facing | Facing **right**. We mirror it in code for left. (Exception: sit and sleep may be three-quarter front, but must still look fine mirrored: no text, no asymmetric collar.) |
| Resolution | 1080p or higher, 24-30 fps source. |
| Length | 3-6 s per clip. Longer is not better. |
| Loops | For looping clips, the first and last pose should match. If the generator can't do this, it is fine: we find the best loop point in post. |
| Motion | The cat stays in place (walks on the spot) when possible. If it travels across the frame, that is acceptable: we stabilise in post. |
| Extras | No people, no humans' hands, no furniture, no text, no watermark. If the tool adds a watermark, the clip is unusable. |

## 2. Animation list

Priority P0 is needed for the first release. P1 comes later.

| ID | Priority | Loop | Description | Used for |
|---|---|---|---|---|
| `sleep` | P0 | yes | Curled up asleep, slow breathing. | Idle state, 0% CPU (one frame shown when truly idle) |
| `sit` | P0 | yes | Sitting upright, tail flicks, slow blink, ears twitch. | Waiting for a command |
| `lie` | P0 | yes | Lying stretched out on its side, relaxed, tail twitch. | Idle variety |
| `walk` | P0 | yes | Proper walk cycle, 2-4 full steps, head level. | Roaming |
| `leap` | P0 | no | Crouches, then leaps forward and down, head first, tail up, out of frame. | Jumping into the folder in the file-fetch scene |
| `peek` | P0 | no | Head and front paws rise into view from the bottom edge, look around, then retreat. | Popping out of the folder; appearing at the screen edge |
| `carry` | P0 | yes | Walk cycle with head slightly lowered and mouth closed as if carrying something small. | Carrying the file |
| `stretch` | P1 | no | Wakes up, front-paw stretch, yawn. | Waking from sleep |
| `groom` | P1 | yes | Sitting, licks paw and washes face. | Idle variety |
| `happy` | P1 | no | Tail straight up, small happy hop or head-bump. | Task finished |
| `alert` | P1 | no | Ears back, arched back, hiss. | Threat found by the scanner |
| `think` | P1 | yes | Sitting, head tilt left and right. | Waiting for the LLM or a long task |
| `poke` | P1 | no | Startled flinch, then annoyed look at the camera. | Clicked on |

Notes:
- The "carried file" is drawn by the app on top of the `carry` clip, so ask the AI for
  a cat walking with nothing visible in its mouth. Generated paper or toys cannot be swapped out.
- Window resize/close and "paw on a button" from the original brief are out of scope.

## 3. Prompt template

Fill in the animation line. Always attach the reference image.

```
A realistic domestic cat, white with grey and black patches, [ANIMATION LINE].
Side-on view, cat facing right, whole body in frame with margin all around.
Locked static camera, no zoom, no pan, no cut. Plain flat solid green screen
background (#00B140), no floor, no shadow, no props. Soft even studio lighting,
no motion blur. Photorealistic fur, natural movement. [LENGTH] seconds.
```

Animation lines to paste:

- `sleep`: "curled up asleep, slowly breathing, only the flank and tail moving slightly"
- `sit`: "sitting upright, tail tip flicking, one slow blink, small ear twitch"
- `lie`: "lying stretched out on its side, relaxed, occasional tail twitch"
- `walk`: "walking at a calm steady pace in place, full walk cycle, head level"
- `leap`: "crouching low, then leaping forward and down, head first with tail up"
- `peek`: "head and front paws rising from below the bottom edge of the frame, looking around, then lowering out of sight"
- `carry`: "walking at a calm steady pace in place with its head slightly lowered, mouth closed"

Tips:
- Generate 3-4 takes per animation and keep the best. The first take is rarely the best.
- If green spill or a grey floor appears, generate again. Don't try to fix it in the prompt.
- If the tool cannot do a clean green background, a plain white or black background also works,
  but green gives the best cutout, especially on white fur.

## 4. What to send me

Put clips in a folder, one file per animation, named by ID: `walk_01.mp4`, `walk_02.mp4`, `sleep_01.mp4` and so on,
plus the reference image as `reference.png`. Note which generator and version you used and the date.

## 5. What I will do with them (pipeline)

1. Remove the background (chroma key with despill, or a matting model if the green is imperfect).
2. Stabilise, crop to one shared bounding box per animation and scale to a common size
   (target: 256 px tall, so a frame is about 330 KB decoded).
3. Find the best loop point for looping clips.
4. Resample to 12-15 fps and export frames as lossless WebP with alpha into `assets/<id>/0001.webp`.
5. Write `assets/<id>/anim.toml`: fps, loop, frame count, foot anchor, and the mouth anchor for `carry`.
6. Check against the budget below, then plug into the sprite player.

## 6. Budget

| Item | Limit |
|---|---|
| Frame size | at most 256 px tall |
| Frames per animation | 12-36 |
| Whole P0 set on disk | about 2-5 MB |
| Decoded in memory | only the playing animation. Everything is freed when asleep except one frame (about 0.3 MB) |
| Active RAM target | 10-30 MB. Asleep: a few MB |

## 7. Licensing

Open-source projects get taken down for unlicensed assets. Before shipping:
- Read the generator's terms: is the output allowed in a public, redistributable repo? Commercial use?
- Record the tool, version, date and terms link in `assets/ASSETS.md`.
- Don't use a real person's or a third party's cat as a reference without permission.
- Keep sprite packs data-only and size-capped so community skins can't carry anything executable.

## 8. Acceptance checklist

- [ ] Same cat in every clip
- [ ] Solid background, no floor or shadow, no watermark
- [ ] Locked camera, whole cat inside the frame
- [ ] Faces right and still looks right when mirrored
- [ ] Loops don't pop at the seam
- [ ] P0 set complete: sleep, sit, lie, walk, leap, peek, carry
- [ ] Generator terms recorded
