---
title: 'Editors and agents'
description: 'Make image uploads part of writing, scripts, and automation.'
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

Install these two extensions from their source directories in the repository:

- [VS Code integration](https://github.com/liyown/img/tree/main/integrations/vscode): paste image uploads in Markdown and upload from the file explorer.
- [Raycast integration](https://github.com/liyown/img/tree/main/integrations/raycast): screenshot, clipboard, and file upload commands.

Follow each README and configure a working `img` executable. The GUI already contains it; add the terminal entry point from About and updates.

## Agents and scripts

```sh
img upload cover.png --format json --no-copy --progress
```

stdout contains final JSON; stderr carries separate progress events. `--progress` accepts one file only. Check the exit code and each file's `success`. Failures retain `error` and may include `error_code`, `http_status`, and `retryable`. A progress value of 100% is not server-confirmed success.

Install the companion Skill for assistants that support Agent Skills and can run local commands:

```sh
npx skills add liyown/img --skill img-uploader
```

Install the native CLI and configure your default storage first. Your agent can then upload local images and insert the returned links into an article. The Skill checks files and configuration, reads per-file JSON results, and preserves successful links when other uploads fail. It reuses existing storage settings without requiring credentials in the conversation. Read the full [Skill workflow](https://github.com/liyown/img/blob/main/skills/img-uploader/SKILL.md).

The [GitHub Action](https://github.com/liyown/img/blob/main/action.yml) uploads and rewrites Markdown images in a workflow. Its installer fetches the latest released CLI; pinning the Action commit does not pin the downloaded CLI. For a reproducible version, download and verify a specific CLI release in your workflow, then call `img rewrite`.
