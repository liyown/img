# img

[官网 · 功能、对比与文档](https://liyown.github.io/img/)

[![CI](https://github.com/liyown/img/actions/workflows/ci.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/ci.yml)
&nbsp;[English](README.en.md)

`img` 是使用 Rust 编写的图片上传工具，提供独立 CLI 和原生 macOS / Windows / Linux GUI；安装 GUI 时已包含完整 CLI。把本地图片、截图或外链 URL 上传到已配置的图床，返回 URL、Markdown 或 JSON。

支持图床：Cloudflare R2、通用 S3、阿里云 OSS、GitHub 仓库、自定义 HTTP 接口

支持格式：PNG、JPEG、GIF、WebP、SVG、AVIF

```console
$ img screenshot --region --format markdown
![screenshot.png](https://img.example.com/2026/07/screenshot.png)
```

---

## 安装

选择适合自己的版本：

| 版本 | 平台 | 包含内容 |
| --- | --- | --- |
| CLI | macOS、Linux、Windows | 独立 `img` 命令，无需图形界面或语言运行时 |
| GUI | macOS 13+（Apple silicon / Intel）、Windows x64、Ubuntu 24.04 / Debian 13+ x64 | 原生图形界面 + 同版本完整 CLI |

桌面版请使用[官网下载入口](https://liyown.github.io/img/install/#gui)，它自动指向最新稳定桌面发行版；也可查看 [GitHub Releases](https://github.com/liyown/img/releases)。下载 DMG 后将 Img.app 拖到 Applications，无需 Rust 或 Go。

社区安装包未经过 Apple 公证。若首次打开被阻止，请在「系统设置 → 隐私与安全性」中找到 img，确认来源后选择「仍要打开」，无需关闭系统保护。完整步骤见 [安装说明](desktop/INSTALL.html)。

GUI 内置同版本 CLI；「设置 → 关于与更新」可添加终端命令、检查更新，并下载校验后退出安装新版。只需要独立 CLI 时可使用 CLI 发行包或源码安装。

```sh
sh install.sh --gui   # 自动获取最新桌面版，安装 GUI 与内置 CLI
sh install.sh --cli   # 独立 CLI 的 latest 发行版
```

开发者本地构建可运行 `make desktop-package` 或 `make cli-package`，源码安装 CLI 使用 `make install`。公开安装产物由 GitHub Actions 构建、验收并发布。

CLI 默认安装到 `~/.local/bin/img`；GUI 默认安装到 `~/Applications/Img.app`，终端命令链接到包内 `Contents/MacOS/img`，更新应用后沿用新版。目录可通过 `IMG_INSTALL_DIR`、`IMG_APP_DIR` 指定。若该命令目录不在 PATH，安装器会提示添加。

Windows 使用 `install.ps1 -Product cli` 或 `install.ps1 -Product gui`。Linux GUI 使用 DEB 或 `sh install.sh --gui`，由系统包管理器安装。直接拖动 DMG 安装 GUI 时 CLI 也已在应用内，可在「设置 → 关于与更新 → 添加终端命令」启用终端入口。

验证安装：

```sh
img version
# img 0.3.0
# implementation: Rust
```

详细构建、离线安装及发布方式见 [发行说明](desktop/RELEASING.md)。

---

## 初始化

首次使用前需要创建一个 Provider 并将它设为默认图床。

### 交互式初始化

```sh
img init
```

根据提示输入类型、名称和地址，初始化完成后验证：

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

### 阿里云 OSS

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

### 自定义 HTTP 图床

```sh
img init --type http --name custom \
  --url https://example.com/api/upload \
  --url-json-path data.url
```

需要固定 Header 或表单字段时，在配置文件中添加：

```toml
[providers.custom.headers]
Authorization = "Bearer ${IMG_HTTP_TOKEN}"

[providers.custom.fields]
folder = "images"
```

### GitHub 图床

```sh
img init --type github --name github \
  --owner your-name --repo images \
  --token '${IMG_GITHUB_TOKEN}'
```

或手动在配置文件中添加：

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

## 命令

### img upload — 上传图片

```sh
img screenshot.png                          # 上传，输出 URL
img screenshot.png --format markdown        # 输出 Markdown 链接
img upload a.png b.jpg c.webp              # 多张同时上传
img https://example.com/photo.jpg          # 转存外链图片
img http://192.168.1.10/img.png --allow-insecure  # 允许 HTTP 源
```

常用选项：

```sh
--format url|markdown|html|json    # 输出格式
--provider <name>                  # 指定图床
--path posts/assets                # 远端路径前缀
--name cover.png                   # 指定远端文件名
--overwrite                        # 覆盖已存在的文件
--copy / --no-copy                 # 复制结果到剪贴板
--quiet                            # 不输出到终端（配合 --copy 使用）
--verbose                          # 显示详细信息
```

处理选项（见[处理选项](#处理选项)章节）：

```sh
--optimize       # 压缩后上传
--strip-exif     # 剥离 EXIF 元数据
--resize 1200    # 缩放到最大宽度 1200px
```

### img screenshot — 截图即上传

截图后直接上传，结果默认复制到剪贴板：

```sh
img screenshot                   # 全屏
img screenshot --region          # 框选区域（交互式）
img screenshot --window          # 当前活动窗口
img screenshot --format markdown
img screenshot --optimize
img screenshot --no-copy         # 不复制到剪贴板
```

- macOS：使用系统内置 `screencapture`
- Linux：按顺序尝试 `flameshot`、`scrot`、`gnome-screenshot`、`import`
- Windows：PowerShell 全屏截图

### img serve — 编辑器图片上传代理

启动一个兼容 PicGo 协议的本地 HTTP 服务，让编辑器通过 img 上传图片：

```sh
img serve                        # 127.0.0.1:36677（PicGo 默认端口）
img serve --port 9000
img serve --optimize --strip-exif --resize 1200
```

**Typora** — 偏好设置 → 图像 → 上传图片 → Custom Command：

```
img "${filepath}"
```

**Obsidian**（Image Auto Upload 插件）：

```
上传服务 URL：http://127.0.0.1:36677/upload
```

### img rewrite — 文章图片批量转存

将 Markdown 文章里的所有图片上传到图床并替换引用：

```sh
img rewrite article.md                     # 原地改写
img rewrite *.md                           # 批量改写多篇文章
img rewrite article.md --stdout            # 结果输出到 stdout
cat article.md | img rewrite               # stdin → stdout
img rewrite article.md --optimize --strip-exif
```

- 本地路径和外链 URL 都会被上传并替换
- 支持 `![alt](path "title")` 和 `<img src="path">` 格式
- alt text、title 等属性保持不变，只替换 URL 部分
- 无法处理的引用（`data:` URI 等）原样保留

### img info — 查看图片信息

检查图片的类型、尺寸、大小和 EXIF 元数据：

```sh
img info photo.jpg screenshot.png          # 表格输出
img info *.jpg --format json               # JSON 输出
```

输出示例：

```
photo.jpg                               JPEG    3000×2000    2.4 MB  ⚠ EXIF
screenshot.png                          PNG     1440×900     156 KB
icon.svg                                SVG     –            4 KB

⚠  Files marked with EXIF may contain GPS location and device information.
   Remove before uploading: img upload <file> --strip-exif
```

---

## 处理选项

以下选项适用于 `upload`、`screenshot`、`rewrite`、`serve` 所有命令：

### --optimize

上传前压缩，只在压缩后更小时才使用压缩版本：

| 格式 | 处理方式 |
|------|---------|
| JPEG | 以质量 85 重新编码 |
| PNG（不透明） | JPEG 与无损 WebP 中取更小的 |
| PNG（含透明） | 尝试无损 WebP（保留透明度） |
| SVG / GIF / WebP / AVIF | 原样上传 |

```sh
img photo.jpg --optimize --verbose   # 显示每张图的压缩率
```

### --strip-exif

上传前剥离 JPEG 文件的 EXIF 元数据（GPS 位置、设备型号等），无损操作：

```sh
img photo.jpg --strip-exif
img photo.jpg --strip-exif --optimize   # 组合使用
```

### --resize \<宽度\>

上传前将图片缩放到指定最大宽度（px），只缩小不放大：

```sh
img photo.jpg --resize 1200
img photo.jpg --resize 1200 --optimize   # 缩放 + 压缩
```

### 配置默认值

不想每次传标志时，可以写入全局配置：

```sh
img config set upload.strip_exif true
img config set upload.max_width 1200
img config set upload.retry_count 3     # 上传失败自动重试次数
```

配置后，所有命令（包括 `img serve`）都会自动应用。

---

## 配置

### 常用配置命令

```sh
img config list                            # 查看当前配置
img config path                            # 查看配置文件路径
img config validate                        # 检查配置是否有效
img config get upload.strip_exif           # 查看某个配置项
img config set output.format markdown      # 默认输出 Markdown
img config set output.copy true            # 自动复制结果
img config set output.quiet true           # 静默模式
img config set upload.retry_count 3        # 失败自动重试 3 次
img config unset upload.max_width          # 恢复默认值
```

### 配置优先级

全局配置 → 项目 `.img.toml` → 环境变量 → 命令行参数（从低到高）

项目级 `.img.toml` 只能指定已在全局配置中定义的 Provider，以及输出格式和路径设置，不能在其中写入凭据。

---

## Provider 管理

```sh
img provider list              # 查看所有 Provider
img provider show r2           # 查看配置（敏感字段已隐藏）
img provider use github        # 切换默认 Provider
img provider test r2           # 测试连通性
img provider remove old        # 删除 Provider
```

---

## JSON 输出

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

多文件部分失败时，成功的结果仍会保留，退出码为 `3`。

---

## 集成

### Shell 补全

```sh
img completion bash      # eval "$(img completion bash)"
img completion zsh       # eval "$(img completion zsh)" 或写入 _img 函数文件
img completion fish      # img completion fish > ~/.config/fish/completions/img.fish
```

### VS Code 扩展

`integrations/vscode/` — 在 Markdown 编辑器里 `Cmd+Alt+V` 粘贴剪贴板图片自动上传，右键资源管理器里的图片可以上传。

### Raycast 扩展

`integrations/raycast/` — macOS Raycast 里三个命令：截图上传、剪贴板上传、文件上传。

### GitHub Action

在文档仓库里自动把本地图片路径转 CDN URL：

```yaml
- uses: liyown/img@v0.2
  with:
    provider-type: s3
    s3-endpoint: https://ACCOUNT.r2.cloudflarestorage.com
    s3-bucket: images
    s3-access-key: ${{ secrets.R2_ACCESS_KEY }}
    s3-secret-key: ${{ secrets.R2_SECRET_KEY }}
    s3-public-url: https://img.example.com
    optimize: 'true'
```

详细配置见 [action.yml](action.yml)。

---

## 给 Agent 使用

仓库包含配套 Skill：[skills/img-uploader](skills/img-uploader)。

```sh
npx skills add liyown/img --skill img-uploader
```

为 Codex 全局非交互安装：

```sh
npx skills add liyown/img --skill img-uploader --agent codex --global --yes
```

---

## 安全建议

- 凭据使用 `${ENV_NAME}` 引用，不要写入配置文件或仓库
- 项目级 `.img.toml` 加入 `.gitignore`
- `config list` 和 `provider show` 会隐藏敏感字段
- 覆盖操作需显式传入 `--overwrite`

完整配置示例见 [config.example.toml](config.example.toml)。

## Rust 工作区

- `crates/img-core`：配置、凭据解析、HTTP / S3 / GitHub、图片处理、路径与上传队列。
- `crates/img-cli`：独立 `img` 可执行文件、截图、Markdown 替换和 PicGo 代理。
- `desktop`：Rust / GPUI 原生 GUI，使用包内同版本 `img` 完成上传。

CLI 命令、JSON 文件结果、退出码与 v1 TOML 配置保持兼容；已有 GUI 队列和偏好目录不变。S3 使用 AWS Rust 凭据链与签名库，保留环境变量、配置引用及 macOS 钥匙串支持。迁移验证见 [验收记录](design-qa.md)。

## 0.3.0 桌面日常使用

这是未发布的本地内测版。主窗口的文件、粘贴、截图和链接操作先把图片加入队列，再点击「上传」。关闭窗口或按 `⌘W` 会隐藏到后台并继续上传，`⌘Q` 才退出；退出会等待队列落盘并回收上传进程。

菜单栏的「上传剪贴板」「截图上传」会直接上传，成功链接自动复制为所选格式。默认全局快捷键是 `⌘⌥U` 和 `⌘⌥S`，可在「设置 → 后台与全局快捷键」修改或关闭。每次触发会固定默认存储源、图片处理和链接格式，按批次排队，不包含主窗口里尚未提交的图片。未配置默认源时，图片会保留并打开存储设置。

macOS 不提供完整的第三方非独占快捷键查询。img 会报告系统返回的注册错误及 img 实例间的冲突，并保持非独占注册以免抢占其他应用；不能保证识别所有第三方冲突，冲突时请改用另一组组合键或菜单操作。

「设置 → 诊断与恢复」可以导出只含版本、操作阶段与错误分类的诊断记录。导出内容不包含图片、凭据、请求正文、文件名和 URL。队列损坏时，先恢复最近两份有效备份之一，或保留原文件后重建空队列，再开始上传。原图副本和较大预览图会保留，列表另用最长边 512 px 的缩略图和 64 MiB 解码缓存。

完整功能、测试证据和仍待验证的系统交互见 [0.3.0 稳定性验收记录](stability-qa.md)。
