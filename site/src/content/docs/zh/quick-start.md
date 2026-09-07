---
title: '快速开始'
description: '从配置第一个存储源，到复制第一条图片链接。'
locale: zh
topic: quick-start
order: 0
---

## 先选你的工作方式

需要拖放上传和图库，可以安装原生 macOS GUI。终端、编辑器或自动化任务可使用独立 CLI。GUI 已包含同版本 CLI，两种方式共用上传核心与存储配置。

先从[安装页](/img/install/#gui)获取最新桌面社区版，无需编译；按页面说明完成首次打开。

## 在桌面端上传

1. 打开「设置 → 存储源」，选择 R2、S3、OSS、GitHub 或自定义 HTTP。
2. 填写地址与凭据，保存并设为默认。可先测试连接。
3. 回到上传队列，拖入图片，或选择文件、粘贴、截图、导入链接。
4. 图片先进入待上传队列。检查存储源与处理参数，点击「上传」。
5. 上传完成后默认复制成功链接，在编辑器中粘贴即可。

界面导入每批最多 50 张，默认单张上限 8 MiB；设置可调整到 1 到 128 MiB。先用一张小图确认存储配置。

## 在终端上传

```sh
img init
img config validate
img provider list
img upload photo.png --format markdown --copy
```

`img init` 会引导建立存储源。普通 CLI 上传默认输出 URL，复制需加 `--copy` 或设置 `output.copy`；截图命令默认复制结果。

```sh
img screenshot --region --format markdown
img upload a.png b.webp --format json --no-copy
img upload https://example.com/photo.jpg --provider r2
```

最后一个地址是示例，请换成自己的图片链接。远程图片导入默认要求 HTTPS。

## 调整常用默认值

```sh
img config set output.format markdown
img config set output.copy true
img config set upload.retry_count 3
img config set upload.strip_exif true
```

GUI 的复制格式和上传偏好也可直接在分区设置中选择。继续阅读[存储配置](/img/docs/storage/)或[桌面使用](/img/docs/desktop/)。
