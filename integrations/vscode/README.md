# img Uploader — VS Code Extension

Paste or upload images directly to your configured [img](https://github.com/liyown/img) image host from VS Code.

## Requirements

`img` must be installed and configured:

```sh
curl -fsSL https://raw.githubusercontent.com/liyown/img/main/install.sh | sh
img init   # configure your provider (R2, OSS, GitHub, HTTP…)
```

## Features

| Action | How |
|--------|-----|
| Paste clipboard image and upload | `Cmd+Alt+V` (macOS) / `Ctrl+Alt+V` |
| Upload image file via palette | `Ctrl+Shift+P` → **img: Paste and Upload Image** |
| Upload from Explorer right-click | Right-click `.png/.jpg/…` → **Upload with img** |

All three insert a Markdown link at the cursor position (or copy to clipboard if no editor is open).

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `img.executablePath` | `img` | Path to the img binary |
| `img.outputFormat` | `markdown` | Inserted format: `url`, `markdown`, `html` |
| `img.provider` | _(default)_ | Override provider |
| `img.optimize` | `false` | Compress before upload |
| `img.stripExif` | `false` | Strip EXIF metadata |

## Development

```sh
cd integrations/vscode
npm install
npm run compile
# Press F5 in VS Code to launch Extension Development Host
```
