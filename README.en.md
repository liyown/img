<div align="center">
  <img src="site/public/favicon.svg" width="72" alt="img">
  <h1>img</h1>
  <p><strong>Capture an image. Paste a link. Keep writing.</strong></p>
  <p>Upload to your own storage and get links ready for your next post.<br>A native Rust desktop app + standalone CLI for macOS, Windows, and Linux.</p>
  <p><a href="https://liyown.github.io/img/en/install/#gui"><strong>Download desktop</strong></a> · <a href="https://liyown.github.io/img/en/install/#cli">Download CLI</a> · <a href="https://liyown.github.io/img/en/">Website</a> · <a href="https://liyown.github.io/img/en/docs/">Docs</a> · <a href="README.md">中文</a></p>

[![Desktop release](https://github.com/liyown/img/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/desktop-release.yml)
[![CLI release](https://github.com/liyown/img/actions/workflows/release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built_with-Rust-dea584)](Cargo.toml)

</div>

[![img native gallery: find images, preview, and copy links; macOS demo](site/public/screenshots/gallery.webp)](https://liyown.github.io/img/en/install/#gui)

## Fewer steps between an image and your next sentence

Whether you write blog posts, take notes, or maintain documentation, configure your storage once and turn screenshots into links you can paste.

- **A shortcut from screenshot to link.** Upload your clipboard or capture a screenshot with a global shortcut. The resulting link is copied in your chosen format.
- **Your images, your storage, your domain.** Connect Cloudflare R2, S3, Alibaba Cloud OSS, GitHub, or a custom HTTP host with your own public image URL.
- **One tool for your desktop and scripts.** A native Rust / GPUI interface bundles the matching CLI. The standalone CLI works without the desktop app.
- **Find the images you already uploaded.** Search a local gallery, switch between grid and list, and copy links in batches across search results. Clearing local records preserves original files and remote images.
- **Handle a whole article.** Batch upload, optimize, resize, strip JPEG EXIF, or use `img rewrite` to upload Markdown images and replace their references.
- **Works with your editor and agents.** Typora custom commands, a PicGo-compatible local service, structured JSON output, and a companion Agent Skill.

> img is an upload client: bring your own storage service and publicly accessible image URL. The gallery manages local records; remote file management and the PicGo plugin ecosystem are outside the current scope.

## Download and start

**[Get the latest stable release →](https://liyown.github.io/img/en/install/#gui)** No Rust, Go, or build tools required.

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

## Fit it into your workflow

| Your workflow | With img |
| --- | --- |
| Blog posts and notes | Capture, upload, and paste the copied link into Markdown |
| Typora | Set the custom upload command to `img "${filepath}"` |
| Obsidian | Run `img serve` and connect the Image Auto Upload plugin to the PicGo-compatible service |
| Article migration | Use `img rewrite` for local and remote images while preserving alt text and titles |
| Agent workflows | Read structured CLI JSON and reuse configured storage with the companion Skill |

```sh
npx skills add liyown/img --skill img-uploader
```

[Integration examples](docs/cli.en.md#integrations) · [Agent Skill](skills/img-uploader) · [GitHub Action](action.yml)

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

If img saves you the upload-and-copy routine, give it a star or share it with someone who writes.
