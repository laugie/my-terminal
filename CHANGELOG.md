# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。

## [Unreleased]
### Changed
- 启动默认进入拓扑首页。登录锁改为顶栏设置里的开关，默认关闭，记在本机 `config.json` 的 `settings.loginLockEnabled`。旧配置没有这个字段时视为关闭。打开后从下次启动起要求登录；还没设过口令时会立即进入设置，成功后才保存。关闭立即生效，已设过口令时须先验证，失败与登录共用等待时间和同一把尝试锁。哈希仍留在系统凭据库。配置损坏重置后开关为关闭并提示；配置来自更新版本时开关只读。

### Fixed
- macOS 从 Finder 启动时，Tailscale 状态不再因 App 包内二进制进入 GUI 模式而解析失败。查找 CLI 时在 PATH 之后检查 `/usr/local/bin`、`/opt/homebrew/bin` 和 App 包内路径；调用时直接 spawn 并设置 `TERM=dumb`。状态 JSON 容忍 null 与单个坏节点，失败原因显示在主机列表上方。

### Added
- 启动登录锁：首次设置用户名和口令，只把 argon2id 哈希放进系统凭据库的一条记录（service 为 `my-terminal`）。之后每次启动登录。连续失败 5 次后等待 30 秒，再错则翻倍，最长 15 分钟；重启不清零，时钟回拨不会缩短剩余时间。凭据库不可用时拒绝进入。解锁前不加载拓扑和服务器列表，也不能开会话。
- 拓扑首页：按区域显示本机 `topology.json`，移动设备带标签；文件缺失、格式错误、重名或 jump 成环时给出具体提示，并提供「重新加载」。点击只提交设备名。跳板机写了密钥路径时生成临时 ssh 配置（Unix `0600`；Windows 用 `icacls` 去掉继承并尽量只留给当前用户），以 `-F` 调用且 `Include ~/.ssh/config`，会话结束或退出时删除。`tailscaleAddress` 对照现有 Tailscale 状态显示在线、离线或未知。原来的服务器列表保留在「手动服务器」。
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
