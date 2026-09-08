---
name: img-uploader
description: Upload and process images, query the shared image library, inspect sync, and preview image-host migrations or Markdown reference repairs with the native img CLI. Use for image hosting and local PNG/JPEG/static WebP processing through configured S3/R2/OSS, GitHub, WebDAV, or HTTP providers.
---

# img image workflows

Use the installed `img` CLI. Locate it with `command -v img` and check `img version`; the commands below require 0.4 or later. Desktop packages include the CLI, but the desktop does not need to run. Credentials and library records are shared locally.

Choose the workflow the user requested. Local processing and library queries do not require provider validation. Before uploading, run `img config validate`; inspect provider names with `img provider list` when needed. Keep credentials and environment values out of commands, logs, and responses.

## Upload

Check requested files exist, then use the configured default provider unless another was specified:

```sh
img upload "/absolute/path/image.png" --format json --no-copy
img upload "/path/a.png" "/path/b.webp" --provider oss --path articles --format json --no-copy
```

Read every `files` result. Exit 3 means partial failure: preserve successful URLs and identify failures. Do not upload unrelated files or infer permission to overwrite. Use `--name` only for a single file. Return useful URLs or Markdown, never invented links.

## Local image processing

A new output preserves the original and does not upload anything:

```sh
img process "/path/photo.png" --output "/path/photo.webp" --image-format webp
img process "/path/photo.png" --recipe "/path/recipe.json" --output-dir "/path/results"
```

Minimal recipe:

```json
{"version":1,"encoding":{"format":"webp","compression":{"mode":"quality","quality":85}}}
```

PNG/JPEG/static WebP can be edited; do not flatten animated inputs. Target-size mode reports `target_met`; do not describe an unmet target as successful compression to the requested size. Read per-output order and errors from the result. Existing tasks can be inspected with `img tasks show TASK_ID` and retried with `img tasks retry TASK_ID`.

## Library and sync

```sh
img library list --search cover --limit 100
img library show IMAGE_ID
img library check IMAGE_ID
img library download IMAGE_ID --output-dir /path/downloads
img sync status
img sync conflicts
```

Results cover indexed scopes, not necessarily all remote files. Cache presence does not establish remote availability. A failed authenticated request is not proof a file is missing. Query additional pages using `--offset` when needed.

`img sync run` exchanges metadata with the configured sync backend; use it when the user asks to synchronize. Hosting credentials are not additionally encrypted in the sync directory, while the backend's own credentials stay device-local. Never configure sync or resolve a conflict merely to answer a status question.

## Migration and reference repair

Start with a reviewable plan:

```sh
img migrate plan IMAGE_ID --to r2 --prefix articles --output /path/move.json
img migrate show TASK_ID
img references scan /path/articles --migration TASK_ID --output /path/references.json
img references show REFERENCE_TASK_ID
```

A migration plan freezes the selected locations and destination. Applying it uploads images; do this only when the user requested migration execution:

```sh
img migrate apply /path/move.json --report /path/report.json
```

Sources remain in place. Do not switch references to unverified URLs. `img references apply REFERENCE_TASK_ID --yes` writes articles and must be explicitly requested. It rechecks documents and destination images and backs up each file before replacement. `references restore` creates a restoration preview, not an immediate write.

Remote deletion also requires an explicit request identifying the target storage and objects. Do not interpret migration, cache cleanup, or reference scanning as deletion authorization. References outside a scanned directory are unknown.

## Failures and boundaries

- Exit 1: operation failed; return useful context. Exit 2: invalid configuration or arguments; do not retry unchanged. Exit 3: preserve partial successes.
- Reuse saved task IDs for retries instead of starting a duplicate migration or upload.
- A successful upload can include a record-saving warning: return its URL and explain that the local record was not saved.
- Use `--no-copy` for agent uploads. Clipboard interaction is unnecessary for returning results.
- If `img` is absent, use the project's documented installer; do not substitute another hosting service.
