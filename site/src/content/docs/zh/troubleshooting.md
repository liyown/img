---
title: "故障排查"
description: "识别上传错误，恢复队列，并导出最少必要的诊断信息。"
locale: zh
topic: troubleshooting
order: 4
---
## 上传失败先看错误分类

| 错误代码 | 处理方法 |
| --- | --- |
| `invalid_config` | 检查默认源、必填字段和配置格式 |
| `authentication` / `permission` | 检查凭据有效性、桶或仓库写入权限 |
| `network` / `timeout` | 检查网络和服务地址后重试 |
| `rate_limited` / `server` | 稍后重试，必要时降低并发 |
| `conflict` | 换名，或确认后启用覆盖 |
| `too_large` | 检查本地与服务端上限，压缩或缩小图片 |
| `invalid_image` | 检查图片是否损坏或格式受支持 |
| `file_not_found` / `io` | 重新选择原文件，检查读写权限 |
| `invalid_response` | 检查 HTTP 响应字段和公开图片地址 |
| `cancelled` / `unknown` | 根据当前任务状态检查详情 |

GUI 提供修改存储配置、重试、选择原文件和查看详情入口。旧 JSON 缺少新错误字段时仍能显示原有 `error`。网络、超时、限流和服务端错误通常可重试；确认远端是否已有图片再决定重复上传。

## 恢复损坏队列

检测到损坏后不会自动开始上传。选择一份有效备份恢复，或保留损坏文件后重建空队列。系统保留最近两份有效队列备份；缺少有效备份时不能凭空恢复历史。重建队列不会找回原来的记录映射。

不要在应用运行时手改队列。需要人工备份时先退出，再复制 `~/Library/Application Support/aperture`。

## 导出诊断

在应用的诊断入口主动导出。诊断只保留应用版本、错误分类和操作阶段，不包含凭据、请求正文或 URL 查询参数。它不是完整日志或队列备份。向项目反馈问题时，补充可复现步骤与系统版本即可。

## 配置与 CLI

```sh
img version
img config path
img config validate
img provider list
```

遇到项目配置报错，检查当前目录 `.img.toml`：这里只允许存储源、输出格式与上传路径等受限字段。全局 `output.copy` 不能放进项目配置。

GUI 内置 CLI 可在「设置 → 关于与更新 → 添加终端命令」建立入口。若终端找不到 `img`，按提示把 `~/.local/bin` 加入 PATH；源码 `cargo install` 通常安装到 `~/.cargo/bin`。

## 平台与预览边界

截图和剪贴板依赖平台工具与权限。Linux 需要可用的截图／剪贴板工具，Windows 截图为 PowerShell 全屏方式。macOS 屏幕录制或通知权限由系统管理；通知被拒绝时仍可查看应用队列。

当前本地 GUI 安装包是开发签名，尚未 Apple 公证。正式签名、真实云上传和部分原生桌面交互验收仍待完成。查看[当前验收记录](https://github.com/liyown/img/blob/main/stability-qa.md)，或在 [GitHub Issues](https://github.com/liyown/img/issues)反馈可复现问题。
