---
title: 'CLI reference'
description: 'Complete Rust CLI commands, flags, JSON, progress, and exit codes.'
locale: en
topic: cli
order: 5
---

## Commands and shorthand

`img <image or URL>` is shorthand for `img upload <image or URL>`. `--config <file>` is global. The complete command help below comes from the current 0.4.0 release binary.

## Processing and output

`upload`, `screenshot`, `serve`, and `rewrite` share provider, path, overwrite, optimization, EXIF removal, maximum width, and trusted HTTP-source options. `--resize` means maximum width, only scales down, and is not a longest-edge setting. `--optimize` keeps a processed result only if smaller: JPEG is re-encoded; opaque PNG tries JPEG/lossless WebP; transparent PNG tries lossless WebP. Formats without supported processing, including GIF, SVG, WebP, and AVIF, retain original bytes.

CLI formats are `url`, `markdown`, `html`, and `json`. The GUI's Markdown-link and BBCode formats are desktop copy features. `--name` and `--progress` require one file. `--no-copy` overrides `--copy` and configuration. `--quiet` or global `output.quiet` suppresses ordinary result output; disable quiet settings for machine calls.

## JSON contract

```sh
img upload photo.png --format json --no-copy --progress
```

The final stdout result has top-level `success` and `files`. Success entries contain local and remote paths, URL, provider, size, and content type. Failures retain `error` and may add `error_code`, `http_status`, and `retryable`. Consumers must tolerate older results without new fields.

```json
{
  "success": false,
  "files": [
    {
      "local_path": "photo.png",
      "success": false,
      "error": "Service rate limit reached; try again later",
      "error_code": "rate_limited",
      "http_status": 429,
      "retryable": true
    }
  ]
}
```

This illustrative result shows key fields. Explicit `upload --format json` also returns structured configuration and engine-initialization failures.

`--progress` writes one JSON event per stderr line with `stage`, `sent`, `total`, and optional `attempt`. Transferred bytes include multipart/JSON overhead. They are neither original image bytes nor server-confirmed success; use the final result.

| Exit code | Meaning                                                         |
| --------- | --------------------------------------------------------------- |
| 0         | Success                                                         |
| 1         | All uploads failed                                              |
| 2         | Argument, configuration, initialization, or other command error |
| 3         | Partial upload success                                          |
| 130       | Interrupted                                                     |

## Platform differences

macOS invokes `screencapture`. Linux Wayland uses desktop portals; X11 supports flameshot, scrot, gnome-screenshot, and ImageMagick import. Windows supports region, window, and full-screen capture. Clipboard access requires platform commands and a working graphical session.

`fetch` only downloads, with an 8 MiB default and a configurable 1–128 MiB limit. `rewrite` edits file arguments in place and reads stdin without files. `serve` defaults to `127.0.0.1:36677`. See [storage configuration](/img/en/docs/storage/) for configuration details.

## Complete command help

### img

```text
Upload images from files, screenshots or links. Rust CLI included with img GUI.

Usage: img [OPTIONS] <COMMAND>

Commands:
  references        Preview and repair Markdown image references with backups
  migrate           Plan and apply recoverable copies; preserve source files
  sync              Synchronize metadata through your own WebDAV or S3 storage
  library           Query the shared remote image library
  remote            Browse remote files or explicitly delete one version
  backup            Back up local settings and records to a new directory (never uploads)
  restore           Verify a backup, or restore it while img desktop is closed
  process           Preview processing locally without uploading or modifying the source
  tasks             Inspect and resume local tasks
  presets           Share processing presets through configured metadata sync
  restore-document  Restore a Markdown document from a backup created by img rewrite
  watch             Upload new or changed images after they settle in a directory
  import-config     Preview or import PicGo/PicList storage configurations
  check             Verify public image URLs without sending storage credentials
  upload            Upload local files or remote image URLs
  fetch             Download an image URL without uploading it
  screenshot        Capture and upload a screenshot (copies result by default)
  serve             Run a PicGo-compatible editor upload server
  rewrite           Upload image references and rewrite Markdown documents
  info              Inspect image dimensions, type and EXIF presence
  init              Configure a storage provider interactively or with flags
  provider          List, show, select, remove or test storage providers
  config            Inspect or change configuration
  completion        Print a shell completion script
  version           Print application version
  install-cli       Add the bundled CLI to a directory on PATH (no GUI required to run it)
  help              Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
  -V, --version          Print version
```

### img upload

```text
Upload local files or remote image URLs

Usage: img upload [OPTIONS] <FILES>...

Arguments:
  <FILES>...

Options:
      --config <CONFIG>
          Use this global configuration file
      --recursive
          Include images in directories and their subdirectories
      --preset <PRESET>
          [possible values: original, web, photo]
      --image-format <IMAGE_FORMAT>
          [possible values: original, png, jpeg, webp]
      --quality <QUALITY>
          JPEG encoding quality
      --max-edge <MAX_EDGE>

      --watermark <WATERMARK>
          Local image watermark placed at bottom right
      --watermark-opacity <WATERMARK_OPACITY>

      --reuse
          Reuse a previous link for identical processed bytes in this destination
      --force
          Upload again instead of reusing a cached link
      --origin <ORIGIN>
          Source shown in the desktop library [default: cli] [possible values: cli, agent, editor, rewrite, screenshot]
      --no-history
          Do not retain a local library record or image copy
      --provider <PROVIDER>
          Storage provider name [default: ""]
      --path <PATH>
          Remote path prefix [default: ""]
      --overwrite

      --optimize
          Compress images before uploading
      --strip-exif
          Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>
          [default: 0]
      --allow-insecure
          Allow trusted plain HTTP image sources
      --format <FORMAT>
          [possible values: url, markdown, html, json]
      --copy

      --no-copy

      --quiet

      --verbose

      --name <NAME>
          [default: ""]
      --progress
          Write JSON progress events to stderr (one file only)
  -h, --help
          Print help
```

### img fetch

```text
Download an image URL without uploading it

Usage: img fetch [OPTIONS] --output <OUTPUT> <URL>

Arguments:
  <URL>

Options:
      --config <CONFIG>      Use this global configuration file
      --output <OUTPUT>
      --max-size <MAX_SIZE>  [default: 8388608]
      --allow-insecure
  -h, --help                 Print help
```

### img screenshot

```text
Capture and upload a screenshot (copies result by default)

Usage: img screenshot [OPTIONS]

Options:
      --config <CONFIG>
          Use this global configuration file
      --output <OUTPUT>
          Save the capture locally without uploading or copying a link
      --preset <PRESET>
          [possible values: original, web, photo]
      --image-format <IMAGE_FORMAT>
          [possible values: original, png, jpeg, webp]
      --quality <QUALITY>
          JPEG encoding quality
      --max-edge <MAX_EDGE>

      --watermark <WATERMARK>
          Local image watermark placed at bottom right
      --watermark-opacity <WATERMARK_OPACITY>

      --reuse
          Reuse a previous link for identical processed bytes in this destination
      --force
          Upload again instead of reusing a cached link
      --origin <ORIGIN>
          Source shown in the desktop library [default: cli] [possible values: cli, agent, editor, rewrite, screenshot]
      --no-history
          Do not retain a local library record or image copy
      --provider <PROVIDER>
          Storage provider name [default: ""]
      --path <PATH>
          Remote path prefix [default: ""]
      --overwrite

      --optimize
          Compress images before uploading
      --strip-exif
          Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>
          [default: 0]
      --allow-insecure
          Allow trusted plain HTTP image sources
      --region

      --window

      --format <FORMAT>
          [possible values: url, markdown, html, json]
      --no-copy

      --verbose

  -h, --help
          Print help
```

### img serve

```text
Run a PicGo-compatible editor upload server

Usage: img serve [OPTIONS]

Options:
      --config <CONFIG>
          Use this global configuration file
      --preset <PRESET>
          [possible values: original, web, photo]
      --image-format <IMAGE_FORMAT>
          [possible values: original, png, jpeg, webp]
      --quality <QUALITY>
          JPEG encoding quality
      --max-edge <MAX_EDGE>

      --watermark <WATERMARK>
          Local image watermark placed at bottom right
      --watermark-opacity <WATERMARK_OPACITY>

      --reuse
          Reuse a previous link for identical processed bytes in this destination
      --force
          Upload again instead of reusing a cached link
      --origin <ORIGIN>
          Source shown in the desktop library [default: cli] [possible values: cli, agent, editor, rewrite, screenshot]
      --no-history
          Do not retain a local library record or image copy
      --provider <PROVIDER>
          Storage provider name [default: ""]
      --path <PATH>
          Remote path prefix [default: ""]
      --overwrite

      --optimize
          Compress images before uploading
      --strip-exif
          Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>
          [default: 0]
      --allow-insecure
          Allow trusted plain HTTP image sources
      --bind <BIND>
          [default: 127.0.0.1]
      --port <PORT>
          [default: 36677]
  -h, --help
          Print help
```

### img rewrite

```text
Upload image references and rewrite Markdown documents

Usage: img rewrite [OPTIONS] [FILES]...

Arguments:
  [FILES]...

Options:
      --config <CONFIG>
          Use this global configuration file
      --dry-run
          List image references without uploading or changing documents
      --report <REPORT>
          Write per-image successes and failures to a JSON report
      --preset <PRESET>
          [possible values: original, web, photo]
      --image-format <IMAGE_FORMAT>
          [possible values: original, png, jpeg, webp]
      --quality <QUALITY>
          JPEG encoding quality
      --max-edge <MAX_EDGE>

      --watermark <WATERMARK>
          Local image watermark placed at bottom right
      --watermark-opacity <WATERMARK_OPACITY>

      --reuse
          Reuse a previous link for identical processed bytes in this destination
      --force
          Upload again instead of reusing a cached link
      --origin <ORIGIN>
          Source shown in the desktop library [default: cli] [possible values: cli, agent, editor, rewrite, screenshot]
      --no-history
          Do not retain a local library record or image copy
      --provider <PROVIDER>
          Storage provider name [default: ""]
      --path <PATH>
          Remote path prefix [default: ""]
      --overwrite

      --optimize
          Compress images before uploading
      --strip-exif
          Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>
          [default: 0]
      --allow-insecure
          Allow trusted plain HTTP image sources
      --stdout

  -h, --help
          Print help
```

### img info

```text
Inspect image dimensions, type and EXIF presence

Usage: img info [OPTIONS] <FILES>...

Arguments:
  <FILES>...

Options:
      --config <CONFIG>  Use this global configuration file
      --format <FORMAT>  [default: text] [possible values: text, json]
  -h, --help             Print help
```

### img init

```text
Configure a storage provider interactively or with flags

Usage: img init [OPTIONS]

Options:
      --authorization <AUTHORIZATION>
          Authorization header reference, for HTTP or WebDAV [default: ""]
      --config <CONFIG>
          Use this global configuration file
      --type <KIND>
          [default: ""]
      --name <NAME>
          [default: ""]
      --url <URL>
          [default: ""]
      --url-json-path <URL_JSON_PATH>
          [default: data.url]
      --method <METHOD>
          [default: POST] [possible values: POST, PUT, PATCH]
      --file-field <FILE_FIELD>
          [default: file]
      --endpoint <ENDPOINT>
          [default: ""]
      --region <REGION>
          [default: auto]
      --bucket <BUCKET>
          [default: ""]
      --access-key <ACCESS_KEY>
          [default: ""]
      --secret-key <SECRET_KEY>
          [default: ""]
      --session-token <SESSION_TOKEN>
          [default: ""]
      --public-url <PUBLIC_URL>
          [default: ""]
      --path-style

      --allow-insecure

      --owner <OWNER>
          [default: ""]
      --repo <REPO>
          [default: ""]
      --branch <BRANCH>
          [default: main]
      --token <TOKEN>
          [default: ""]
      --commit-message <COMMIT_MESSAGE>
          [default: "upload: {path}"]
  -h, --help
          Print help
```

### img provider

```text
List, show, select, remove or test storage providers

Usage: img provider [OPTIONS] <COMMAND>

Commands:
  list
  show
  use
  remove
  test
  help    Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider list

```text
Usage: img provider list [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider show

```text
Usage: img provider show [OPTIONS] <NAME>

Arguments:
  <NAME>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider use

```text
Usage: img provider use [OPTIONS] <NAME>

Arguments:
  <NAME>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider remove

```text
Usage: img provider remove [OPTIONS] <NAME>

Arguments:
  <NAME>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider test

```text
Usage: img provider test [OPTIONS] <NAME>

Arguments:
  <NAME>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config

```text
Inspect or change configuration

Usage: img config [OPTIONS] <COMMAND>

Commands:
  path
  list
  validate
  get
  set
  unset
  help      Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config path

```text
Usage: img config path [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config list

```text
Usage: img config list [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config validate

```text
Usage: img config validate [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config get

```text
Usage: img config get [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config set

```text
Usage: img config set [OPTIONS] <KEY> <VALUE>

Arguments:
  <KEY>
  <VALUE>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config unset

```text
Usage: img config unset [OPTIONS] <KEY>

Arguments:
  <KEY>

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img completion

```text
Print a shell completion script

Usage: img completion [OPTIONS] [SHELL]

Arguments:
  [SHELL]  [default: bash] [possible values: bash, elvish, fish, powershell, zsh]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img version

```text
Print application version

Usage: img version [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img install-cli

```text
Add the bundled CLI to a directory on PATH (no GUI required to run it)

Usage: img install-cli [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
      --json             Return a structured installation result
      --dir <DIR>
  -h, --help             Print help
```

### img process

```text
Preview processing locally without uploading or modifying the source

Usage: img process [OPTIONS] [FILES]...

Arguments:
  [FILES]...

Options:
      --config <CONFIG>
          Use this global configuration file
      --output <OUTPUT>

      --output-dir <OUTPUT_DIR>

      --recipe <RECIPE>
          A versioned ProcessingPlan JSON file
      --resume <RESUME>
          Resume a saved processing task
      --preview
          Render without adding a persistent task; uses the export renderer
      --prepare
          Save the task without running it
      --inputs-manifest <INPUTS_MANIFEST>
          Input snapshots with per-image annotations and optional source asset IDs
      --preset <PRESET>
          [possible values: original, web, photo]
      --image-format <IMAGE_FORMAT>
          [possible values: original, png, jpeg, webp]
      --quality <QUALITY>
          JPEG encoding quality
      --max-edge <MAX_EDGE>

      --watermark <WATERMARK>
          Local image watermark placed at bottom right
      --watermark-opacity <WATERMARK_OPACITY>

      --reuse
          Reuse a previous link for identical processed bytes in this destination
      --force
          Upload again instead of reusing a cached link
      --origin <ORIGIN>
          Source shown in the desktop library [default: cli] [possible values: cli, agent, editor, rewrite, screenshot]
      --no-history
          Do not retain a local library record or image copy
      --provider <PROVIDER>
          Storage provider name [default: ""]
      --path <PATH>
          Remote path prefix [default: ""]
      --overwrite

      --optimize
          Compress images before uploading
      --strip-exif
          Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>
          [default: 0]
      --allow-insecure
          Allow trusted plain HTTP image sources
  -h, --help
          Print help
```

### img library

```text
Query the shared remote image library

Usage: img library [OPTIONS] <COMMAND>

Commands:
  delete-plan
  delete
  preview
  scopes
  index
  list
  show
  check
  download
  hide
  cache
  help         Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img sync

```text
Synchronize metadata through your own WebDAV or S3 storage

Usage: img sync [OPTIONS] <COMMAND>

Commands:
  configure  Store a dedicated sync connection in this device's system keychain
  status
  run
  pause
  resume
  conflicts
  resolve    Resolve one conflict using the explicitly selected event
  help       Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img migrate

```text
Plan and apply recoverable copies; preserve source files

Usage: img migrate [OPTIONS] <COMMAND>

Commands:
  source  Attach a local original to a failed item, without uploading; then retry the task
  plan    Inspect destinations without uploading; save a fixed plan for review
  apply   Apply or retry this exact plan; verified successes are preserved
  show    Inspect locally saved migration progress
  help    Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img references

```text
Preview and repair Markdown image references with backups

Usage: img references [OPTIONS] <COMMAND>

Commands:
  scan     Scan only the selected directory; no document writes
  show
  apply    Apply the saved preview; recheck destination images and original documents
  restore  Preview restoration; apply the returned task with references apply --yes
  export   Export the report and document backups into a new directory
  help     Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img tasks

```text
Inspect and resume local tasks

Usage: img tasks [OPTIONS] <COMMAND>

Commands:
  upload  Upload saved processing outputs without reprocessing their pixels
  list
  show
  retry
  help    Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```
