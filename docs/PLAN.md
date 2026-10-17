# my-terminal 项目计划

## 目标
做一个自用、免费的 Termius 替代品，给 macOS 和 Windows 用：打开即见服务器列表，点一下就用系统 ssh 连上；配合 Tailscale SSH，不用输用户名和密码。

## 技术选型和理由
- **Tauri 2**：一套代码打包 macOS / Windows，安装包只有几 MB，比 Electron 轻；后端 Rust 负责进程和文件。
- **xterm.js**：成熟的网页终端组件（VS Code 同款），支持多标签、主题、复制粘贴。
- **系统自带 ssh + PTY（portable-pty）**：macOS 自带 ssh，Windows 10/11 自带 OpenSSH，不用自己实现 SSH 协议，直接沿用 ~/.ssh/config 和密钥。
- **Tailscale SSH**：登录验证交给 tailnet 身份，访问规则在 Tailscale 后台管理，点击即连。
- **本地 JSON 配置**：简单、可读、便于后续同步。

## 架构
- 前端（TypeScript + xterm.js）：服务器列表、标签页、设置界面。
- Tauri 后端（Rust）：
  - `hosts`：运行 `tailscale status --json` 解析机器（名字、IP、在线状态），与手动添加的服务器合并；
  - `session`：用 PTY 启动 `ssh <目标>`，通过 Tauri 事件双向传输数据、处理窗口大小变化；
  - `config`：读写本地 JSON（macOS `~/Library/Application Support/my-terminal/`，Windows `%APPDATA%\my-terminal\`）。
- 不保存任何密码；认证依赖 Tailscale SSH 或用户自己的 SSH 密钥。

## 版本路线图

### v0.1 MVP
- 服务器列表：读取 `tailscale status --json`，并支持手动添加（名称、主机、用户、端口）。
- 点击用系统 ssh 连接。
- 多标签 xterm 终端。
- 本地 JSON 配置。

验收标准：
- macOS 和 Windows 的 CI 都能构建出安装包；
- 装了 Tailscale 时列表显示 tailnet 机器和在线状态，没装时不崩溃并给出提示；
- 手动添加的服务器重启后仍在；
- 点击开启了 Tailscale SSH 的服务器能直接进入 shell，不弹密码；
- 可同时开至少 3 个标签，关闭标签会结束对应 ssh 进程；改窗口大小后 vim/top 显示正常。

### v0.2
- 分组、搜索、主题和字体设置、快捷键。

验收标准：服务器可建分组并拖入/移出；搜索按名称/IP 实时过滤；可切换至少 2 套配色、改字体和字号并持久保存；新建/关闭/切换标签与搜索有快捷键（macOS 用 Cmd，Windows 用 Ctrl）。

### v0.3
- SFTP 文件传输、端口转发。

验收标准：可浏览远端目录，上传/下载文件并显示进度；可配置本地端口转发（-L），开关后端口确实可用/释放；失败时有明确报错。

### v0.4
- 多设备配置同步：用户可选 Git 仓库，或 iCloud / OneDrive 目录。

验收标准：两台机器指向同一同步源后，增删改服务器能互相看到；冲突时不静默丢数据（保留两份或提示）；同步内容不含任何密钥或密码。

### v1.0
- 自动更新和代码签名。

验收标准：macOS 包签名并公证、Windows 包签名，安装时不出现"未知开发者"类拦截；应用内检测到新版本可一键更新；发布流程写入文档。

## 迭代规则
1. 每个版本拆成若干 GitHub issue，打 `agent-ready` 标签。
2. 由 AI 工程团队按流程处理：Triage 分诊 → Builder 实现（分支 `agent/my-terminal-<号>-<简述>`）→ Reviewer 评审 → 开草稿 PR。
3. PR 正文第一行写 `Closes #N`；不直接推 main，不部署。
4. 每个 PR 合并都需要用户单独授权；合并后确认 main 上 CI 通过。
5. 每个版本完成后更新 CHANGELOG.md 并打 tag（如 `v0.1.0`），再按需调整本计划。
