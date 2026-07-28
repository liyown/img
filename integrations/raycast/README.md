# img Uploader — Raycast Extension

Upload images to your configured [img](https://github.com/liyown/img) host directly from Raycast.

## Requirements

`img` must be installed and configured:

```sh
curl -fsSL https://raw.githubusercontent.com/liyown/img/main/install.sh | sh
img init   # configure your provider
```

## Commands

| Command | Description |
|---------|-------------|
| **Upload Screenshot** | Captures a screenshot (uses `img screenshot`) and copies the Markdown link |
| **Upload Clipboard Image** | Reads the image on the clipboard and uploads it |
| **Upload Image File** | File picker — upload one or more image files |

All commands copy the result to the clipboard in your preferred format.

## Preferences

| Preference | Default | Description |
|------------|---------|-------------|
| img executable | `img` | Path to the img binary |
| Output format | `markdown` | `markdown`, `url`, or `html` |
| Provider override | _(default)_ | Override default provider |
| Compress before upload | off | Pass `--optimize` |
| Strip EXIF metadata | off | Pass `--strip-exif` |

## Development

```sh
cd integrations/raycast
npm install
npm run dev
```
