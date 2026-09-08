# img CLI reference

[README](../README.en.md)

## Setup

Create a provider and set it as the default before first use.

### Interactive setup

```sh
img init
```

Follow the prompts, then verify:

```sh
img config validate
img provider list
```

### Cloudflare R2

```sh
export IMG_R2_ACCESS_KEY='your-access-key'
export IMG_R2_SECRET_KEY='your-secret-key'
```

```sh
img init \
  --type s3 --name r2 \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --region auto --bucket images \
  --access-key '${IMG_R2_ACCESS_KEY}' \
  --secret-key '${IMG_R2_SECRET_KEY}' \
  --public-url https://img.example.com \
  --path-style
```

### Alibaba Cloud OSS

```sh
export IMG_ALIYUN_ACCESS_KEY_ID='your-access-key-id'
export IMG_ALIYUN_ACCESS_KEY_SECRET='your-access-key-secret'
```

```sh
img init \
  --type s3 --name aliyun \
  --endpoint https://oss-cn-shenzhen.aliyuncs.com \
  --region oss-cn-shenzhen --bucket your-bucket \
  --access-key '${IMG_ALIYUN_ACCESS_KEY_ID}' \
  --secret-key '${IMG_ALIYUN_ACCESS_KEY_SECRET}' \
  --public-url https://img.example.com
```

### Custom HTTP endpoint

```sh
img init --type http --name custom \
  --url https://example.com/api/upload \
  --url-json-path data.url
```

To add fixed headers or form fields, edit the config file:

```toml
[providers.custom.headers]
Authorization = "Bearer ${IMG_HTTP_TOKEN}"

[providers.custom.fields]
folder = "images"
```

### GitHub repository

```sh
img init --type github --name github \
  --owner your-name --repo images \
  --token '${IMG_GITHUB_TOKEN}'
```

Or add manually to the config file:

```toml
[providers.github]
type = "github"
owner = "your-name"
repo = "images"
branch = "main"
token = "${IMG_GITHUB_TOKEN}"
```

```sh
export IMG_GITHUB_TOKEN='your-token'
img config validate
```

---

## Commands

### img upload — upload images

```sh
img screenshot.png                          # upload, print URL
img screenshot.png --format markdown        # Markdown output
img upload a.png b.jpg c.webp              # upload multiple files
img https://example.com/photo.jpg          # rehost a remote URL
img http://192.168.1.10/img.png --allow-insecure
```

Common flags:

```sh
--format url|markdown|html|json    # output format
--provider <name>                  # override default provider
--path posts/assets                # remote path prefix
--name cover.png                   # remote filename
--overwrite                        # overwrite existing file
--copy / --no-copy                 # copy result to clipboard
--quiet                            # suppress stdout (use with --copy)
--verbose                          # verbose logging
```

Processing flags (see [Processing options](#processing-options)):

```sh
--optimize       # compress before upload
--strip-exif     # remove EXIF metadata
--resize 1200    # downscale to max width 1200 px
```

### img screenshot — screenshot and upload in one step

```sh
img screenshot                   # full screen, result auto-copied
img screenshot --region          # interactive area selection
img screenshot --window          # active window
img screenshot --format markdown
img screenshot --optimize
img screenshot --no-copy
```

- **macOS:** uses the built-in `screencapture` command
- **Linux:** desktop portals on Wayland; `flameshot`, `scrot`, `gnome-screenshot`, or `import` on X11
- **Windows:** region, window, and full-screen capture; Escape cancels

### img serve — editor image upload proxy

Starts a PicGo-compatible local HTTP server so any editor can upload through `img`:

```sh
img serve                        # 127.0.0.1:36677 (PicGo default port)
img serve --port 9000
img serve --optimize --strip-exif --resize 1200
```

**Typora** — Preferences → Image → Upload Image → Custom Command:

```
img "${filepath}"
```

**Obsidian** (Image Auto Upload plugin):

```
Upload server URL: http://127.0.0.1:36677/upload
```

### img rewrite — batch-rehost images in a Markdown article

Uploads every image referenced in a Markdown article and rewrites the links:

```sh
img rewrite article.md                     # rewrite file in-place
img rewrite *.md                           # rewrite multiple articles
img rewrite article.md --stdout            # print result to stdout
cat article.md | img rewrite               # stdin → stdout
img rewrite article.md --optimize --strip-exif
```

- Both local paths and remote URLs are uploaded and replaced
- Supports `![alt](path "title")` and `<img src="path">`
- Alt text, titles, and attributes are preserved; only URLs are replaced
- Unresolvable references (`data:` URIs, etc.) are kept as-is

### img info — inspect image metadata

```sh
img info photo.jpg screenshot.png          # table output
img info *.jpg --format json               # JSON output
```

Sample output:

```
photo.jpg                               JPEG    3000×2000    2.4 MB  ⚠ EXIF
screenshot.png                          PNG     1440×900     156 KB
icon.svg                                SVG     –            4 KB

⚠  Files marked with EXIF may contain GPS location and device information.
   Remove before uploading: img upload <file> --strip-exif
```

---

## Processing options

These flags work with `upload`, `screenshot`, `rewrite`, and `serve`:

### --optimize

Compress before upload. The smaller of the original and the compressed version is used:

| Format | Action |
|--------|--------|
| JPEG | Re-encode at quality 85 |
| Opaque PNG | Pick the smaller of JPEG q85 and lossless WebP |
| Transparent PNG | Try lossless WebP (preserves alpha) |
| SVG / GIF / WebP / AVIF | Upload unchanged |

```sh
img photo.jpg --optimize --verbose   # show per-file savings
```

### --strip-exif

Remove EXIF metadata (GPS coordinates, device model, timestamps) from JPEG files before upload. Metadata-only removal avoids re-encoding when orientation is already normal. Rotated photos are oriented correctly before metadata is removed:

```sh
img photo.jpg --strip-exif
img photo.jpg --strip-exif --optimize
```

### --resize \<width\>

Downscale the image to fit the given max width in pixels. Never upscales:

```sh
img photo.jpg --resize 1200
img photo.jpg --resize 1200 --optimize
```

### Persistent defaults

Set these once to avoid repeating flags:

```sh
img config set upload.strip_exif true
img config set upload.max_width 1200
img config set upload.retry_count 3     # auto-retry on transient failures
```

Once set, all commands (including `img serve`) apply them automatically.

---

## Configuration

```sh
img config list                            # show current config
img config path                            # print config file path
img config validate                        # validate config
img config get upload.strip_exif           # read a key
img config set output.format markdown      # default to Markdown output
img config set output.copy true            # always copy result
img config set output.quiet true           # quiet mode
img config set upload.retry_count 3        # auto-retry up to 3 times
img config unset upload.max_width          # restore default
```

**Priority** (low → high): global config → project `.img.toml` → env vars → CLI flags

Project-level `.img.toml` can only reference providers already defined globally and set output format or path options. Credentials must live in the global config.

---

## Provider management

```sh
img provider list              # list all providers
img provider show r2           # show config (sensitive fields hidden)
img provider use github        # switch default provider
img provider test r2           # test connectivity
img provider remove old        # delete a provider
```

---

## JSON output

```sh
img upload a.png b.png --format json --no-copy
```

```json
{
  "success": true,
  "files": [
    {
      "local_path": "a.png",
      "success": true,
      "remote_path": "2026/07/a.png",
      "url": "https://img.example.com/2026/07/a.png",
      "provider": "r2",
      "size": 1024,
      "content_type": "image/png"
    }
  ]
}
```

On partial failure, successful results are preserved and the process exits with code `3`.

---

## Integrations

### Shell completions

```sh
img completion bash      # eval "$(img completion bash)"
img completion zsh       # eval "$(img completion zsh)" or write to a file in $fpath
img completion fish      # img completion fish > ~/.config/fish/completions/img.fish
```

### VS Code extension

`integrations/vscode/` — in Markdown editors, press `Cmd+Alt+V` (macOS) / `Ctrl+Alt+V` to paste a clipboard image and upload it. Right-click image files in the Explorer to upload.

### Raycast extension

`integrations/raycast/` — three commands for macOS Raycast: upload screenshot, upload clipboard image, upload file.

### GitHub Action

In a documentation repository, this Action automatically rewrites local image paths in Markdown to CDN URLs on every push:

```yaml
- uses: liyown/img@v0.4.0
  with:
    provider-type: s3
    s3-endpoint: https://ACCOUNT.r2.cloudflarestorage.com
    s3-bucket: images
    s3-access-key: ${{ secrets.R2_ACCESS_KEY }}
    s3-secret-key: ${{ secrets.R2_SECRET_KEY }}
    s3-public-url: https://img.example.com
    optimize: 'true'
```

Full configuration in [action.yml](../action.yml).

---

## AI Agent usage

The repository ships a companion Skill: [skills/img-uploader](../skills/img-uploader).

```sh
npx skills add liyown/img --skill img-uploader
```

Global non-interactive install for Codex:

```sh
npx skills add liyown/img --skill img-uploader --agent codex --global --yes
```

---

## Security

- Reference credentials with `${ENV_NAME}` — never write secrets into config files or repositories
- Add project-level `.img.toml` to `.gitignore`
- `config list` and `provider show` redact sensitive fields
- Overwriting requires an explicit `--overwrite` flag

See [config.example.toml](../config.example.toml) for a full configuration example.

