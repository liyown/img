# img

[![CI](https://github.com/liyown/img/actions/workflows/ci.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/ci.yml)

`img` 是一个为 AI Agent 和命令行用户设计的图片上传工具。它把本地图片上传到已配置的图床，并返回 URL、Markdown 或 JSON。

支持：

- Cloudflare R2 和通用 S3 存储
- 阿里云 OSS
- GitHub 仓库
- 通用 HTTP 上传接口
- PNG、JPEG、GIF、WebP、SVG、AVIF

```console
$ img screenshot.png --format markdown
![](https://img.example.com/2026/07/screenshot.png)
```

## 安装

macOS / Linux：

```sh
curl -fsSL https://raw.githubusercontent.com/liyown/img/main/install.sh | sh
```

Windows PowerShell：

```powershell
irm https://raw.githubusercontent.com/liyown/img/main/install.ps1 | iex
```

安装器会自动完成下载、校验和命令配置。

确认安装：

```sh
img version
```

## 初始化

首次使用需要创建一个 Provider，并将它设为默认图床。

### 交互式初始化

```sh
img init
```

根据提示输入 Provider 类型、名称、Bucket 或上传地址。初始化完成后验证配置：

```sh
img config validate
img provider list
```

### Cloudflare R2

先设置凭据：

```sh
export IMG_R2_ACCESS_KEY='your-access-key'
export IMG_R2_SECRET_KEY='your-secret-key'
```

非交互初始化：

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

将 endpoint、region、Bucket 和公开域名替换为自己的配置。

### HTTP 图床

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

在该文件中添加：

```toml
version = 1
default_provider = "github"

[providers.github]
type = "github"
owner = "your-name"
repo = "images"
branch = "main"
token = "${IMG_GITHUB_TOKEN}"
commit_message = "upload image: {path}"
```

然后设置 Token 并验证：

```sh
export IMG_GITHUB_TOKEN='your-token'
img config validate
```

## 上传图片

上传单张图片：

```sh
img upload screenshot.png
```

可以省略 `upload`：

```sh
img screenshot.png
```

上传多张图片：

```sh
img upload a.png b.jpg c.webp
```

输出 Markdown：

```sh
img screenshot.png --format markdown
```

输出 JSON：

```sh
img upload a.png b.jpg --format json
```

指定 Provider 和远端目录：

```sh
img screenshot.png --provider github --path posts/assets
```

指定远端文件名：

```sh
img screenshot.png --name cover.png
```

覆盖已存在的文件：

```sh
img screenshot.png --overwrite
```

复制结果：

```sh
img screenshot.png --format markdown --copy
```

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

Skill 会使用 JSON 模式调用 CLI、保留多文件部分成功结果，并且默认不会操作剪贴板或覆盖远端文件。

## Provider 管理

```sh
img provider list
img provider show r2
img provider use github
img provider test r2
img provider remove old
```

切换默认 Provider：

```sh
img provider use aliyun
```

临时使用另一个 Provider：

```sh
img screenshot.png --provider github
```

## 常用配置

查看当前配置：

```sh
img config list
```

设置默认输出格式：

```sh
img config set output.format markdown
```

启用自动复制：

```sh
img config set output.copy true
```

检查配置：

```sh
img config validate
```

配置优先级从低到高：全局配置、当前目录 `.img.toml`、环境变量、命令行参数。

## JSON 输出

Agent 和脚本建议使用：

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

多文件上传部分失败时，成功结果仍会保留，进程退出码为 `3`。

## 安全建议

- 使用 `${ENV_NAME}` 引用凭据，不要把 Token 或 Secret 写入仓库
- 不要把真实密钥发送给 Agent、Issue 或聊天窗口
- 将项目级 `.img.toml` 加入自己的 `.gitignore`
- `config list` 和 `provider show` 会隐藏敏感字段
- 覆盖操作必须显式传入 `--overwrite`

完整配置示例见 [config.example.toml](config.example.toml)。
