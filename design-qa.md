# img desktop design QA — 2026-09-05

This is the earlier design and Rust migration record. Current stability work and its incomplete acceptance items are tracked in [0.3.0 stability QA](stability-qa.md).

final result: passed

## Scope and evidence

- Source visual truth: `design/reference-titlebar.png` (3024 × 64). The user requested a similar titlebar layout, then smoother transitions. The supplied image covers only the titlebar.
- Implementation: `target/Img.app`, native Rust / GPUI. Final ordinary window was reopened and inspected, not just the isolated QA build.
- Implementation screenshot: inline CUA captures in this task titled “把参考图与最终顶部布局放在一起对照” and “最终核对普通窗口与参考图的顶部结构”. CUA returned image bytes rather than a filesystem export; no nonexistent screenshot path is claimed.
- Both comparisons emitted the source and implementation together in the same tool result. Final capture is JPEG, 1093 × 768, from the default 1280 × 900 logical window. The compact QA capture was 960 × 700. No browser CSS viewport applies to this native app.
- Density: the reference is a narrow, high-density crop; comparison uses titlebar structure, icon order, hierarchy, alignment, and control density rather than claiming equal full-window pixels. The implementation uses a 36 px logical titlebar. A system capture indicator covers the native traffic-light area in CUA images and is not app content.
- Theme: preserved the existing light palette after asking whether the reference implied dark mode and stating the light-layout assumption. The dark reference palette is not claimed to be reproduced.
- States inspected: sidebar expanded and fully collapsed; queue empty / ready / failed / completed; settings with existing sources, new form and saved form; format menu; navigation history; grid and list preference controls.

## Comparison history

1. [P1, fixed] The separate “工作空间” row and large solid brown navigation selection were visibly heavier than the new reference. Moved sidebar toggle, back and forward into the titlebar; removed that extra row; reduced navigation rows to 36 px, with neutral selected background and readable icons/text. Post-fix combined capture confirms the continuous bar and correct left-to-right control ordering.
2. [P2, fixed] The first revised titlebar was still 48 px high and its extra upload CTA competed with the actual upload controls. Reduced it to 36 px, removed the redundant top upload CTA, and retained search / storage at the right. Final ordinary-window combined capture confirms this arrangement.
3. [P2, fixed] State switches were immediate. Added a shared sidebar width spring, a moving navigation selection background, 160 ms content entrance, and spring color transitions for filters and grid/list selection. Native trace contains intermediate widths (240, 236.76, 225.44, …), and a reversal from 4.78 through 7.47, 15.35, … toward 240. Repeated toggles reached stable expanded/collapsed layouts without a snap to the opposite endpoint. Framework motion APIs honor system reduced-motion preferences and stop requesting frames once settled.

## Required fidelity surfaces

- Typography: reused bundled Inter and system Chinese fallback. Compact 13 px title, 17–18 px line icons, clear navigation labels; no title or upload-action clipping at the minimum size.
- Spacing/layout: titlebar forms one compact horizontal row; all file/paste/screenshot/format/upload controls fit below the drop area at 960 × 700. Sidebar can release all its width; content scrolls independently of titlebar/footer. Settings forms scroll without obscuring persistent navigation.
- Color/tokens: existing light palette retained; navigation selection uses #E4DACB with dark #221811 labels. Disabled history controls are visibly muted. The earlier large brown selection has been removed.
- Images/icons: retained existing bundled assets. Sidebar toggle and history arrows use packaged GPUI Kit icons; navigation uses bundled Phosphor icons. No screenshot was used as interactive UI and no new raster assets were needed.
- Copy/content: title shows the active page; controls describe file selection, clipboard paste, capture, link format and upload. Settings are separated into storage, clipboard/link behavior and application preferences. No unconfigured example source is presented as a working cloud provider in the normal app.

## Functional validation

- `cargo test -p img-desktop`: 9 passed.
- `go test -race ./...`: passed.
- `git diff --check`: passed.
- Native file dialog imported a test PNG into the queue using the new bottom-row file button.
- Local HTTP end-to-end upload: two images uploaded sequentially; success URLs were automatically copied as plain links. Pasting into the search field confirmed both URLs were present (single-line input normalizes newlines).
- Changed the quick format to Markdown image, imported a new image with the native chooser, uploaded it, and pasted back `![photo-1.png](https://cdn.example.test/photo-3.png)` without clicking a copy button.
- Settings: blank required name blocked save without losing the draft; Chinese source name and HTTP fields saved, appeared in the source list, and were read back from the isolated TOML. “Set default” updated both the form state and the top storage selector.
- Sidebar state and chosen copy format survived a process restart. Back and forward restored queue/settings.
- Storage tests cover preserving unrelated settings, avoiding plaintext secrets, retaining existing credentials on blank edit, duplicate-name / malformed-config rejection, and rolling back newly saved credentials after a credential-store failure.
- No real cloud upload was performed; credential tests use an in-memory store and do not claim to validate a user's Keychain authorization. The normal app retained its original configured storage source when reopened.

## Follow-up polish

- No outstanding P0/P1/P2 issue in the requested scope. A full dark theme was not selected and is not part of this pass.

## 2026-09-05 桌面功能补全验收

范围：按用户要求补齐队列管理、存储管理、高级上传和发行流程；明确不执行真实 S3 / R2 / OSS / GitHub 上传与实际存储凭据授权测试。

本次新增验证：

- 原生窗口 1280×900、960×700：操作栏、队列按钮、分区设置和高级 HTTP 键值编辑没有重叠；截图在本任务原生工具输出中。
- 本地 HTTP fixture（`desktop/tests/http_fixture.py`），隔离配置和数据目录：三项并发，界面观测到 53%、83% 与“等待确认”；服务端峰值并发 3，请求总长度 1,891,586 字节。
- 暂停全部：三个上传子进程全部退出，队列持久化为三个 Paused 和一个 Ready；恢复后四项全部完成，原顺序的四个 URL 自动复制。
- 高级参数通过原生设置保存为：宽度 640、并发 2、重试 1、压缩 / EXIF 清理开启；重启后保留。新上传请求体缩到约 197,030 字节，界面显示原始与上传大小。
- 单个链接与剪贴板两行链接导入均成功，原始字节进入应用自己的队列目录。
- 本地服务返回 503：按设置自动尝试两次，保留失败项；恢复服务后“重试失败项”成功。
- 取消本批：两项变为 Cancelled，子进程停止；不会发起远端删除。
- 上传中 ⌘Q：原生弹窗提供继续使用、上传完成后退出、暂停并退出。选择暂停并退出后，进程结束，队列保存 Paused；重启后“继续队列”可用。
- 清理六条已完成测试记录：剩余一条暂停任务和其缓存，四份测试原始文件仍在；清理范围在原生确认框内明确展示。
- 本地 HTTP 连接测试成功；高级表单保存 PATCH 和 X-Client 请求头；删除最后一个测试源后默认源和选择器正确变为空状态。非空凭据保存 / 回滚 / 未使用凭据清理通过内存凭据替身测试，不访问真实存储钥匙串。
- 更新入口、网络错误状态、稳定版本与架构筛选、拒绝外域资产、下载完整性与篡改拒绝均已检查。首次公开更新接口返回限流 403，已补上公开 Atom feed 回退；未宣称完成真实新版安装。
- Rust 测试 17 项通过，Go 全仓 `go test -race ./...` 与 `go vet ./...` 通过；包含实际本地 HTTP 字节统计、压缩 / 缩放 / EXIF 清理、动画保留和透明像素保留的验证。
- DMG / ZIP 本地 release 包生成并校验；镜像结构、SHA-256、应用 ad-hoc 签名与 Info.plist 验证通过。
- 最终 release 包再次通过 SHA-256、`hdiutil verify` 和只读挂载检查；包内 `img-engine version` 返回 0.2.0。两份 GitHub Actions 工作流通过 actionlint，Rust 格式和 shell 语法检查通过。
- 最终回看时电脑已锁定，原生工具无法访问窗口。因此 Atom 回退和编辑器关闭后的焦点修复已通过构建 / 自动测试，但未完成最终版本的原生窗口回看；AVIF 预览也未完成这次手动检查。前述原生交互结果来自本轮此前已完成的隔离验收。

正式发布前提：本机 `security find-identity -v -p codesigning` 返回 0 个有效身份；仓库 secrets 为空，`desktop-release` 环境尚未配置。因此没有执行 Developer ID 正式签名、Apple 公证、远端发布或正式更新安装。完整脚本与凭据清单位于 `desktop/RELEASING.md`，本地包明确标记 development / not notarized。Intel 构建已纳入 CI，尚未在本机实测。

## 2026-09-05 Rust 迁移与安装选择

- 版本统一为 0.3.0：`img-core` 共用库、`img-cli` 独立二进制、`img-desktop` 原生界面。CLI 依赖图不含 GPUI / desktop。GUI 包内直接附带 `Contents/MacOS/img`，旧 `img-engine` 已移除。
- Go 源码、go.mod / go.sum、GoReleaser 及 CI 的 Go 构建步骤已移除。现有 TOML v1、JSON 结果字段、常用命令与退出码继续可用；没有清空或迁移用户数据目录。
- 整个 Rust 工作区测试 41 项通过：核心 12、CLI 单元测试 3、CLI 端到端测试 9、GUI 测试 17。Clippy（核心 / CLI，全部 targets，warnings 视为错误）、格式检查和四份工作流 actionlint 均通过。
- HTTP / S3 / GitHub 使用本地替身验证 multipart、实际请求体进度、SHA-256 签名、路径编码、会话令牌、覆盖检查、自动重试和凭据错误脱敏。没有使用实际图床凭据。
- 命令端到端覆盖默认上传、部分失败 JSON / 退出码、链接下载和禁止覆盖、缩放、图片信息、Markdown 与 stdin 失败处理、PicGo JSON / multipart 服务、配置管理及 shell 补全。
- `scripts/test-installers.py` 分别将实际 CLI 压缩包和 GUI ZIP 安装到临时目录。PATH 只保留系统工具，不含 Go / Rust；两者都返回 Rust 0.3.0 并完成本地上传、进度输出和原图 / 配置保留验证。
- GUI 安装器自动建立到包内 CLI 的终端链接；单独 CLI 安装目录没有 GUI。CLI 与 GUI 的篡改包都在写入应用或命令前被 SHA-256 校验拒绝。发布工作流已纳入安装器检查。
- 本机 arm64 CLI tar.gz、GUI DMG / ZIP 生成成功；校验和、镜像校验、应用签名和 Info.plist 检查通过。当前包为 ad-hoc 本地测试包，没有 Apple 公证。
- 在隔离工作目录对现有全局配置运行 Rust `config validate` 通过。仓库已有 `.img.toml` 包含项目配置不允许的 output.copy 字段；严格项目边界继续拒绝该字段，未擅自修改此文件。GUI 仍使用隔离工作目录，不受它影响。
- 普通应用已替换为 Rust 0.3.0 并启动；启动前确认旧队列为空且无上传子进程，启动前后现有配置、偏好和队列文件哈希相同。电脑仍锁定，未完成这版新增终端入口按钮的原生窗口回看。其他架构与 Windows / Linux 的构建及安装检查已配置在 CI，本机未实测。
