---
name: img-uploader
description: Upload local image files with the img CLI and return usable URL or Markdown results. Use when the user asks an agent to upload, publish, host, share, or create links for PNG, JPEG, GIF, WebP, SVG, or AVIF files through a configured S3/R2/OSS, GitHub, or HTTP image provider.
---

# Image Uploader

Use the `img` executable as the only upload interface. Keep credentials out of commands, logs, and responses.

## Workflow

1. Confirm each requested local file exists. Do not upload unrelated images.
2. Locate the CLI with `command -v img`. In an `img` repository checkout, fall back to `./bin/img` when present.
3. Run `img config validate`. If configuration is missing, stop and give the initialization command from the repository README. Never ask the user to paste credentials into chat.
4. Use the configured default Provider unless the user names one. Inspect names with `img provider list` when needed.
5. Upload with `--format json --no-copy` so results are machine-readable and no clipboard process is invoked.
6. Parse every item in `files`. Preserve partial successes when the process exits with code 3.
7. Return concise clickable links. Prefer Markdown image syntax when the user wants content for Markdown; otherwise return URLs.

## Commands

Single file:

```sh
img upload "/absolute/path/image.png" --format json --no-copy
```

Multiple files:

```sh
img upload "/path/a.png" "/path/b.webp" --format json --no-copy
```

Named Provider and remote prefix:

```sh
img upload "/path/image.png" --provider github --path posts/assets --format json --no-copy
```

Use `--name desired.png` only for a single file. Use `--overwrite` only when the user explicitly requests replacement or confirms an existing object may be replaced.

## Result Handling

- Exit 0: report all uploaded files.
- Exit 1: report the upload error with its useful context.
- Exit 2: report the configuration or argument problem; do not retry unchanged.
- Exit 3: report successful URLs and list failed files separately.
- Never discard successful items because another file failed.
- Do not expose Provider tokens, secrets, Authorization headers, or environment values.

## Guardrails

- Treat uploading as an external write. Only upload files included in the user's request.
- Do not create a public link for private or sensitive material without clear user intent.
- Do not enable overwrite by inference.
- Do not modify global or project configuration during an ordinary upload request.
- Do not use `--copy` in agent workflows.
- If `img` is unavailable, provide the documented install command rather than substituting another upload service.
