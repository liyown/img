## img CLI 0.4.0

图床管理、图片处理与文章维护，共用桌面版的存储配置和 SQLite 图库。

- `process --recipe … --output-dir …`：共享处理配方与可恢复任务，保留原有单文件参数。
- `library`：索引已选范围，查询、预览、下载、检查和管理远端图片；缓存独立管理。
- `sync`：通过自己的 WebDAV 或 S3 同步配置、索引和预设，查看并解决冲突。同步目录中的图床凭据为明文，缓存不传输。
- `migrate`：创建和继续执行跨图床复制任务，回读验证后生成链接映射，默认保留源端图片。
- `references`：扫描 Markdown 行内、引用式与 HTML 图片，逐文件预览，备份后替换，导出报告与恢复。
- Agent Skill 扩展图片处理、图库查询、同步状态与迁移预览；远端删除及文章写入要求明确执行请求。

提供 macOS ARM64 / x86_64、Linux ARM64 / x86_64、Windows x86_64 安装产物。桌面安装包已内置 CLI，无需重复安装。

[安装与更新](https://liyown.github.io/img/install/#cli) · [0.4 使用指南](https://liyown.github.io/img/docs/0.4/) · [English guide](https://liyown.github.io/img/en/docs/0.4/)

### English

img 0.4 adds a shared library, reusable processing recipes, sync through your own WebDAV or S3 storage, recoverable cross-provider copies, and safe Markdown reference repair. Desktop and CLI use the same storage configuration and catalog. Existing upload and single-file process commands remain compatible.

Sync transfers configuration, indexes, and presets, including plaintext provider credentials, but never image caches. Migration keeps source images by default. Reference changes require explicit execution and successful backups.
