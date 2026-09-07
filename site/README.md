# img website

Bilingual static product website for the unreleased Rust img 0.3.0 preview.
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
