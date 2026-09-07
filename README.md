<div align="center">
  <img src="site/public/favicon.svg" width="72" alt="img">
  <h1>img</h1>
  <p><strong>截图上传，链接直接粘贴。</strong></p>
  <p>自己用桌面快捷键，让 AI 用原生 CLI 和 Skill。<br>共用一套存储配置，图片上传到你自己的图床。支持 macOS、Windows 和 Linux。</p>
  <p><a href="https://liyown.github.io/img/install/#gui"><strong>下载桌面版</strong></a> · <a href="https://liyown.github.io/img/install/#cli">下载 CLI</a> · <a href="https://liyown.github.io/img/">官网</a> · <a href="https://liyown.github.io/img/docs/">文档</a> · <a href="README.en.md">English</a></p>

[![Desktop release](https://github.com/liyown/img/actions/workflows/desktop-release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/desktop-release.yml)
[![CLI release](https://github.com/liyown/img/actions/workflows/release.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/release.yml)
[![Rust](https://img.shields.io/badge/built_with-Rust-dea584)](Cargo.toml)

</div>

[![img 原生图库：查找图片、预览并复制链接；macOS 演示界面](site/public/screenshots/gallery.webp)](https://liyown.github.io/img/install/#gui)

## 为什么换到 img

你截好一张图，按下快捷键，上传后的链接就会复制到剪贴板。回到文章里粘贴即可。让 AI 写文章时，它也能调用 img，把本地截图或图表上传后放进 Markdown。手动操作和 Agent 上传共用同一套存储配置。

桌面端用 Rust / GPUI 构建，包内自带同版本原生 CLI。只在终端或自动化里用，可以单独下载命令行程序，不用启动桌面应用，也不用安装 Node.js。上传、图片处理和存储配置共用实现，换个入口不用再搭一套工具。

已有 R2、S3、OSS、GitHub 或 HTTP 图床，可以继续使用原来的存储和公开域名。旧文章里的链接不受影响。你可以先用 CLI 传几张图，再决定是否把日常上传也交给 img。

桌面端上传的图片保存在本地图库记录中，可以搜索、预览、切换网格或列表，跨搜索多选后批量复制链接。整篇文章要转存时，用 `img rewrite` 上传图片并替换引用；批量上传还支持压缩、缩放和移除 JPEG EXIF。

## 给 AI 一个能直接调用的上传工具

Agent 生成了图表，或写文章时引用了本地截图，可以通过原生 CLI 上传，再把返回的链接写进正文。仓库提供可安装的 [img-uploader Skill](skills/img-uploader)，适用于支持 Agent Skills 且能执行本地命令的助手。

```sh
npx skills add liyown/img --skill img-uploader
```

安装 CLI、配置好默认存储源后，就可以给 Agent 这样的任务：

> 把 ./assets/chart.png 上传到默认图床，将返回的 Markdown 图片链接写进 article.md。

Skill 会先检查文件和配置，再使用 `--format json --no-copy` 上传。CLI 返回逐文件结果与退出码，部分失败时保留成功链接。Agent 复用你配好的存储源，无需在对话中提供密钥，也不必操作上传窗口。

[查看 Skill 的上传流程](skills/img-uploader/SKILL.md) · [CLI JSON 输出与退出码](docs/cli.md#json-输出)

## 下载即用

[下载最新稳定版](https://liyown.github.io/img/install/#gui)。普通用户无需安装 Rust、Go 或编译工具。

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

## 继续用你熟悉的编辑器

| 你在做什么 | img 怎么帮忙 |
| --- | --- |
| 写博客、记笔记 | 截图上传后自动复制链接，直接粘贴到 Markdown |
| 在 Typora 写文章 | 自定义上传命令：`img "${filepath}"` |
| 在 Obsidian 管理笔记 | 运行 `img serve`，通过 Image Auto Upload 插件连接 PicGo 兼容服务 |
| 迁移文章或更换图床 | `img rewrite` 处理本地图片与外链，保留 alt 和 title |
| 让 Agent 发布图片 | CLI 返回结构化 JSON，配套 Skill 复用已配置存储源 |

[集成示例](docs/cli.md#集成) · [Agent Skill](skills/img-uploader) · [GitHub Action](action.yml)

### 从其他上传工具过来

先在 img 里添加原来的存储源，上传一张图并检查返回的 URL，再调整编辑器的上传入口。现有配置需要重新添加，img 不会自动导入其他工具的配置或历史图库，也不会修改已有文章链接。

需要远端文件管理或依赖 PicGo 插件时，可以继续保留原来的工具。img 的图库管理本地记录；清理记录会保留原文件和远端图片。你也可以先只把 Agent 和脚本里的上传交给 img。

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

欢迎在 Issue 里告诉我们你用 img 接上了哪个编辑器或 Agent，以及哪一步还不顺手。
