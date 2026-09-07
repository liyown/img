---
title: 'Quick start'
description: 'Configure your first storage provider and copy your first image link.'
locale: en
topic: quick-start
order: 0
---

## Choose your workflow

Use the native macOS / Windows / Linux GUI for drag and drop and a visual library. Use the standalone CLI for terminals, editors, and automation. The GUI includes the same-version CLI; both share the upload core and storage configuration.

Get the latest desktop community build from the [installation page](/img/en/install/#gui), then follow the first-launch instructions. No compilation is needed.

## Upload from the desktop

1. Open Settings → Storage and choose R2, S3, OSS, GitHub, or custom HTTP.
2. Enter the endpoint and credentials, save, and set a default. You can test the connection first.
3. Return to the queue. Drop an image, choose files, paste, capture a screenshot, or import a link.
4. Imports enter the ready queue. Review the provider and processing options, then click Upload.
5. Successful links are copied automatically by default. Paste them into your editor.

The desktop imports up to 50 images per batch. Its default per-image limit is 8 MiB, configurable from 1–128 MiB. Start with one small image to check your configuration. The current application interface is Chinese; website documentation is bilingual.

## Upload from the terminal

```sh
img init
img config validate
img provider list
img upload photo.png --format markdown --copy
```

`img init` guides you through provider creation. A regular CLI upload prints a URL by default; add `--copy` or set `output.copy` to copy it. The screenshot command copies by default.

```sh
img screenshot --region --format markdown
img upload a.png b.webp --format json --no-copy
img upload https://example.com/photo.jpg --provider r2
```

The last URL is an example; replace it with your own image. Remote imports require HTTPS by default.

## Set useful defaults

```sh
img config set output.format markdown
img config set output.copy true
img config set upload.retry_count 3
img config set upload.strip_exif true
```

The GUI also offers direct controls for copy formats and upload preferences. Continue with [storage configuration](/img/en/docs/storage/) or the [desktop guide](/img/en/docs/desktop/).
