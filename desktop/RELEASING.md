# 三端桌面版与 CLI 发行

公开安装包由 GitHub Actions 构建、验收和发布，不依赖开发电脑上传二进制。版本读取根目录 Cargo.toml 的 workspace.package.version，GUI 与内置 CLI 保持一致。

## 发布步骤

更新版本和发行说明、提交并通过 CI 后，推送 desktop-vX.Y.Z 发布 GUI，推送 vX.Y.Z 发布独立 CLI。标签须与 workspace 版本一致。全部原生 runner 通过测试、打包和验收后统一创建 Release。

| 产品 | 平台 | 产物 |
| --- | --- | --- |
| GUI | macOS arm64 / Intel | DMG、ZIP、SHA-256 |
| GUI | Windows x64 | EXE、便携 ZIP、SHA-256 |
| GUI | Ubuntu 24.04 x64 | DEB、SHA-256 |
| CLI | macOS arm64 / Intel、Linux arm64 / x64、Windows x64 | TAR.GZ / ZIP、checksums.txt |

桌面发行使用 latest=false，CLI 使用 GitHub latest。官网固定[安装入口](https://liyown.github.io/img/install/#gui)访问时分别查询稳定桌面版和 Rust CLI，按语义版本选择并核对资产所属标签与校验文件。发布新版无需修改或重新部署网站，网络失败时回退 Releases 页面。

## CI 验收

ci.yml 验证五种 CLI 目标的测试、Clippy、打包、首次与覆盖安装、本地上传和篡改拒绝，另检查 workflow lint 与脚本语法。

desktop-ci.yml 验证四种 GUI 目标的工作区测试、Clippy、安装包与隔离数据目录原生窗口启动。Windows 执行 EXE 首次与覆盖安装；Linux 在 Xvfb 和软件 Vulkan 下启动。desktop-release.yml 重复测试、打包、安装验证与窗口启动，全部通过才发布。

实际结果见[安装验收记录](install-qa.md)。原生 runner 启动检查不等同于 Windows / Linux 实机完整交互验收。

## 社区包与可选 Apple 公证

默认发布社区版，无需 Apple Developer 账号。macOS 使用 ad-hoc 签名并附带 INSTALL.html，未经过 Apple 公证。首次启动及更新后可能需要在「系统设置 → 隐私与安全性」允许打开，见 [Apple 说明](https://support.apple.com/102445)。无需关闭系统保护。Windows 社区包也未进行代码签名。

启用 Developer ID 签名时，在 GitHub desktop-release environment 设置 DESKTOP_SIGNING=true，并配置 MACOS_CERTIFICATE_P12_BASE64、MACOS_CERTIFICATE_PASSWORD、MACOS_SIGNING_IDENTITY、MACOS_TEAM_ID、APPLE_ID、APPLE_APP_PASSWORD secrets。凭据只导入临时钥匙串，结束清理。正式签名或公证失败会阻止发布，不回退社区包。

## 安装与更新

macOS：打开 DMG 拖入 Applications，或 sh install.sh --gui 安装到 ~/Applications/Img.app 并链接 CLI。IMG_APP_DIR 和 IMG_INSTALL_DIR 可自定义目录。

Windows：运行 EXE 或 install.ps1 -Product gui，默认安装到当前用户应用目录。

Linux：sh install.sh --gui 或 sudo apt install ./img-desktop_*.deb。支持 Ubuntu 24.04 / Debian 13+ x64，需要图形桌面、Vulkan 驱动和已解锁的 Secret Service。系统包包含桌面入口和 CLI。

独立 CLI：macOS / Linux 使用 sh install.sh --cli，Windows 使用 install.ps1 -Product cli。CLI 可脱离 GUI 使用，Linux 同时提供 ARM64。

「关于与更新」检查稳定 desktop-v 新版本，下载对应平台安装包并校验大小与 SHA-256。保存队列并结束上传子进程后才退出安装。macOS 验证标识、版本、架构和签名，在同一文件系统暂存，由助手等待旧进程、备份替换并重新打开；启动命令失败时回退。Windows 打开安装向导；Linux 请求系统授权后由 apt 安装。配置和图库保留，也可手动安装。

自动检查默认关闭，启用后每天最多检查一次。API 失败可回退 Atom feed；无法核实资产时只打开版本页面。首次桌面发行没有更高版本可用于真实跨版本更新，安装事务和版本筛选由测试分别验证。

平台差异：Linux 全局快捷键需要 X11，Wayland 使用窗口入口；截图需要系统截图工具。Windows 当前截图为全屏。菜单栏后台入口仅在 macOS 提供。

## 本地开发

make install 源码安装 CLI，make cli-package 生成本机 CLI 包，make desktop-package 生成 macOS 包，python scripts/package-desktop.py 在 Windows / Linux 原生打包。本地包用于验收，公开产物由 CI 生成。

离线验收可通过 IMG_LOCAL_PACKAGE_DIR 指向产物目录，并设置 IMG_VERSION。安装器仍校验 SHA-256，macOS 还校验应用签名。测试使用临时目录和虚构凭据。
