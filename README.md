# img

[![CI](https://github.com/liyown/img/actions/workflows/ci.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/ci.yml)

`img` 是一个为 AI Agent 和命令行用户设计的图片上传工具。把本地图片或外链 URL 上传到已配置的图床，返回 URL、Markdown 或 JSON。

支持图床：Cloudflare R2、通用 S3、阿里云 OSS、GitHub 仓库、自定义 HTTP 接口

支持格式：PNG、JPEG、GIF、WebP、SVG、AVIF

```console
$ img screenshot.png --format markdown
![screenshot.png](https://img.example.com/2026/07/screenshot.png)
```

## 安装

macOS / Linux：

```sh
curl -fsSL https://raw.githubusercontent.com/liyown/img/v0.1.1/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/liyown/img/v0.1.1/install.ps1 | iex
```

确认安装：

```sh
img version
```

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
  --type s3 \
  --name r2 \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --region auto \
  --bucket images \
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
  --type s3 \
  --name aliyun \
  --endpoint https://oss-cn-shenzhen.aliyuncs.com \
  --region oss-cn-shenzhen \
  --bucket your-bucket \
  --access-key '${IMG_ALIYUN_ACCESS_KEY_ID}' \
  --secret-key '${IMG_ALIYUN_ACCESS_KEY_SECRET}' \
  --public-url https://img.example.com
```

### 自定义 HTTP 图床

```sh
img init \
  --type http \
  --name custom \
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

查看配置文件路径：

```sh
img config path
```

用 `img init` 命令配置：

```sh
img init \
  --type github \
  --name github \
  --owner your-name \
  --repo images \
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

设置 Token：

```sh
export IMG_GITHUB_TOKEN='your-token'
img config validate
```

## 上传图片

```sh
img screenshot.png                        # 上传单张，输出 URL
img screenshot.png --format markdown      # 输出 Markdown
img upload a.png b.jpg c.webp            # 多张同时上传
img upload a.png b.jpg --format json     # JSON 输出
img screenshot.png --provider github     # 指定图床
img screenshot.png --path posts/assets   # 指定远端目录
img screenshot.png --name cover.png      # 指定远端文件名
img screenshot.png --overwrite           # 覆盖已存在的文件
img screenshot.png --format markdown --copy   # 上传并复制结果到剪贴板
img screenshot.png --quiet --copy        # 仅复制，不打印到终端
```

转存外链图片到自己的图床：

```sh
img https://example.com/photo.jpg
img https://example.com/photo.jpg --format markdown
img http://192.168.1.10/photo.png --allow-insecure   # 允许 HTTP 源
```

上传前自动压缩（只在压缩后更小时才使用压缩版本）：

```sh
img screenshot.png --optimize
img screenshot.png --optimize --verbose   # 显示每张图的压缩率
```

`--optimize` 的压缩策略：

| 格式 | 处理方式 |
|------|---------|
| JPEG | 以质量 85 重新编码 |
| PNG（不透明） | JPEG 与无损 WebP 中取更小的 |
| PNG（含透明） | 尝试无损 WebP（保留透明度） |
| SVG / GIF / WebP / AVIF | 原样上传 |

## 截图即上传

截图后直接上传，结果自动复制到剪贴板：

```sh
img screenshot                  # 全屏截图
img screenshot --region         # 框选区域（交互式）
img screenshot --window         # 当前活动窗口
img screenshot --format markdown # 输出 Markdown 链接
img screenshot --optimize       # 截图后压缩再上传
img screenshot --no-copy        # 不自动复制
```

- macOS：使用系统内置 `screencapture`，无需安装额外工具
- Linux：按顺序尝试 `flameshot`、`scrot`、`gnome-screenshot`、`import`（ImageMagick）
- Windows：使用 PowerShell 截全屏（不支持选区）

## 编辑器图片上传代理

启动一个兼容 PicGo 协议的本地 HTTP 服务，让 Typora、Obsidian 等编辑器通过 img 上传图片：

```sh
img serve                       # 监听 127.0.0.1:36677（PicGo 默认端口）
img serve --port 9000           # 自定义端口
img serve --optimize            # 上传前自动压缩
img serve --provider r2         # 指定图床
```

配置编辑器：

**Typora**：偏好设置 → 图像 → 插入图片时 → 上传图片 → Custom Command

```
img "${filepath}"
```

**Obsidian**（Image Auto Upload 插件）：

```
上传服务 URL：http://127.0.0.1:36677/upload
```

每次有图片上传时，终端会实时显示上传结果。按 Ctrl+C 停止服务。

## 文章图片转存

将 Markdown 文章里的所有图片一键上传到图床并替换引用：

```sh
img rewrite article.md                        # 原地改写文件
img rewrite article.md --stdout               # 结果输出到 stdout，不改原文件
cat article.md | img rewrite                  # 从 stdin 读，结果到 stdout
img rewrite article.md --optimize             # 上传前压缩图片
img rewrite article.md --provider github      # 指定图床
```

- 本地路径和外链 URL 都会被上传并替换
- 支持 `![alt](path "title")` 和 `<img src="path">` 两种格式
- alt text、title 等属性保持不变，只替换 URL 部分
- 无法处理的引用（如 `data:` URI）原样保留

## 给 Agent 使用

仓库包含配套 Skill：[skills/img-uploader](skills/img-uploader)。使用开放 Agent Skills CLI 安装：

```sh
npx skills add liyown/img --skill img-uploader
```

为 Codex 全局非交互安装：

```sh
npx skills add liyown/img --skill img-uploader --agent codex --global --yes
```

重新启动或刷新 Agent 后，可以直接提出：

```text
使用 $img-uploader 上传这张图片并返回 Markdown。
使用 $img-uploader 把这些截图上传到 posts/assets。
使用 $img-uploader 通过 github Provider 上传图片。
```

## Provider 管理

```sh
img provider list              # 查看所有 Provider
img provider show r2           # 查看某个 Provider 的配置
img provider use github        # 切换默认 Provider
img provider test r2           # 测试连通性
img provider remove old        # 删除 Provider
```

## 常用配置

```sh
img config list                            # 查看当前配置
img config path                            # 查看配置文件路径
img config validate                        # 检查配置是否有效
img config set output.format markdown      # 默认输出 Markdown
img config set output.copy true            # 自动复制结果
img config set output.quiet true           # 静默模式
```

配置优先级（从低到高）：全局配置 → 项目 `.img.toml` → 环境变量 → 命令行参数。

项目级 `.img.toml` 只能指定已在全局配置中定义的 Provider，以及输出格式和路径设置，不能在其中写入凭据。

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

## 安全建议

- 凭据使用 `${ENV_NAME}` 引用，不要写入配置文件或仓库
- 项目级 `.img.toml` 加入 `.gitignore`
- `config list` 和 `provider show` 会隐藏敏感字段
- 覆盖操作需显式传入 `--overwrite`

完整配置示例见 [config.example.toml](config.example.toml)。
