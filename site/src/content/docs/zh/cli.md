---
title: "CLI 参考"
description: "完整 Rust CLI 命令、参数、JSON、进度与退出码。"
locale: zh
topic: cli
order: 5
---
## 命令与简写

`img <图片或 URL>` 等价于 `img upload <图片或 URL>`。`--config <文件>` 是全局选项。命令帮助以当前 0.3.0 release 二进制输出为准，下方完整列出各子命令参数。

## 处理与输出

`upload`、`screenshot`、`serve`、`rewrite` 共用存储源、路径、覆盖、压缩、EXIF 清理、最大宽度和可信 HTTP 来源参数。`--resize` 指最大宽度，只缩小不放大，不是最长边。`--optimize` 只在产物更小时采用：JPEG 重编码，不透明 PNG 尝试 JPEG／无损 WebP，透明 PNG 尝试无损 WebP。GIF、SVG、WebP、AVIF 等不支持处理的格式保留原始字节。

CLI 格式是 `url`、`markdown`、`html`、`json`；GUI 额外的 Markdown 链接和 BBCode 属于桌面复制功能。`--name`、`--progress` 都仅允许单文件。`--no-copy` 优先于 `--copy` 和配置；`--quiet` 或全局 `output.quiet` 会抑制普通结果输出，机器调用时请关闭静默设置。

## JSON 契约

```sh
img upload photo.png --format json --no-copy --progress
```

最终结果写 stdout，包含顶层 `success` 和 `files`。成功文件含本地路径、远端路径、URL、存储源、大小和内容类型。失败保留 `error`，可选增加 `error_code`、`http_status`、`retryable`。读取旧结果时不能要求新字段必定存在。

```json
{
  "success": false,
  "files": [{
    "local_path": "photo.png",
    "success": false,
    "error": "Service rate limit reached; try again later",
    "error_code": "rate_limited",
    "http_status": 429,
    "retryable": true
  }]
}
```

这是展示关键字段的示意结果。显式使用 `upload --format json` 时，配置加载和引擎初始化失败也返回结构化结果。

`--progress` 向 stderr 逐行写 JSON，字段为 `stage`、`sent`、`total` 和可选 `attempt`。传输字节包含 multipart／JSON 编码开销，不代表原图字节或服务端已确认；最终以结果为准。

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 1 | 上传全部失败 |
| 2 | 参数、配置、初始化或其他命令错误 |
| 3 | 上传部分成功 |
| 130 | 中断 |

## 平台差异

macOS 截图调用 `screencapture`；Linux 依次尝试 flameshot、scrot、gnome-screenshot、ImageMagick import；Windows 使用 PowerShell 全屏截图，区域和窗口参数不能提供等同 macOS 的交互。剪贴板需要对应系统命令和有效图形会话。

`fetch` 只下载，不上传；默认最大 8 MiB，可设置 1–128 MiB。`rewrite` 对文件默认原地改写，无文件时读 stdin；`serve` 默认监听 `127.0.0.1:36677`。配置详细解释见[存储配置](/img/docs/storage/)。

## 完整帮助输出

### img 

```text
Upload images from files, screenshots or links. Rust CLI included with img GUI.

Usage: img [OPTIONS] <COMMAND>

Commands:
  upload       Upload local files or remote image URLs
  fetch        Download an image URL without uploading it
  screenshot   Capture and upload a screenshot (copies result by default)
  serve        Run a PicGo-compatible editor upload server
  rewrite      Upload image references and rewrite Markdown documents
  info         Inspect image dimensions, type and EXIF presence
  init         Configure a storage provider interactively or with flags
  provider     List, show, select, remove or test storage providers
  config       Inspect or change configuration
  completion   Print a shell completion script
  version      Print application version
  install-cli  Add the bundled CLI to a directory on PATH (no GUI required to run it)
  help         Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
  -V, --version          Print version
```

### img upload

```text
Upload local files or remote image URLs

Usage: img upload [OPTIONS] <FILES>...

Arguments:
  <FILES>...  

Options:
      --config <CONFIG>      Use this global configuration file
      --provider <PROVIDER>  Storage provider name [default: ""]
      --path <PATH>          Remote path prefix [default: ""]
      --overwrite            
      --optimize             Compress images before uploading
      --strip-exif           Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>      [default: 0]
      --allow-insecure       Allow trusted plain HTTP image sources
      --format <FORMAT>      [possible values: url, markdown, html, json]
      --copy                 
      --no-copy              
      --quiet                
      --verbose              
      --name <NAME>          [default: ""]
      --progress             Write JSON progress events to stderr (one file only)
  -h, --help                 Print help
```

### img fetch

```text
Download an image URL without uploading it

Usage: img fetch [OPTIONS] --output <OUTPUT> <URL>

Arguments:
  <URL>  

Options:
      --config <CONFIG>      Use this global configuration file
      --output <OUTPUT>      
      --max-size <MAX_SIZE>  [default: 8388608]
      --allow-insecure       
  -h, --help                 Print help
```

### img screenshot

```text
Capture and upload a screenshot (copies result by default)

Usage: img screenshot [OPTIONS]

Options:
      --config <CONFIG>      Use this global configuration file
      --provider <PROVIDER>  Storage provider name [default: ""]
      --path <PATH>          Remote path prefix [default: ""]
      --overwrite            
      --optimize             Compress images before uploading
      --strip-exif           Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>      [default: 0]
      --allow-insecure       Allow trusted plain HTTP image sources
      --region               
      --window               
      --format <FORMAT>      [possible values: url, markdown, html, json]
      --no-copy              
      --verbose              
  -h, --help                 Print help
```

### img serve

```text
Run a PicGo-compatible editor upload server

Usage: img serve [OPTIONS]

Options:
      --config <CONFIG>      Use this global configuration file
      --provider <PROVIDER>  Storage provider name [default: ""]
      --path <PATH>          Remote path prefix [default: ""]
      --overwrite            
      --optimize             Compress images before uploading
      --strip-exif           Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>      [default: 0]
      --allow-insecure       Allow trusted plain HTTP image sources
      --bind <BIND>          [default: 127.0.0.1]
      --port <PORT>          [default: 36677]
  -h, --help                 Print help
```

### img rewrite

```text
Upload image references and rewrite Markdown documents

Usage: img rewrite [OPTIONS] [FILES]...

Arguments:
  [FILES]...  

Options:
      --config <CONFIG>      Use this global configuration file
      --provider <PROVIDER>  Storage provider name [default: ""]
      --path <PATH>          Remote path prefix [default: ""]
      --overwrite            
      --optimize             Compress images before uploading
      --strip-exif           Remove JPEG EXIF metadata, preserving orientation
      --resize <RESIZE>      [default: 0]
      --allow-insecure       Allow trusted plain HTTP image sources
      --stdout               
  -h, --help                 Print help
```

### img info

```text
Inspect image dimensions, type and EXIF presence

Usage: img info [OPTIONS] <FILES>...

Arguments:
  <FILES>...  

Options:
      --config <CONFIG>  Use this global configuration file
      --format <FORMAT>  [default: text] [possible values: text, json]
  -h, --help             Print help
```

### img init

```text
Configure a storage provider interactively or with flags

Usage: img init [OPTIONS]

Options:
      --config <CONFIG>                  Use this global configuration file
      --type <KIND>                      [default: ""]
      --name <NAME>                      [default: ""]
      --url <URL>                        [default: ""]
      --url-json-path <URL_JSON_PATH>    [default: data.url]
      --method <METHOD>                  [default: POST] [possible values: POST, PUT, PATCH]
      --file-field <FILE_FIELD>          [default: file]
      --endpoint <ENDPOINT>              [default: ""]
      --region <REGION>                  [default: auto]
      --bucket <BUCKET>                  [default: ""]
      --access-key <ACCESS_KEY>          [default: ""]
      --secret-key <SECRET_KEY>          [default: ""]
      --session-token <SESSION_TOKEN>    [default: ""]
      --public-url <PUBLIC_URL>          [default: ""]
      --path-style                       
      --allow-insecure                   
      --owner <OWNER>                    [default: ""]
      --repo <REPO>                      [default: ""]
      --branch <BRANCH>                  [default: main]
      --token <TOKEN>                    [default: ""]
      --commit-message <COMMIT_MESSAGE>  [default: "upload: {path}"]
  -h, --help                             Print help
```

### img provider

```text
List, show, select, remove or test storage providers

Usage: img provider [OPTIONS] <COMMAND>

Commands:
  list    
  show    
  use     
  remove  
  test    
  help    Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider list

```text
Usage: img provider list [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider show

```text
Usage: img provider show [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider use

```text
Usage: img provider use [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider remove

```text
Usage: img provider remove [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img provider test

```text
Usage: img provider test [OPTIONS] <NAME>

Arguments:
  <NAME>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config

```text
Inspect or change configuration

Usage: img config [OPTIONS] <COMMAND>

Commands:
  path      
  list      
  validate  
  get       
  set       
  unset     
  help      Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config path

```text
Usage: img config path [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config list

```text
Usage: img config list [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config validate

```text
Usage: img config validate [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config get

```text
Usage: img config get [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config set

```text
Usage: img config set [OPTIONS] <KEY> <VALUE>

Arguments:
  <KEY>    
  <VALUE>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img config unset

```text
Usage: img config unset [OPTIONS] <KEY>

Arguments:
  <KEY>  

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img completion

```text
Print a shell completion script

Usage: img completion [OPTIONS] [SHELL]

Arguments:
  [SHELL]  [default: bash] [possible values: bash, elvish, fish, powershell, zsh]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img version

```text
Print application version

Usage: img version [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
  -h, --help             Print help
```

### img install-cli

```text
Add the bundled CLI to a directory on PATH (no GUI required to run it)

Usage: img install-cli [OPTIONS]

Options:
      --config <CONFIG>  Use this global configuration file
      --dir <DIR>        
  -h, --help             Print help
```
