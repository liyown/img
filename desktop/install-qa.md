# 社区安装与更新链路验收

## 本地验证（2026-09-07）

- `cargo test --locked --workspace --quiet`：core / CLI / desktop 全部通过，其中 desktop 35 项。
- `cargo clippy --locked --workspace --all-targets -- -D warnings` 和格式检查用于发布前门禁。
- 本机 arm64 release DMG / ZIP 已构建，应用和内置 CLI 通过签名完整性验证。
- `python3 scripts/test-installers.py --gui-only`：无 Rust / Go 的 PATH 下，首次安装、覆盖安装、内置 CLI、本地 HTTP 上传、进度、数据保留和篡改拒绝通过。
- 安装助手测试：等待旧进程退出后替换；启动命令失败回退旧应用；不合法应用拒绝；安装回执须匹配正在运行版本。测试替换了最终 open 命令，未伪称 Apple 公证或真实新版本启动健康检查。
- 网站测试验证语义版本排序、排除 CLI / 草稿 / 预发布、校验资产地址、缺少架构不生成虚假链接，以及新增 release 后固定页面无需改代码即可选择新版。

## 发布边界

三端 CI 实际验收（2026-09-07）：

- [Desktop CI 34096425657](https://github.com/liyown/img/actions/runs/34096425657)：macOS arm64 / Intel、Windows x64、Linux x64 四个任务全部通过，包含工作区测试、Clippy、安装包校验与隔离数据目录原生窗口启动。
- [CLI CI 34096425738](https://github.com/liyown/img/actions/runs/34096425738)：五个平台架构测试全部通过；workflow lint 发现发布脚本通配符缺少 `./`，已修正，后续 CI 复验。
- [网站 CI 34096425640](https://github.com/liyown/img/actions/runs/34096425640)：测试、构建和 GitHub Pages 部署通过。

这些结果证明原生构建和基础安装启动，不等同于 Windows / Linux 实机完整交互验收。

正式对外安装包由 `desktop-release.yml` 在 CI 构建，不上传本机临时产物。社区版 ad-hoc 签名，首次运行及新版可能需要用户在 macOS 系统设置中允许打开。没有关闭系统保护或清理用户隔离属性。

首次发布尚无比当前版本更高的真实远端更新；版本选择、安装事务和保存失败路径分别验收，后续发布后还应验证实际跨版本用户流程。
