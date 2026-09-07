---
title: "Editors and agents"
description: "Make image uploads part of writing, scripts, and automation."
locale: en
topic: integrations
order: 3
---
## Typora

Choose a custom image-upload command:

```sh
img "${filepath}"
```

Ensure the editor can resolve `img`. If its PATH differs from your terminal, use the CLI's absolute path.

## Obsidian and the PicGo protocol

```sh
img serve --bind 127.0.0.1 --port 36677
```

Editor plugins supporting PicGo uploads can use `http://127.0.0.1:36677/upload`. The service listens locally by default and uses your current storage configuration. Add `--optimize --strip-exif --resize 1200` as needed. Do not expose this unauthenticated local proxy publicly.

## Rewrite Markdown image references

```sh
img rewrite article.md
img rewrite first.md second.md --optimize
img rewrite article.md --stdout
cat article.md | img rewrite
```

File arguments are rewritten in place by default. Use `--stdout` to inspect results first. Markdown images and HTML `img` references are supported; alt text and titles are retained while image addresses change. Unsupported references such as `data:` remain unchanged. Keep a versioned copy of important documents.

## VS Code and Raycast

Two source extensions are included in the repository; this is not a claim of marketplace availability:

- [VS Code integration](https://github.com/liyown/img/tree/main/integrations/vscode): paste image uploads in Markdown and upload from the file explorer.
- [Raycast integration](https://github.com/liyown/img/tree/main/integrations/raycast): screenshot, clipboard, and file upload commands.

Follow each README and configure a working `img` executable. The GUI already contains it; add the terminal entry point from About and updates.

## Agents and scripts

```sh
img upload cover.png --format json --no-copy --progress
```

stdout contains final JSON; stderr carries separate progress events. `--progress` accepts one file only. Check the exit code and each file's `success`. Failures retain `error` and may include `error_code`, `http_status`, and `retryable`. A progress value of 100% is not server-confirmed success.

The repository's [SKILL.md](https://github.com/liyown/img/blob/main/skills/img-uploader/SKILL.md) documents agent usage; the [GitHub Action](https://github.com/liyown/img/blob/main/action.yml) supports workflows. The Action installer still fetches a published CLI, which may currently be the older Go version. To use Rust 0.3.0 in CI, build the CLI from a reviewed commit as described in the installation guide, then invoke img rewrite directly. Pinning the Action commit does not pin its downloaded CLI version.
