<div align="center">
  <img src="site/public/favicon.svg" width="72" alt="img">
  <h1>img</h1>
  <p><strong>截图、上传、复制链接，一气呵成。</strong></p>
  <p>图片存到你自己的图床，链接直接贴进文章。<br>原生 Rust 桌面应用 + 独立 CLI，覆盖 macOS、Windows 和 Linux。</p>
  <p><a href="https://liyown.github.io/img/install/#gui"><strong>下载桌面版</strong></a> · <a href="https://liyown.github.io/img/install/#cli">下载 CLI</a> · <a href="https://liyown.github.io/img/">官网</a> · <a href="https://liyown.github.io/img/docs/">文档</a> · <a href="README.en.md">English</a></p>

[![Desktop release](https://github.com/liyown/img/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/desktop-release.yml)
[![CLI release](https://github.com/liyown/img/actions/workflows/release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built_with-Rust-dea584)](Cargo.toml)

</div>

[![img 原生图库：查找图片、预览并复制链接；macOS 演示界面](site/public/screenshots/gallery.webp)](https://liyown.github.io/img/install/#gui)

## 少做几步，多写一点

写博客、做笔记、维护文档，图片不该打断思路。配置一次存储源，之后把截图变成可以直接粘贴的链接。

- **截图后直接得到链接。** 全局快捷键触发剪贴板上传或截图上传，完成后自动复制所选格式，回到编辑器直接粘贴。
- **图片和域名由你掌握。** 连接 Cloudflare R2、S3、阿里云 OSS、GitHub 或自定义 HTTP 图床，使用自己的公开访问地址。
- **桌面操作与脚本用同一套工具。** Rust / GPUI 原生界面，包内附带同版本 CLI；也可以只下载独立命令行。
- **传过的图片，随时找回来。** 本地图库支持搜索、网格 / 列表、跨搜索多选和批量复制；清理本地记录保留原文件与远端图片。
- **从一张图到整篇文章。** 批量上传、压缩、缩放、移除 JPEG EXIF；用 `img rewrite` 转存 Markdown 图片并替换引用。
- **写作工具与 Agent 都能接入。** Typora 自定义命令、PicGo 兼容本地服务、JSON 输出和配套 Agent Skill。

> img 是上传客户端，需要你自己的存储服务及可公开访问的图片地址。图库管理本地记录；远端文件管理与 PicGo 插件生态不在当前范围内。

## 下载即用

**[打开安装页 →](https://liyown.github.io/img/install/#gui)** 始终获取最新稳定版，普通用户无需安装 Rust、Go 或编译工具。

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

## 试着上传第一张图

### 用桌面版

1. 安装并打开 img，在「设置 → 存储源」添加图床，设为默认。
2. 复制一张图片，按下快捷键直接上传。
3. 回到文章里粘贴链接。Markdown、URL 等格式可在设置中选择。

| 快捷操作 | macOS | Windows / Linux X11 |
| --- | --- | --- |
| 上传剪贴板并复制链接 | `⌘⌥U` | `Ctrl+Alt+U` |
| 截图上传并复制链接 | `⌘⌥S` | `Ctrl+Alt+S` |

也可从窗口选择文件、粘贴图片或添加外链，确认队列后点击上传。

### 用 CLI

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

## 放进你的写作流程

| 你在做什么 | img 怎么帮忙 |
| --- | --- |
| 写博客、记笔记 | 截图上传后自动复制链接，直接粘贴到 Markdown |
| 在 Typora 写文章 | 自定义上传命令：`img "${filepath}"` |
| 在 Obsidian 管理笔记 | 运行 `img serve`，通过 Image Auto Upload 插件连接 PicGo 兼容服务 |
| 迁移文章或更换图床 | `img rewrite` 处理本地图片与外链，保留 alt 和 title |
| 让 Agent 发布图片 | CLI 返回结构化 JSON，配套 Skill 复用已配置存储源 |

```sh
npx skills add liyown/img --skill img-uploader
```

[集成示例](docs/cli.md#集成) · [Agent Skill](skills/img-uploader) · [GitHub Action](action.yml)

## 支持你已有的存储

**Cloudflare R2 · S3 兼容服务 · 阿里云 OSS · GitHub 仓库 · 自定义 HTTP 接口**

支持 PNG、JPEG、GIF、WebP、SVG、AVIF。可以为不同项目选择不同存储源，配置默认输出格式与图片处理选项。[存储配置指南](https://liyown.github.io/img/docs/storage/)

## 参与改进

遇到问题，欢迎提交系统版本、img 版本和复现步骤；请勿附上存储密钥。[报告问题或提出需求](https://github.com/liyown/img/issues)

```sh
git clone https://github.com/liyown/img.git
cd img
cargo test --locked --workspace
```

代码分为 [共享上传核心](crates/img-core)、[独立 CLI](crates/img-cli) 和 [原生桌面应用](desktop)。[本地构建与发行流程](desktop/RELEASING.md)

如果 img 帮你省下了反复上传、复制链接的时间，欢迎点个 Star，或把它分享给一起写作的人。
