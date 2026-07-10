# img

[![CI](https://github.com/liyown/img/actions/workflows/ci.yml/badge.svg)](https://github.com/liyown/img/actions/workflows/ci.yml)
[![Go Report Card](https://goreportcard.com/badge/github.com/liyown/img)](https://goreportcard.com/report/github.com/liyown/img)
[![Go Reference](https://pkg.go.dev/badge/github.com/liyown/img.svg)](https://pkg.go.dev/github.com/liyown/img)

一个轻量、跨平台、可扩展的图片上传 CLI。一条命令把本地图片上传到 Cloudflare R2、S3 兼容存储、阿里云 OSS、GitHub 仓库或自建 HTTP 图床，并输出 URL、Markdown、HTML 或 JSON。

```console
$ img screenshot.png --format markdown
![](https://img.example.com/2026/07/screenshot.png)
```

## 特性

- 简短命令：支持 `img upload image.png` 和 `img image.png`
- 多图上传：保守的有界并发，结果顺序与输入一致
- 多种输出：URL、Markdown、HTML 和稳定 JSON
- 安全配置：环境变量凭据、敏感字段脱敏、Provider 凭据隔离
- 路径模板：日期、时间戳、Hash 和 UUID，统一使用 `/`
- 上传保护：图片类型、文件大小、路径穿越和同名覆盖检查
- 单文件程序：不依赖 Node.js、Python 或外部运行时
- 跨平台：macOS、Linux 和 Windows

## Provider

| Provider | 状态 | 说明 |
|---|---|---|
| Cloudflare R2 | ✅ | 使用 S3 Provider，支持自定义域名 |
| S3 Compatible | ✅ | AWS S3、MinIO、Backblaze B2 等 |
| 阿里云 OSS | ✅ | S3 兼容模式，已处理 `aws-chunked` 差异 |
| GitHub | ✅ | GitHub Contents API，支持创建和 SHA 覆盖 |
| HTTP API | ✅ | multipart、自定义 Header/字段、点号 JSON 路径 |

## 安装

使用 Go 1.26 或更高版本：

```sh
go install github.com/liyown/img/cmd/img@latest
```

从源码构建：

```sh
git clone https://github.com/liyown/img.git
cd img
make build
./bin/img version
```

`make cross` 可生成 macOS AMD64/ARM64、Linux AMD64/ARM64 和 Windows AMD64 产物。

## 快速开始

### 1. 初始化

HTTP 或 S3 Provider 可交互初始化：

```sh
img init
```

也可用于脚本或 CI：

```sh
img init --type http --name custom \
  --url https://example.com/upload \
  --url-json-path data.url
```

### 2. 上传

```sh
img upload screenshot.png
img screenshot.png --format markdown
img upload a.png b.jpg c.webp --format json
```

常用选项：

```text
--provider NAME    临时选择 Provider
--format FORMAT    url、markdown、html 或 json
--path PATH        远端路径前缀
--name NAME        单文件远端名称
--overwrite        允许覆盖已存在对象
--copy             复制当前格式的结果
--no-copy          禁止复制
--verbose          显示 Provider 基本信息
```

## 配置

全局配置目录由 Go 的 `os.UserConfigDir()` 决定：

| 系统 | 默认路径 |
|---|---|
| macOS | `~/Library/Application Support/img/config.toml` |
| Linux | `~/.config/img/config.toml` |
| Windows | `%AppData%\img\config.toml` |

运行 `img config path` 可查看当前系统的准确路径。项目目录还可使用不含密钥的 `.img.toml`。

配置优先级从低到高：

1. 内置默认值
2. 全局配置
3. 当前项目 `.img.toml`
4. 环境变量
5. 命令行参数

完整示例见 [`config.example.toml`](config.example.toml)。

### Cloudflare R2 / S3

```toml
version = 1
default_provider = "r2"

[providers.r2]
type = "s3"
endpoint = "https://ACCOUNT_ID.r2.cloudflarestorage.com"
region = "auto"
bucket = "images"
access_key = "${IMG_R2_ACCESS_KEY}"
secret_key = "${IMG_R2_SECRET_KEY}"
public_url = "https://img.example.com"
path_style = true
```

`public_url` 必填，因为 API endpoint 无法证明 Bucket 已公开，也无法推断期望的访问域名。

### 阿里云 OSS

```toml
[providers.aliyun]
type = "s3"
endpoint = "https://oss-cn-shenzhen.aliyuncs.com"
region = "oss-cn-shenzhen"
bucket = "your-bucket"
access_key = "${IMG_ALIYUN_ACCESS_KEY_ID}"
secret_key = "${IMG_ALIYUN_ACCESS_KEY_SECRET}"
public_url = "https://img.example.com"
path_style = false
```

客户端只在服务端要求时计算 AWS 请求校验和，避免阿里云 OSS 不支持的 `aws-chunked` 上传编码。

### GitHub

```toml
[providers.github]
type = "github"
owner = "example"
repo = "images"
branch = "main"
token = "${IMG_GITHUB_TOKEN}"
commit_message = "upload image: {path}"
```

未设置 `public_url` 时生成 `raw.githubusercontent.com` 地址；也可配置 jsDelivr 等 CDN。默认拒绝覆盖，传入 `--overwrite` 后会先获取文件 SHA。

### 通用 HTTP API

```toml
[providers.custom]
type = "http"
url = "https://example.com/upload"
method = "POST"
file_field = "file"
url_json_path = "data.url"

[providers.custom.fields]
folder = "images"

[providers.custom.headers]
Authorization = "Bearer ${IMG_HTTP_TOKEN}"
```

响应最大读取 1 MiB。`url_json_path` 第一版支持 `data.url` 形式的点号路径；非 2xx 响应正文会截断，敏感 Header 值会从错误信息中清除。

## 路径与重命名

默认模板是 `{year}/{month}/{filename}`。支持：

```text
{year} {month} {day} {timestamp} {unix}
{filename} {stem} {ext} {hash} {uuid}
```

重命名策略支持 `original`、`timestamp`、`hash` 和 `uuid`。绝对路径、`../`、空路径段和控制字符会在上传前被拒绝。

## 命令

```sh
img upload a.png b.jpg --format json
img image.webp --path blog/assets --copy
img upload image.png --name architecture.png --overwrite

img provider list
img provider show r2
img provider use github
img provider remove old
img provider test r2

img config path
img config list
img config validate
img config get output.format
img config set output.format markdown
img config unset output.copy

img version
```

## JSON 与退出码

多文件上传时，单个失败不会丢失已经成功的结果：

```json
{
  "success": false,
  "files": [
    {
      "local_path": "a.png",
      "success": true,
      "remote_path": "2026/07/a.png",
      "url": "https://img.example.com/2026/07/a.png",
      "provider": "r2",
      "size": 1024,
      "content_type": "image/png"
    },
    {
      "local_path": "missing.png",
      "success": false,
      "error": "file not found"
    }
  ]
}
```

| 退出码 | 含义 |
|---:|---|
| 0 | 全部成功 |
| 1 | 上传全部失败或通用执行失败 |
| 2 | 参数或配置错误 |
| 3 | 部分文件上传失败 |

## 剪贴板

macOS 使用系统 `pbcopy`，Windows 使用 `clip`。Linux 当前调用 `xclip`，使用 `--copy` 前需要安装它。剪贴板失败只显示警告，不会改变上传成功状态。

## 安全

- 不要在配置中提交真实 Token、AccessKey 或 Secret
- 优先使用 `${ENV_NAME}` 引用环境变量
- 不同 Provider 的凭据仅在该 Provider 被选中时解析
- `config list` 和 `provider show` 会遮蔽 AccessKey、Token、Secret、Password 和 Authorization
- `.img.toml`、构建产物和本地日志已被 `.gitignore` 排除
- 已经粘贴到终端历史、Issue 或聊天中的凭据应立即轮换

## 开发

```sh
make fmt     # 格式化
make test    # race 单元测试
make lint    # go vet
make build   # 当前平台
make cross   # 发布目标平台
```

Provider 测试使用 `httptest.Server`，不需要真实云端凭据。发布支持 GoReleaser，但本地开发不依赖它。

## Roadmap

- OSS/COS 原生 SDK
- 七牛云、SM.MS 和 WebDAV
- 系统凭据存储（Keychain、Credential Manager、Secret Service）
- 更细粒度的网络错误分类、有限重试和实时 Provider 健康检查
