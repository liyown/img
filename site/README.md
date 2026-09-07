# img website

Bilingual static product website for img community releases.
Chinese: https://liyown.github.io/img/ · English: https://liyown.github.io/img/en/

## Local development

Node 24 and pnpm 11.10.0. Dependencies stay separate from the Rust workspace.

```sh
cd site
pnpm install --frozen-lockfile
pnpm dev
pnpm check
pnpm build
```

The development URL is `http://127.0.0.1:4321/img/`. Content lives in
`src/content/docs/{zh,en}`, shared routes and site facts in `src/lib/site.ts`,
and sourced comparison data in `src/lib/comparison.ts`. CLI help was captured
from the local release binary for product commit `cec6219`; update both language
references when commands change. The GUI currently has a Chinese interface.

Screenshot source files are unaltered native captures of the release app with
isolated demo records. `scripts/prepare-demo.py` builds that fixture from the
local 0.3.0 package without using real storage credentials. Close the demo app
with Command-Q after capture. It uses only `target/site-demo`.

This website does not publish an installer or create a product release tag.

## Quality checks and deployment

```sh
pnpm format:check
pnpm check
pnpm build
pnpm test
pnpm verify
```

`verify` checks all 26 pages, matching languages, internal links and fragments,
metadata, sitemap, structured data, image dimensions, and the project-root
`404.html` used by GitHub Pages. The two localized 404 routes remain available.
No project-level robots.txt is written: `/img/robots.txt` would not govern the
host. The host-root robots response is checked separately in `QA.md`.

Pushes affecting the website on main build and deploy through `site.yml`.
Pull requests run checks without deploying. Enable GitHub Pages with source
**GitHub Actions**. No tags or installer releases are created.

### Optional analytics

The first deployment has no GA4 ID and makes no Google Analytics requests.
Later set the repository variable `PUBLIC_GA_MEASUREMENT_ID` to a valid `G-...`
ID and dispatch the website workflow. Keep GA4 Enhanced Measurement disabled
for that data stream: this integration intentionally sends only `page_view`
and `entry_click` (GUI, CLI, GitHub). It disables automatic initial pageviews,
ad personalization, and Google signals. The browser loads GA only after consent.
Choices can be refused or withdrawn, and are synchronized across tabs.

`PUBLIC_GOOGLE_SITE_VERIFICATION` optionally emits a Search Console meta tag.
Setting it does not submit the site for indexing. Both variables are optional,
public build-time configuration; they are not secrets.

`pnpm test` uses a non-networked DOM simulation and the test-only ID `G-TESTONLY`
for missing-ID, refusal, consent, withdrawal, subsequent consent, and cross-tab
scenarios. It never loads Google's script or sends data to an analytics account.

### Screenshots and share cards

The original native window captures are lossless 1280×900 PNG files. This capture
display is 1×; images are never upscaled or represented as Retina captures.
`pnpm assets` generates 640/896/960/1280-wide lossless WebP variants and bilingual
1200×630 Open Graph cards from local sources. Preserve original PNG captures;
do not substitute the downscaled JPEG previews returned by accessibility tools.
The website design uses code-authored icons and standard system fonts.

## Latest downloads

The stable entry point is `/img/install/#gui` (English: `/img/en/install/#gui`).
The Downloads component resolves stable `desktop-v*` releases at page load,
checks architecture assets and checksum links, and updates the download links.
A new release needs no website version edit or rebuild. On API failure, visitors
can use the official Releases page. CLI releases are excluded from desktop selection.
Package creation and publishing belong to the desktop release workflow, not Pages.
