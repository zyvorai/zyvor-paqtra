# Social assets

| File | What it is | Rebuild |
|---|---|---|
| `paqtra-share-card.html` / `.png` | 1200×630 card: the README hero and the website's social preview (`website/docusaurus.config.ts` serves this folder as static files) | `./docs/social/build-social-card.sh` |
| `paqtra-social-card.html` / `.jpg` | 1600×900 (16:9) card for LinkedIn and X: the flow → verdict → drop → investigate → policy story | `./docs/social/build-social-card.sh` |
| `readme/*.html` | Three cards used by the README, in the same dark look: `readme-how-it-works.jpg`, `readme-capabilities.jpg`, `readme-boundary.jpg` (written to `docs/ux/`) | `./docs/social/readme/build.sh` |

The build needs Google Chrome (set `CHROME=...` to override the path) and macOS `sips` for the JPEG. Nothing is installed.

Claims on the cards are limited to what [`AGENTS.md`](../../AGENTS.md) and
[`cilium-brotherhood.md`](../cilium-brotherhood.md) already say: Paqtra observes, Cilium decides.
The flow example on the 16:9 card is illustrative and labelled as such. When the version on the
share-card footer moves, edit `paqtra-share-card.html` and rebuild.
