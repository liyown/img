---
title: "存储配置"
description: "直接选择图床，理解配置优先级、凭据与路径规则。"
locale: zh
topic: storage
order: 1
---
## 桌面端直接选择

「设置 → 存储源」支持新增、编辑、测试、设为默认和删除。R2、通用 S3、OSS、GitHub 与 HTTP 都有对应表单。新凭据存入 macOS 钥匙串，配置文件保留引用。编辑时留空的凭据字段会保留原值。

GUI 自动维护配置，日常使用不需要手写 TOML。CLI 可用 `img init` 交互配置，也可通过命令或文件管理。

## R2 与 S3

在当前终端环境设置 `IMG_R2_ACCESS_KEY` 和 `IMG_R2_SECRET_KEY` 后，执行：

```sh
img init --type s3 --name r2 \
  --endpoint https://ACCOUNT_ID.r2.cloudflarestorage.com \
  --region auto --bucket images \
  --access-key '${IMG_R2_ACCESS_KEY}' \
  --secret-key '${IMG_R2_SECRET_KEY}' \
  --public-url https://images.example.com --path-style
img provider use r2
img config validate
```

单引号保留环境变量引用，避免将密钥直接写入配置。请替换账号、桶名和公开域名。通用 S3 使用供应商给出的 endpoint 与 region；OSS 使用对应地域的 S3 兼容接口。`public_url` 应能公开访问上传后的对象，API endpoint 不等于图片公开地址。

## GitHub

设置环境变量 `IMG_GITHUB_TOKEN`，再执行：

```sh
img init --type github --name github \
  --owner YOUR_NAME --repo images --branch main \
  --token '${IMG_GITHUB_TOKEN}'
```

令牌需要目标仓库的内容写入权限。可配置 `public_url` 与 `commit_message`；默认提交信息是 `upload: {path}`。

## 自定义 HTTP

```sh
img init --type http --name custom \
  --url https://example.com/api/upload \
  --method POST --file-field file --url-json-path data.url
```

`url_json_path` 指向响应 JSON 中的图片地址。支持 POST、PUT、PATCH，额外请求头和表单字段在 GUI 中直接填写，或写入：

```toml
[providers.custom.headers]
Authorization = "Bearer ${IMG_HTTP_TOKEN}"

[providers.custom.fields]
folder = "images"
```

只有明确可信的 HTTP 服务才开启 `allow_insecure`；远程图片来源的 `--allow-insecure` 与存储端配置分别控制。

## 配置位置与优先级

执行 `img config path` 查看准确路径。macOS 默认 `~/Library/Application Support/img/config.toml`；Linux 使用 XDG 配置目录下的 `img/config.toml`；Windows 使用用户 Roaming 配置目录。

优先级由低到高：全局配置 → 当前目录 `.img.toml` → 环境变量 → 命令行选项。`--config` 指定另一个全局文件，CLI 仍会读取当前目录的项目配置。GUI 在隔离目录调用内置 CLI，不读取项目配置。

项目 `.img.toml` 只允许版本、已定义的 provider、输出格式和上传路径，不允许凭据或 `output.copy`：

```toml
version = 1
provider = "r2"
[output]
format = "markdown"
[upload]
path = "posts"
path_template = "{year}/{month}/{filename}"
```

覆盖环境变量包括 `IMG_PROVIDER`、`IMG_DEFAULT_PROVIDER`、`IMG_OUTPUT_FORMAT`、`IMG_OUTPUT_COPY`、`IMG_UPLOAD_CONCURRENCY`。普通凭据引用支持 `${VARIABLE}`；macOS 也可解析 GUI 创建的钥匙串引用，环境变量优先。

## 管理与默认参数

```sh
img provider list
img provider show r2
img provider test r2
img provider use r2
img provider remove old
img config list
img config get upload.max_size
img config set upload.retry_count 3
img config unset upload.max_width
```

`provider show` 和 `config list` 隐藏敏感字段。连接测试可能在目标存储写入测试对象，请在自己有权限的存储上使用。

CLI 核心默认并发为 4、单图上限 20 MiB、重试次数 0；GUI 单独保存上传偏好，默认并发 3、单图 8 MiB。路径模板默认 `{year}/{month}/{filename}`，命名默认保留原名，同名默认报错；可按需启用覆盖。完整字段见仓库 [config.example.toml](https://github.com/liyown/img/blob/main/config.example.toml)。
