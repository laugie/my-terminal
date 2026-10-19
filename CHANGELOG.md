# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。

## [Unreleased]
### Added
- 项目计划 docs/PLAN.md、README、CHANGELOG 骨架。
- Tauri 2 项目骨架，以及 macOS（arm64 .dmg）/ Windows（NSIS .exe）构建 CI。
- 服务器列表：解析 `tailscale status --json`（机器名、IP、在线状态），并支持在内存中手动添加服务器。
- 本地 JSON 配置：手动服务器写入 `config.json`（macOS `~/Library/Application Support/my-terminal/`，Windows `%APPDATA%\my-terminal\`）。文件损坏时备份并恢复默认值；版本更高时只读加载；不保存密码或密钥。
- 单个 PTY 会话：点击服务器后用系统 ssh 连接，在 xterm.js 中显示；同一时间只保留一个会话。
- 多标签终端：同一窗口最多 8 个会话，可新建、切换、关闭；关闭标签会结束对应进程。支持复制粘贴，会话结束后保留标签并可重新连接。
- 粘贴和按键输入按最多 64 KiB、沿 UTF-8 字符边界分块，并按标签串行写入。界面上已结束的标签也计入 8 个上限。CI 在 macOS 与 Windows 上运行前端单元测试。
- PTY 在发出退出事件前先排空读线程，避免 Windows 上 ssh 的尾部输出被退出事件盖过。

## [0.1.0] - 未发布
## [0.2.0] - 未发布
## [0.3.0] - 未发布
## [0.4.0] - 未发布
## [1.0.0] - 未发布
