# img

[![CI](https://github.com/liyown/img/actions/workflows/ci.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/ci.yml)

`img` 是一个为 AI Agent 和命令行用户设计的图片上传工具。把本地图片、截图或外链 URL 上传到已配置的图床，返回 URL、Markdown 或 JSON。

支持图床：Cloudflare R2、通用 S3、阿里云 OSS、GitHub 仓库、自定义 HTTP 接口

支持格式：PNG、JPEG、GIF、WebP、SVG、AVIF

```console
$ img screenshot --region --format markdown
![screenshot.png](https://img.example.com/2026/07/screenshot.png)
```

---

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
