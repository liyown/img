---
title: '编辑器与 Agent 集成'
description: '让图片上传成为写作、脚本和自动化的一部分。'
locale: zh
topic: integrations
order: 3
---

## Typora

将图片上传方式设为自定义命令：

```sh
img "${filepath}"
```

确保编辑器能找到 `img`。如果编辑器的 PATH 与终端不同，请使用 CLI 的绝对路径。

## Obsidian 与 PicGo 协议

```sh
img serve --bind 127.0.0.1 --port 36677
```

支持 PicGo 上传协议的编辑器插件可使用 `http://127.0.0.1:36677/upload`。服务默认只监听本机，使用当前存储配置；可附加 `--optimize --strip-exif --resize 1200`。不要把这个无认证的本地代理暴露到公网。

## Markdown 批量转存

```sh
img rewrite article.md
img rewrite first.md second.md --optimize
img rewrite article.md --stdout
cat article.md | img rewrite
```

有文件参数时默认原地改写；使用 `--stdout` 可先检查结果。支持 Markdown 图片与 HTML `img` 引用，保留 alt 和 title，只替换图片地址。`data:` 等不能上传的引用保持原样。重要文章改写前保留版本记录。

## VS Code 与 Raycast

以下两个扩展通过仓库源码安装：

- [VS Code 集成](https://github.com/liyown/img/tree/main/integrations/vscode)：Markdown 中粘贴图片上传，资源管理器右键上传。
- [Raycast 集成](https://github.com/liyown/img/tree/main/integrations/raycast)：截图、剪贴板和文件上传命令。

按各自 README 安装，并配置可用的 `img` 命令。GUI 包里已有 CLI，可在「关于与更新」添加终端入口。

## Agent 与脚本

```sh
img upload cover.png --format json --no-copy --progress
```

stdout 是最终 JSON，stderr 是独立的进度事件。`--progress` 限单文件；脚本应同时判断退出码与各文件的 `success`。失败结果保留 `error`，并可提供 `error_code`、`http_status`、`retryable`。不要只凭进度达到 100% 就判断上传成功。

安装配套 Skill，让支持 Agent Skills 且能执行本地命令的助手使用 img：

```sh
npx skills add liyown/img --skill img-uploader
```

先安装原生 CLI 并配置默认存储源，再让 Agent 上传本地图片、把返回的链接写进文章。Skill 会检查文件与配置，读取逐文件 JSON 结果，保留部分成功的链接。Agent 复用已有配置，无需在对话中输入存储密钥。完整流程见 [SKILL.md](https://github.com/liyown/img/blob/main/skills/img-uploader/SKILL.md)。

[GitHub Action](https://github.com/liyown/img/blob/main/action.yml) 可在工作流中转存 Markdown 图片。安装步骤获取最新已发布 CLI；固定 Action 提交不会固定下载的 CLI 版本。需要可复现版本时，在工作流中下载并校验指定 CLI 发行包，再调用 `img rewrite`。
