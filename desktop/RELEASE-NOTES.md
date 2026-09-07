## img desktop — 社区版

原生 macOS / Windows / Linux 图片上传应用，内置同版本 Rust CLI，安装后无需编译工具。

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

Windows / Linux 使用 Ctrl+Alt+U / Ctrl+Alt+S。Linux 全局快捷键需要 X11，Wayland 可使用窗口入口；截图需要系统截图工具。Windows 当前截取全屏。菜单栏后台入口仅在 macOS 提供。

「设置 → 关于与更新」可检查后续版本、下载并校验、退出并安装更新，保留原有配置和图库；也可手动替换。未公证的新版本可能需要再次允许打开。目录不可写时请使用 Finder 安装。

本版还包含：图库多选、跨搜索保留选择、五种格式批量复制、本地记录清理、队列持久化、失败诊断、后台快捷上传与大图库虚拟列表。

Windows 更新运行安装向导，Linux 更新请求系统权限后由包管理器安装；配置和图库保留。

每个 DMG / ZIP / EXE / DEB 均附 SHA-256 校验文件。CLI 使用「关于与更新 → 添加终端命令」即可启用。
