<div align="center">
  <img src="site/public/favicon.svg" width="72" alt="img">
  <h1>img</h1>
  <p><strong>原生图片上传工具</strong></p>
  <p>将图片上传到自有存储，生成可直接使用的链接。<br>原生桌面应用、独立 CLI 与 Agent Skill，支持 macOS、Windows 和 Linux。</p>
  <p><a href="https://liyown.github.io/img/install/#gui"><strong>下载桌面版</strong></a> · <a href="https://liyown.github.io/img/install/#cli">下载 CLI</a> · <a href="https://liyown.github.io/img/">官网</a> · <a href="https://liyown.github.io/img/docs/">文档</a> · <a href="README.en.md">English</a></p>

[![Desktop release](https://github.com/liyown/img/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/desktop-release.yml)
[![CLI release](https://github.com/liyown/img/actions/workflows/release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built_with-Rust-dea584)](Cargo.toml)

</div>

[![img 原生图库：查找图片、预览并复制链接；macOS 演示界面](site/public/screenshots/gallery.webp)](https://liyown.github.io/img/install/#gui)

## 功能

img 使用 Rust 构建，桌面端采用 GPUI，内置同版本 CLI。桌面上传、编辑器集成和自动化任务共享存储配置与图片处理能力；独立 CLI 无需启动桌面应用或安装语言运行时。

- 快捷上传：支持文件、剪贴板、截图和外链导入。快捷键可直接上传，完成后自动复制 URL、Markdown 等格式的链接。
- 自有存储：连接 R2、S3、OSS、GitHub 或 HTTP 图床，沿用现有存储和公开域名，支持按项目选择存储源。
- 本地图库：搜索和预览桌面上传记录，支持网格、列表、跨搜索多选及批量复制链接。
- 图片处理：批量上传前压缩、缩放或移除 JPEG EXIF，保留原文件。
- 文章转存：使用 `img rewrite` 上传 Markdown 中的本地图片和外链图片，替换引用并保留 alt 与 title。
- 自动化接口：原生 CLI 提供 JSON 输出、逐文件结果和退出码，配套 Skill 支持 AI Agent 调用。

## 安装

从[安装页](https://liyown.github.io/img/install/#gui)下载最新稳定版。桌面安装包包含 CLI，也可单独下载命令行版本，无需从源码编译。

| 系统 | 桌面版（包含 CLI） | 独立 CLI |
| --- | --- | --- |
| macOS 13+ | Apple silicon / Intel · DMG、ZIP | ARM64 / x64 |
| Windows 10/11 | x64 · EXE 安装向导、便携 ZIP | x64 |
| Ubuntu 24.04 / Debian 13+ | x64 · DEB | Linux ARM64 / x64 |

安装包由 GitHub Actions 在各平台构建、测试并发布，附 SHA-256 校验文件。[全部发行版本](https://github.com/liyown/img/releases) · [实际验收记录](desktop/install-qa.md)

<details>
<summary>社区包首次打开与平台说明</summary>

- macOS 社区版未经过 Apple 公证。确认下载来源后，可在「系统设置 → 隐私与安全性」选择「仍要打开」。[完整安装说明](https://liyown.github.io/img/install/#gui)
- Windows 社区包未进行代码签名。Linux GUI 需要图形桌面、Vulkan 驱动和已解锁的 Secret Service。
- Linux 全局快捷键需要 X11，Wayland 可使用窗口入口；截图需要系统截图工具。Windows 当前截图为全屏。macOS 提供菜单栏后台上传。
- 「设置 → 关于与更新」检查并下载校验后续版本，升级保留配置与图库。独立 CLI 可重新运行安装器升级。

</details>

## 快速开始

### 桌面应用

1. 安装并打开 img，在「设置 → 存储源」添加图床，设为默认。
2. 复制一张图片，按下快捷键直接上传。
3. 上传成功后粘贴链接。在设置中选择 Markdown、URL 等输出格式。

| 快捷操作 | macOS | Windows / Linux X11 |
| --- | --- | --- |
| 上传剪贴板并复制链接 | `⌘⌥U` | `Ctrl+Alt+U` |
| 截图上传并复制链接 | `⌘⌥S` | `Ctrl+Alt+S` |

也可从窗口选择文件、粘贴图片或添加外链，确认队列后点击上传。

### 命令行

从[安装页](https://liyown.github.io/img/install/#cli)下载独立 CLI，或在桌面版「关于与更新」中添加终端命令：

```sh
img init                                  # 连接自己的图床
img photo.png --format markdown --copy     # 上传并复制 Markdown 链接
img upload a.png b.jpg --format json       # 批量上传，供脚本读取
img rewrite article.md --stdout            # 转存文章图片，输出改写后的 Markdown
```

上传示例输出（地址取决于你的存储配置）：

```markdown
![photo.png](https://img.example.com/photo.png)
```

[Cloudflare R2 / OSS / GitHub 配置示例](docs/cli.md#初始化) · [完整命令与配置手册](docs/cli.md)

## 自动化与 AI Agent

CLI 可独立用于脚本、编辑器和 AI Agent。使用 JSON 输出读取每张图片的上传结果，结合退出码处理失败；批量上传部分失败时，成功链接仍会返回。

```sh
img upload ./assets/chart.png --format json --no-copy
```

配套的 [img-uploader Skill](skills/img-uploader) 定义了文件检查、配置校验、上传和结果解析流程。支持 Agent Skills 且具备本地命令执行能力的助手，可通过 Skill 将截图或图表上传并引用到文档中。

```sh
npx skills add liyown/img --skill img-uploader
```

使用前需安装 CLI 并配置默认存储源。凭据沿用本地配置，无需写入提示词。[Skill 使用说明](skills/img-uploader/SKILL.md) · [JSON 输出与退出码](docs/cli.md#json-输出)

## 编辑器与工作流

| 场景 | 接入方式 |
| --- | --- |
| Markdown 写作 | 截图上传后自动复制图片链接 |
| Typora | 自定义上传命令：`img "${filepath}"` |
| Obsidian | 运行 `img serve`，通过 Image Auto Upload 插件连接 PicGo 兼容服务 |
| 文章迁移 | `img rewrite` 批量转存图片并替换引用 |
| AI Agent | 安装 Skill，通过 CLI 获取结构化上传结果 |

[集成示例](docs/cli.md#集成) · [Agent Skill](skills/img-uploader) · [GitHub Action](action.yml)

### 迁移现有配置

使用已有存储服务时，在 img 中添加对应配置，验证上传与公开 URL 后，将编辑器的上传入口指向 img。更换客户端不影响已有文章链接。

目前不支持自动导入其他客户端的配置与历史记录。图库仅管理本地记录，清理操作保留原文件和远端图片；不提供远端文件管理或 PicGo 插件兼容层。

## 支持的存储与格式

**Cloudflare R2 · S3 兼容服务 · 阿里云 OSS · GitHub 仓库 · 自定义 HTTP 接口**

支持 PNG、JPEG、GIF、WebP、SVG、AVIF。可以为不同项目选择不同存储源，配置默认输出格式与图片处理选项。[存储配置指南](https://liyown.github.io/img/docs/storage/)

## 开发与反馈

遇到问题，欢迎提交系统版本、img 版本和复现步骤；请勿附上存储密钥。[报告问题或提出需求](https://github.com/liyown/img/issues)

```sh
git clone https://github.com/liyown/img.git
cd img
cargo test --locked --workspace
```

代码分为 [共享上传核心](crates/img-core)、[独立 CLI](crates/img-cli) 和 [原生桌面应用](desktop)。[本地构建与发行流程](desktop/RELEASING.md)
