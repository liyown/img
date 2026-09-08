---
title: '迁移与资料管理'
description: '统一图库、图片处理、备份恢复、目录监听和远端文件。'
locale: zh
topic: workflows
order: 9
---

本文介绍 0.4 的上传、迁移与资料管理流程。统一图库、单图工具、同步及引用修复见 [0.4 使用指南](/img/docs/0.4/)。

## 从其他工具迁入

在桌面「存储源」选择「导入 PicGo / PicList 配置」，打开 JSON。先查看候选存储源与无法导入的项目，再逐项「检查并添加」。已有同名配置不会被覆盖，新配置自动使用后缀名称。保存后可以测试连接，再设为默认。

当前识别 GitHub、阿里云 OSS 和 PicList 的 AWS S3 配置，包括多配置列表。不执行插件脚本；不支持的插件和 URL 后缀处理会列出。原配置文件保留。

```sh
img import-config piclist.json          # 只预览，不输出密钥
img import-config piclist.json --apply  # 使用系统凭据存储保存
img provider test imported-name
img provider use imported-name
```

## 同一份图库

CLI、编辑器服务、截图、文章转存和 Agent 的成功上传立即写入共享 SQLite 图库，不必等待桌面启动。旧队列与上传收件箱幂等导入；多个进程可以同时写入。新上传即使记录保存失败，也会返回成功 URL 并明确说明保存问题。

```sh
img upload image.png --origin agent --format json
img upload private.png --no-history
```

--no-history 禁止此次上传保留图库记录和图片副本。IMG_DATA_DIR 可为 CLI 与桌面指定共享的隔离数据目录。窗口手动导入仍进入待上传队列；全局快捷键直接上传并复制成功链接。

## 重复图片与链接检查

在上传设置启用「重复图片复用已有链接」，或使用 --reuse。img 对处理后的图片计算内容哈希，并按存储配置、目录及命名规则隔离缓存。重复内容返回已有链接；默认不跳过上传。指定新名称时不复用旧链接。

```sh
img upload image.png --reuse
img upload image.png --reuse --force
img check https://img.example.com/image.png
```

--force 跳过链接复用；远端已有同名文件时，仍需明确使用 --overwrite，或选择随机名称。检查链接不发送存储凭据，区分访问拒绝、文件不存在、限流、服务故障及返回登录页等情况。图库链接菜单也提供「检查公开链接」。

通过 img 删除远端文件会使该存储源的复用缓存失效。在其他工具中删除后，请强制上传；复用本身不会联网确认旧链接。

## 图片处理与水印

「上传与图片处理」提供原始尺寸、网页配图、照片三组预设，可设置最长边、PNG / JPEG / WebP 输出、JPEG 质量与右下角图片水印。「保存并预览图片」比较原图和结果、尺寸及文件大小，不上传。

```sh
img process photo.png --preset web --output preview.webp
img upload photo.png --image-format jpeg --quality 85 --max-edge 2400
img upload photo.png --watermark /absolute/path/logo.png --watermark-opacity 60
```

旧上传处理参数的最长边保持比例、不放大图片，JPEG 透明部分填白，WebP 保持无损。独立工具与 ProcessingPlan 另外支持 JPEG 背景色及有损 WebP 质量／目标体积模式。水印宽高限制在图片的四分之一内，默认透明度 60%。桌面水印路径必须为绝对路径。

显式处理支持 PNG、JPEG 和静态 WebP。GIF、动态 WebP、SVG、AVIF 原样上传时保留；不支持的显式转换会失败，不会默默丢弃动画。处理操作上传副本，不修改原文件。CLI 显式处理参数覆盖全局处理预设。

## Markdown 转存与恢复

「文章与目录」选择 Markdown 文件，预览引用和缺失文件数量，确认后再上传并替换。成功项替换，失败项保留；同目录生成逐图片结果报告与原文备份。

```sh
img rewrite article.md --dry-run
img rewrite article.md --report article-results.json
img restore-document article.md.img-backup-UUID article.md
```

桌面「恢复文章备份」可选择 img 生成的备份并确认恢复。备份名称包含 UUID，不覆盖既有备份。恢复前也备份当前文章，可以撤回恢复。--stdout 只输出改写结果，不修改原文；报告路径必须为新文件。

## 文件夹与目录监听

桌面选择器和拖放递归收集文件夹图片，每批最多 50 张。CLI 的 --recursive 单次最多 10,000 张。符号链接不会被递归跟随。

```sh
img upload ./screenshots --recursive --format json
img watch ./screenshots --reuse
img watch ./screenshots --new-only --interval 2
```

监听等待大小与修改时间连续两次不变后上传；只有成功才将该版本标为已处理。--new-only 跳过启动时已有文件。桌面在「文章与目录」启动或停止监听，关闭窗口继续，退出应用停止。监听不跨重启自动恢复。监听目录不能与应用数据目录互相包含，以免上传缓存图片形成循环。

## 本地备份

「备份设置与图库」先选择保存位置，再选择设置与记录、加入缓存、或加入缓存与凭据。备份是带 SHA-256 清单的目录，不上传至任何服务。

```sh
img backup ./img-backup
img backup ./img-full-backup --include-cache --include-credentials
img restore ./img-backup                 # 校验和预览，不修改数据
img restore ./img-backup --apply         # 先退出桌面应用
```

默认不包含图片缓存和明文凭据。没有缓存的记录仍保留链接；已关联的远端对象可以按需重新下载预览，未关联的旧记录可能需要重新选择原文件。钥匙串凭据及明文密钥只有明确选择后才导出，**凭据备份不加密**，请存放在私人位置。

恢复替换备份包含的设置与记录，按文件恢复缓存，不清空无关文件。恢复前生成 img-before-restore-UUID 副本。恢复副本保留原设置的原始内容；旧配置中的明文密钥也会保留。导入凭据时，也会备份当前配置引用的钥匙串凭据，以便回退，请勿公开分享恢复副本。应用中的恢复按钮先安全保存队列并退出，再恢复并重新打开。损坏备份或中途失败不报告成功，失败时尝试回滚并给出恢复副本位置。原始图片与远端存储不受影响。

## 远端文件

在「存储源」选择索引范围后，从统一图库搜索、预览、下载和管理远端图片。高级 CLI 仍提供按目录浏览和单个对象删除。支持 S3 兼容服务、GitHub、WebDAV。自定义 HTTP 上传接口没有统一的浏览和删除协议。

```sh
img remote --provider r2 list --prefix posts/
img remote --provider r2 list --prefix posts/ --cursor TOKEN
img remote --provider r2 delete posts/image.png --version '"ETAG"' --yes
```

删除需要列表返回的 ETag 或 GitHub SHA。文件变化时需刷新再确认；没有版本标识时不提供删除。目录删除不开放。清缓存保留图库，隐藏不删除图片；**远端删除影响已有链接**。S3 启用版本控制时可能生成删除标记，旧版本仍由存储服务管理。

GitHub 单目录达到 Contents API 的 1,000 条上限时会报出限制，可进入较小目录。远端列表不加载所有原图作为缩略图。

## WebDAV

连接提供 PUT、MKCOL、PROPFIND 的服务。桌面填写目录地址、公开访问地址和可选 Authorization 请求头；账号验证与图片公开访问可使用不同入口。

```sh
img init --type webdav --name dav --endpoint https://dav.example.com/images/ --public-url https://img.example.com --authorization '${DAV_AUTHORIZATION}'
```

环境变量的值为 Bearer 或 Basic 认证头。请勿把真实凭据写入命令历史。上传自动创建目录，默认用条件请求防止覆盖。公开 URL 应让文章读者无需 WebDAV 登录即可访问，保存后可检查链接。

## 平台与显示

- Windows：拖选截图区域，Esc 取消；提供托盘入口。
- Linux Wayland：截图与快捷键经桌面门户授权；全局快捷键取决于桌面环境的 GlobalShortcuts 支持。X11 保留原快捷键流程。托盘依赖 StatusNotifier 支持。
- macOS：保留菜单栏与区域截图流程。
- 深色主题即时切换并保存；中英文界面重新打开应用后生效。系统文件选择器语言也受操作系统设置影响。
