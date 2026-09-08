## img 0.4.0 — 社区版

原生 macOS / Windows / Linux 图床管理与图片工具，内置同版本 Rust CLI，安装后无需编译工具。

### 这个版本

- 统一图库：索引 S3/R2/OSS、GitHub、WebDAV 的已有图片，与桌面和 CLI 新上传共同管理。网格与列表直接复制，多选支持批量下载、检查和迁移。
- 两个独立单图工具：转换与压缩、尺寸与裁剪。PNG/JPEG/静态 WebP，实际输出预览，结果可保存、复制或直接上传。
- 用自己的 WebDAV 或 S3 同步配置、图库索引与预设。缓存不上传；同字段冲突由用户选择。同步目录中的图床凭据为明文，连接同步存储的凭据仅保留在各设备。
- 跨图床复制后验证内容，默认保留源端图片；Markdown 引用按文件预览、备份后替换，支持报告和恢复。
- CLI 与 Agent Skill 覆盖处理、图库查询、同步状态、迁移与引用维护。
- 简化导航、原位复制反馈、扩大启动窗口、滚动条避让，以及 macOS 窗口边缘缩放光标。

[完整使用指南](https://liyown.github.io/img/docs/0.4/)

### 下载与首次安装

- Apple silicon（M 系列）：下载 `macos_arm64.dmg`。
- Intel Mac：下载 `macos_x86_64.dmg`。
- 打开 DMG，将 Img.app 拖到 Applications；ZIP 也可解压后安装。
- **本版为 ad-hoc 签名的社区安装包，未经过 Apple 公证。** 如果首次打开被阻止，在「系统设置 → 隐私与安全性」找到 img，点击「仍要打开」。仅在确认下载来自本项目时继续；无需关闭系统保护。参见 [Apple 官方说明](https://support.apple.com/102445)。
- DMG 内附完整安装说明。最低 macOS 13。
- Windows x64：下载 `windows_x86_64.exe`，运行安装向导；ZIP 可便携使用。社区包未进行代码签名。
- Linux x64：下载 `linux_x86_64.deb`，使用 `sudo apt install ./img-desktop_*.deb`。支持 Ubuntu 24.04 / Debian 13 或更新版本，需要图形桌面、Vulkan 驱动和已解锁的 Secret Service。

### 首次使用与更新

设置默认存储源后，⌘⌥U 直接上传剪贴板，⌘⌥S 截图上传，成功后自动复制链接。

Windows / Linux 使用 Ctrl+Alt+U / Ctrl+Alt+S。Windows 支持区域截图和托盘；Linux Wayland 通过桌面门户处理截图与快捷键，取决于桌面环境的 GlobalShortcuts 支持，X11 保留原快捷键流程。Linux 托盘需要 StatusNotifier 支持；macOS 使用菜单栏入口。

「设置 → 关于与更新」可检查后续版本、下载并校验、退出并安装更新，保留原有配置和图库；也可手动替换。未公证的新版本可能需要再次允许打开。目录不可写时请使用 Finder 安装。

清理本机缓存、隐藏图库记录和删除远端文件分别操作。删除会确认具体存储源与对象，校验版本；清缓存保留图库、原文件与远端图片。

Windows 更新运行安装向导，Linux 更新请求系统权限后由包管理器安装；配置和图库保留。

每个 DMG / ZIP / EXE / DEB 均附 SHA-256 校验文件。CLI 使用「关于与更新 → 添加终端命令」即可启用。
