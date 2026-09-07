# macOS 桌面版发行

## 本地安装包

```sh
make desktop-package
```

生成当前架构的 release 构建、拖入 Applications 的 DMG、ZIP、各自 SHA-256 和 `build-info.json`：

```
dist/desktop/arm64/img-desktop_0.3.0_macos_arm64.dmg
dist/desktop/arm64/img-desktop_0.3.0_macos_arm64.zip
```

本地包使用 ad-hoc 签名，`build-info.json` 明确标记 `channel: development`、`notarized: false`。不能把它描述为通过 Apple 公证的正式发行包。构建版本统一读取 根目录 `Cargo.toml` 的 workspace.package.version，应用、安装包和随附引擎保持一致。最低系统版本为 macOS 13；正式 CI 同时构建 Apple silicon 与 Intel 包，本地此次只验收 Apple silicon。

## 正式签名与公证

需要 Apple Developer 的 **Developer ID Application** 证书及对应私钥，以及可用的 notarytool 凭据。先按 [Apple 公证流程](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) 在发布机器配置证书和钥匙串 profile；不要把密码或证书写入仓库。

设置发布机器环境变量后运行：

```sh
export IMG_SIGNING_IDENTITY='Developer ID Application: Your Name (TEAMID)'
export IMG_SIGNING_TEAM='YOURTEAMID'
export IMG_NOTARY_PROFILE='img-notary'
make desktop-release
```

`desktop/package.sh` 在构建前检查凭据参数；不会默默回退为未公证包。流程包含：

1. `--locked` release 构建与随附 Rust CLI 构建（与独立 CLI 使用相同 crate）。
2. 从内到外签名，引擎和应用启用 Hardened Runtime 与安全时间戳。
3. 提交应用 ZIP，要求 notarytool 返回 `Accepted`，再 staple 与 Gatekeeper 检查。
4. 生成 ZIP 和 DMG；签名、公证并 staple DMG。
5. 对最终产物生成 SHA-256、保存公证回执和构建信息。

脚本不会发布远端 release。安装时将 DMG 中的 `Img.app` 拖入 `Applications`，首次安装后打开；更新时先退出旧版，再替换应用。队列、偏好和存储配置在用户数据目录中，替换应用不会清空它们。打包方式参考 [Apple 分发文档](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution)。

## GitHub Actions

普通分支和 PR 即可运行以下检查，也可手动 `workflow_dispatch`，无需创建发布标签：

- `.github/workflows/ci.yml`：独立 CLI 的 macOS / Linux arm64、x86_64 和 Windows x86_64 构建、测试、Clippy、打包、首次安装、覆盖安装、本地 HTTP 上传和篡改拒绝；另运行 actionlint 和脚本语法检查。
- `.github/workflows/desktop-ci.yml`：Apple silicon / Intel 的工作区测试、Clippy、GUI 打包、同版本随附 CLI 安装检查、数据保留、篡改拒绝、签名与 Info.plist 验证。

这些检查只上传 CI 构建产物，不创建 release。Windows 安装测试使用 `install.ps1 -NoPathUpdate`，避免修改 runner 的用户 PATH；正常安装仍保留原来的 PATH 设置行为。

本轮只在 Apple silicon 实际运行本地安装验收，尚未推送或触发远端 CI。Linux、Windows、Intel 的运行结果不能由工作流配置代替，状态详见 [0.3.0 验收记录](../stability-qa.md)。

`.github/workflows/desktop-release.yml` 只响应 `desktop-v*` 标签，发布时设置 `latest=false`，与原有 CLI 的 `v*` 发布互不干扰。`desktop-v0.3.0` 必须匹配根目录 `Cargo.toml` 的 workspace.package.version。macOS 15 的 arm64 与 Intel runner 分别构建，两种产物都成功后才一起创建 release；runner 标识来自 [GitHub 官方说明](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)。

在 GitHub `desktop-release` environment 配置这些 secrets：

| Secret | 内容 |
| --- | --- |
| `MACOS_CERTIFICATE_P12_BASE64` | Developer ID Application 证书和私钥的 P12，Base64 编码 |
| `MACOS_CERTIFICATE_PASSWORD` | P12 密码 |
| `MACOS_SIGNING_IDENTITY` | 完整 Developer ID Application 身份名称 |
| `MACOS_TEAM_ID` | 10 位 Team ID |
| `APPLE_ID` | 用于公证的 Apple ID |
| `APPLE_APP_PASSWORD` | 对应 app-specific password |

工作流将证书导入临时钥匙串，公证凭据只存临时钥匙串，结束后清理。发布步骤在签名、公证成功之前不会执行。此轮没有创建标签、推送代码、修改仓库 secrets 或发布远端版本。

## 应用更新

“关于与更新”提供检查版本、版本说明、下载安装包与打开安装包。自动检查默认关闭，启用后启动时及运行期间每天最多检查一次。应用查询 `liyown/img` 的公开 GitHub releases，仅接受：

- 比当前版本新的稳定 `desktop-v` 标签；忽略 CLI、草稿、预发布和其他架构。
- 对应架构的确定名称 DMG 及 SHA-256 文件，下载地址必须属于该仓库该标签。
- 下载大小与 SHA-256 匹配。正式构建另校验签名发布团队。

更新不会自动替换或执行下载的应用；用户打开安装包并按提示替换旧版。下载失败或校验失败时，临时文件自动丢弃。上传未结束时禁止从应用打开更新安装包。

首次正式发行前不存在可安装的新版本；公开 GitHub API 限流时回退到公开的 releases Atom feed；如果仅能获得版本页面而无法核实安装资产，更新按钮会打开该版本页面。两个来源都不可用时显示可重试状态。本地验收覆盖版本筛选、错误处理和校验失败拒绝；没有宣称完成真实签名更新的下载、安装、公证验证。

## 独立 CLI 与 GUI 安装选择

版本统一在根目录 `Cargo.toml` 设置，目前为 0.3.0。`img-core` 是共用库，`img-cli` 构建名为 `img` 的可执行文件，GUI 包内直接附带它，不再有另一份引擎实现。所有 Go 源码、模块清单和 GoReleaser 已移除。

```sh
make install      # 源码安装 CLI 到 ~/.cargo/bin
make cli-package  # 本机架构的 CLI 压缩包和校验文件
```

CLI 的 `v*` 发布工作流构建 macOS arm64 / x86_64、Linux arm64 / x86_64、Windows x86_64；全部构建成功后发布同一 release，保留安装器已有的 `img_darwin_arm64.tar.gz` 等名称。GUI 使用 `desktop-v*` 独立发布，不覆盖 CLI 的 latest，缺少 Apple 签名凭据不会阻止单独发布 CLI。

仓库安装器提供 `sh install.sh --cli` 和 `sh install.sh --gui`。GUI 默认安装到 `~/Applications/Img.app`，同时在 `~/.local/bin/img` 建立链接；可用 `IMG_APP_DIR` 与 `IMG_INSTALL_DIR` 自定义。CLI 二进制没有 GUI 依赖，可单独拷贝使用。通过 DMG 拖入 Applications 的 GUI 也内置 CLI，可在设置中添加终端入口。更新 GUI 后，链接会指向包内新版 CLI。

离线安装本次本地包（不会下载旧的远端版本）：

```sh
IMG_LOCAL_PACKAGE_DIR="$PWD/dist/cli/aarch64-apple-darwin" sh install.sh --cli
IMG_VERSION=0.3.0 IMG_LOCAL_PACKAGE_DIR="$PWD/dist/desktop/arm64" sh install.sh --gui
```

两种安装都先校验 SHA-256；GUI 另外验证应用签名。不修改 shell 配置，命令目录不在 PATH 时会明确提示。安装器操作的是应用文件与命令入口，保留现有配置与图片队列。

HTTP / S3 / GitHub 的迁移测试使用本地服务器和虚构凭据。S3 签名采用 [AWS Rust SigV4](https://docs.rs/aws-sigv4/latest/aws_sigv4/http_request/index.html)，默认凭据读取沿用 [AWS Rust 配置链](https://docs.rs/aws-config/latest/aws_config/)。本轮未进行真实云图床上传、Apple 公证或远端发布。
