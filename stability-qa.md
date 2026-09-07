# img 0.3.0 本地内测验收

记录日期：2026-09-07。版本仍为未发布的 `0.3.0`。本轮交付代码、功能提交和本地安装包，没有推送、打标签或发布。真实云上传、正式签名与公证未执行。

## 本轮实现

| 功能组 | 实现与兼容行为 | 提交 |
| --- | --- | --- |
| 诊断与恢复 | 保留 `error`，新增可选 `error_code`、`http_status`、`retryable`；JSON 初始化错误；脱敏导出；串行原子队列保存、两份有效备份、显式损坏恢复 | `0e313e3` |
| 图库性能 | 队列／历史虚拟列表、三／四列图库按行虚拟化；缓存筛选 ID；512 px 缩略图、64 MiB 解码 LRU；拆分调度、持久化和图库代码 | `f261319` |
| 桌面快捷操作 | 应用持有后台队列；关闭隐藏、显式退出等待；原生菜单栏、可配置快捷键、独立快捷批次、自动复制、系统通知 | `a5bd5f9` |
| 安装与验收 | 无标签跨平台 CI、首次／覆盖安装和篡改拒绝测试、超时测试、性能采样检查、使用与验收文档 | 本文所在提交 |

主窗口的导入仍需点击「上传」。快捷操作在触发时固定默认源、上传参数和链接格式，按批次顺序执行，不包含手动待上传项。未配置默认源时保留导入图片并进入存储设置。

队列仍使用原来的 JSON 和图片目录。`queue.backup-1.json`、`queue.backup-2.json` 保存最近两份有效状态；恢复时选择最近可用备份，或重建空队列。替换前将原文件字节保存为 `queue-preserved-<UUID>.json`，不删除图片副本，恢复的进行中项目转为暂停。上传启动、结果确认、缓存删除和正常退出都等待保存确认；强制终止进程不能等同于正常退出。

## 自动化与本地服务测试

Apple M4、16 GiB，macOS 27.0（26A5388g）；Rust stable 1.98.1。所有请求使用 loopback HTTP 与虚构凭据，测试配置和队列位于临时目录或 `target/stability-*`，没有读取真实图床凭据执行上传。

| 检查 | 本地结果 |
| --- | --- |
| 原有 41 项测试与本轮新增测试 | 合计 56 项通过：CLI 单元 3、CLI 集成 11、core 14、desktop 28 |
| 错误与兼容 | 配置加载失败退出 2、上传失败退出 1、部分成功退出 3；旧 JSON、HTTP 401／403／429／503／408 分类通过 |
| 网络与凭据 | loopback 延迟响应触发真实客户端超时；瞬态失败重试、永久失败不重试；诊断与 JSON 不泄露响应正文、令牌及 URL 查询参数 |
| 队列耐久性 | 保存顺序、等待屏障、两份有效备份、损坏阻止上传、恢复保留原字节、写入失败、退出等待保存与子进程回收通过 |
| 缓存与批次 | LRU 预算、旧预览补缩略图与原件保留、进度不重算 ID、快捷准备顺序与手动项隔离、按键重复过滤通过 |
| 本地安装 | Apple silicon CLI／GUI 首次安装与覆盖安装通过；GUI 内含同版本 CLI，终端入口可运行 |
| 数据与篡改 | 配置、偏好、队列及图片哨兵字节保留；被修改的 CLI／GUI 压缩包在首次安装及覆盖安装时均拒绝，已有可执行文件不被替换 |
| 包与工作流 | ad-hoc 签名验证、Info.plist 检查、Shell 语法、Python 语法和 actionlint 1.7.12 通过 |
| Rust 静态检查 | rustfmt 与整个工作区、全部 target、包含 perf 特性的 Clippy 通过（`-D warnings`） |

安装测试的 PATH 排除 Rust／Go，验证已安装二进制的版本、实际本地 HTTP 上传与进度。数据保留验证使用隔离样本，不替换用户当前安装，也不修改用户配置目录。GUI 安装测试验证应用结构和随附 CLI；原生窗口测试另列如下。

## 原生交互证据与待验收项

| 场景 | 状态与证据 |
| --- | --- |
| 手动导入与上传 | 通过。粘贴链接先成为 Ready，点击上传才发送请求 |
| 关闭窗口后继续上传 | 通过。15 秒延迟 HTTP 上传期间关闭，窗口隐藏、CLI 子进程继续；队列随后落盘 Done |
| 重新显示与退出 | 应用菜单「打开窗口」、⌘W 隐藏、隐藏后 ⌘Q 退出通过；队列保留。隐藏后空闲进程观察为 0% CPU |
| 快捷批次 | 通过应用菜单入口完成两批剪贴板链接上传并自动复制；原有手动 Ready 项保持 Ready |
| 暂停／继续 | 通过应用菜单暂停，子进程回收、记录 Paused；继续后 loopback 返回成功并落盘 Done |
| 缺少默认源 | 通过。隔离空配置导入 1 项，保留 Ready 并打开存储设置 |
| 快捷键冲突 | 两个 img 测试实例的同组合键冲突提示通过；操作系统返回的注册错误会显示。第三方冲突见下述限制 |
| 实际菜单栏项目点击 | **未完成**。已创建 AppKit 状态项，但本次自动化未暴露其可操作入口；应用菜单共用动作的验证不能替代状态项点击 |
| 物理全局快捷键 | **未完成**。已验证主线程注册与按键状态门控；自动化发送的窗口按键未触发 Carbon 全局事件，仍需真实键盘操作 |
| 截图取消 | **未完成**。系统截图覆盖层无法由本次自动化可靠操作，已清理测试子进程；不能把强制终止当作 Esc 取消通过 |
| 通知授权／拒绝／点击 | **未完成**。已接入每批一次 GPUI 通知与点击回到对应队列；系统权限及通知中心交互仍需手工验证 |

macOS 的非独占 Carbon 热键接口不能完整查询其他应用占用。img 使用上游 `global-hotkey` 的非独占注册，报告系统错误，并用本地锁避免 img 实例之间重复注册；没有用独占注册抢占其他应用。**“识别任意第三方冲突”尚未达到计划要求**，应更换组合键或使用菜单入口。

原生补验步骤：在隔离配置中启动应用，使用菜单栏重新打开窗口；在其他应用前台按两组快捷键并长按；选择截图后按 Esc，确认队列数不变；分别允许与拒绝系统通知，确认完成状态可见；允许通知后点击完成通知，确认回到相应队列。不得使用真实云凭据替代 loopback 验收。

## 性能测量

使用同一台 Apple silicon 的 release 构建，生成 1,000／10,000 个不同记录 ID 和图片路径，每张图为 1200 × 900 PNG。记录 ID 独立，图片内容相同，不代表真实混合图库。每组运行 60 次不同搜索、180 次滚动步骤，跳过前 30 步后统计 GPUI 帧直方图。脚本要求实际帧流预热，测量至少 30 帧；零帧或窗口未有效绘制的运行判为无效。

对照模式在同一版框架和布局中重放完整记录克隆、完整元素构建及较大预览图，**不是旧版本二进制的直接对照**。`draw_p95_ms` 是 GPUI CPU 绘制流程耗时，不能代表 GPU 完成时间，也不能单凭它声明端到端 60 FPS。

最终复测在没有并行构建或安装测试的情况下进行；每个窗口均通过原生点击激活，确认有实际帧流。自动启动但没有收到足够帧的尝试已拒绝，不计入下表。

| 记录数与模式 | 搜索 P95 | GPUI draw P95 | dirty-to-present P95 | 有效 draw 样本 | 解码缓存峰值 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1,000，虚拟列表 | 0.004416 ms | 3.631103 ms | 9.748479 ms | 298 | 64 MiB |
| 10,000，虚拟列表 | 0.039167 ms | 3.651583 ms | 9.912319 ms | 299 | 64 MiB |
| 1,000，完整渲染对照 | 0.181500 ms | 103.743487 ms | 103.809023 ms | 149 | 未启用 LRU |

两组优化后采样满足搜索小于 100 ms、CPU draw P95 不超过 16.7 ms 和列表缓存不超过 64 MiB 的脚本检查。缓存预算指应用持有的列表解码缓存与待解码预留，不是整个进程或所有 GPU 资源的内存上限。

10,000 条完整渲染对照曾超过 90 秒，未得到有效 P95，因此这一对照仍未完成，脚本默认跳过，仅 `--heavy-baseline` 启用。完整端到端滚动帧耗时的目标尚未验收。

## 复现命令

```sh
RUSTUP_TOOLCHAIN=stable cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=stable cargo test --locked --workspace
RUSTUP_TOOLCHAIN=stable cargo clippy --locked --workspace --all-targets --features perf -- -D warnings

RUSTUP_TOOLCHAIN=stable GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main ./scripts/package-cli.sh
RUSTUP_TOOLCHAIN=stable GITHUB_REF_TYPE=branch GITHUB_REF_NAME=main ./desktop/package.sh --unsigned
python3 scripts/test-installers.py

RUSTUP_TOOLCHAIN=stable cargo build --locked --release -p img-desktop --features perf
python3 scripts/benchmark-desktop.py
# 10,000 条完整渲染对照会显著占用内存，单独按需运行：
# python3 scripts/benchmark-desktop.py --heavy-baseline
```

基准测试会创建独立的 `Benchmark.app`，打开并自动退出测试窗口，应保持其可见。若窗口未激活，在预热阶段点击窗口；没有帧流会失败。可以用 `--records 1000 --optimized-only`、`--records 10000 --optimized-only` 或 `--records 1000 --baseline-only` 分别测量。夹具只写入 `target/stability-perf`，禁用自动更新与全局快捷键，不执行上传。结果含 release 二进制 SHA-256，失败采样不能作为通过证据。本次测量二进制为 `3548b4eeb233bfd8ad93c2ea3456d1b851eae3e749ac3dc7f496b6a7d7d47e47`。

本地日志：`target/stability-final-tests.log`、`target/stability-final-clippy.log`、`target/stability-installers.log`、`target/stability-actionlint.log`、`target/stability-perf/*.json`。这些生成文件不纳入 Git。

## 交付及平台边界

- 独立 CLI：`dist/cli/aarch64-apple-darwin/img_darwin_arm64.tar.gz`。
- GUI：`dist/desktop/arm64/img-desktop_0.3.0_macos_arm64.dmg` 和同名 ZIP，内置 `img 0.3.0` CLI。
- 包目录中包含 SHA-256 和 `build-info.json`；GUI 为 `channel: development`、`notarized: false`，仅有 ad-hoc 签名。
- 普通 PR／分支 CI 已覆盖 CLI 的 macOS／Linux／Windows，以及 GUI Apple silicon／Intel。**本轮没有推送或执行远端 CI，Linux、Windows、Intel 构建安装验收均未完成。**
- 实际物理快捷键、截图取消、通知交互、状态栏点击、10,000 条完整渲染对照和端到端滚动帧耗时仍需补验。正式签名／公证待证书具备；自动升级、远端图库、标签、深色主题及文件夹导入不属于本轮。

代码实现和本机自动化检查不能替代上述未完成验收。当前交付定位为可供本地继续验收的内测候选版。
