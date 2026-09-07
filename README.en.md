<div align="center">
  <img src="site/public/favicon.svg" width="72" alt="img">
  <h1>img</h1>
  <p><strong>Upload a screenshot and paste the link.</strong></p>
  <p>Use desktop shortcuts yourself. Give your AI agent a native CLI and Skill.<br>Both use the same storage configuration on macOS, Windows, and Linux.</p>
  <p><a href="https://liyown.github.io/img/en/install/#gui"><strong>Download desktop</strong></a> · <a href="https://liyown.github.io/img/en/install/#cli">Download CLI</a> · <a href="https://liyown.github.io/img/en/">Website</a> · <a href="https://liyown.github.io/img/en/docs/">Docs</a> · <a href="README.md">中文</a></p>

[![Desktop release](https://github.com/liyown/img/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/desktop-release.yml)
[![CLI release](https://github.com/liyown/img/actions/workflows/release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built_with-Rust-dea584)](Cargo.toml)

</div>

[![img native gallery: find images, preview, and copy links; macOS demo](site/public/screenshots/gallery.webp)](https://liyown.github.io/img/en/install/#gui)

## Why switch to img

Capture an image, press the upload shortcut, and paste the copied link into your article. When an AI agent writes the article, it can call img to upload local screenshots or charts and insert the returned URLs. Both workflows use the same storage configuration.

The Rust / GPUI desktop app includes the matching native CLI. For terminal and automation work, download the command alone: it runs without the desktop app or Node.js. Uploads, image processing, and storage configuration share an implementation across both entry points.

Keep your existing R2, S3, OSS, GitHub, or HTTP host and public domain. Links in old articles remain unchanged. You can try the CLI with a few images before moving your everyday uploads to img.

Find previous desktop uploads in the local gallery, preview them, switch between grid and list, and copy links in batches across search results. Use `img rewrite` to upload a whole article's images and replace their references. Batch uploads also support optimization, resizing, and JPEG EXIF removal.

## Give your AI agent an upload tool

When an agent creates a chart or uses a local screenshot, it can upload the file through the native CLI and put the returned link into the article. The repository includes an installable [img-uploader Skill](skills/img-uploader) for assistants that support Agent Skills and can run local commands.

```sh
npx skills add liyown/img --skill img-uploader
```

Install the CLI and configure your default storage, then give the agent a task such as:

> Upload ./assets/chart.png to my default image host and insert the returned Markdown image link into article.md.

The Skill checks files and configuration, then uploads with `--format json --no-copy`. The CLI returns per-file results and exit codes, preserving successful URLs when other uploads fail. Your agent uses the configured storage without needing credentials in the conversation or controlling an upload window.

[Read the Skill workflow](skills/img-uploader/SKILL.md) · [CLI JSON output and exit codes](docs/cli.en.md#json-output)

## Download and start

[Download the latest stable release](https://liyown.github.io/img/en/install/#gui). You do not need Rust, Go, or build tools.

| System | Desktop, including CLI | Standalone CLI |
| --- | --- | --- |
| macOS 13+ | Apple silicon / Intel · DMG, ZIP | ARM64 / x64 |
| Windows 10/11 | x64 · EXE installer, portable ZIP | x64 |
| Ubuntu 24.04 / Debian 13+ | x64 · DEB | Linux ARM64 / x64 |

GitHub Actions builds, tests, and publishes native packages with SHA-256 checksums. [All releases](https://github.com/liyown/img/releases) · [Verification record](desktop/install-qa.md)

<details>
<summary>Community builds and platform notes</summary>

- macOS builds are not Apple-notarized. After verifying the source, use System Settings → Privacy & Security → Open Anyway if blocked. [Installation guide](https://liyown.github.io/img/en/install/#gui)
- Windows community packages are not code-signed. Linux GUI requires a graphical desktop, Vulkan drivers, and an unlocked Secret Service.
- Linux global shortcuts require X11; use window controls on Wayland. Screenshots require a system capture tool. Windows currently captures the full screen. Menu-bar background uploads are available on macOS.
- Settings → About and updates checks for and downloads verified updates, preserving configuration and the library. Rerun the installer to update the standalone CLI.

</details>

## Upload your first image

### From the desktop

1. Install img, add your image host in Settings → Storage, and set it as default.
2. Copy an image and press the upload shortcut.
3. Return to your article and paste the link. Choose Markdown, URL, or another format in Settings.

| Action | macOS | Windows / Linux X11 |
| --- | --- | --- |
| Upload clipboard and copy link | `⌘⌥U` | `Ctrl+Alt+U` |
| Capture, upload, and copy link | `⌘⌥S` | `Ctrl+Alt+S` |

You can also select files, paste images, or add remote URLs in the window, review the queue, and click Upload.

### From the terminal

Download the [standalone CLI](https://liyown.github.io/img/en/install/#cli), or add the terminal command from the desktop app's About and updates section:

```sh
img init                                  # Connect your image host
img photo.png --format markdown --copy     # Upload and copy a Markdown link
img upload a.png b.jpg --format json       # Batch upload for scripts
img rewrite article.md --stdout            # Upload article images and print rewritten Markdown
```

Example output; the URL depends on your storage configuration:

```markdown
![photo.png](https://img.example.com/photo.png)
```

[R2 / OSS / GitHub setup examples](docs/cli.en.md#setup) · [Full command reference](docs/cli.en.md)

## Keep your editor

| Your workflow | With img |
| --- | --- |
| Blog posts and notes | Capture, upload, and paste the copied link into Markdown |
| Typora | Set the custom upload command to `img "${filepath}"` |
| Obsidian | Run `img serve` and connect the Image Auto Upload plugin to the PicGo-compatible service |
| Article migration | Use `img rewrite` for local and remote images while preserving alt text and titles |
| Agent workflows | Read structured CLI JSON and reuse configured storage with the companion Skill |

[Integration examples](docs/cli.en.md#integrations) · [Agent Skill](skills/img-uploader) · [GitHub Action](action.yml)

### Moving from another uploader

Add your existing storage provider to img, upload one image, and check the returned URL before changing your editor's upload command. Provider settings need to be added again. img does not automatically import another app's configuration or gallery, or change links in existing articles.

If you rely on remote file management or PicGo plugins, keep your current tool for those tasks. The img gallery manages local records; clearing them preserves original files and remote images. You can start by using img only for agent and script uploads.

## Bring your storage

**Cloudflare R2 · S3-compatible services · Alibaba Cloud OSS · GitHub repositories · Custom HTTP endpoints**

PNG, JPEG, GIF, WebP, SVG, and AVIF are supported. Select storage per project and set defaults for output formats and image processing. [Storage guide](https://liyown.github.io/img/en/docs/storage/)

## Help make it better

Found a problem? Include your OS, img version, and reproduction steps, without storage credentials. [Report an issue or request a feature](https://github.com/liyown/img/issues)

```sh
git clone https://github.com/liyown/img.git
cd img
cargo test --locked --workspace
```

Explore the [shared upload core](crates/img-core), [standalone CLI](crates/img-cli), and [native desktop app](desktop). [Build and release guide](desktop/RELEASING.md)

Tell us which editor or agent you connected to img, and which step still gets in your way.
