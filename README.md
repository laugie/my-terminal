# my-terminal

自用的轻量 SSH 客户端（Termius 替代），支持 macOS 和 Windows。基于 Tauri 2 + xterm.js，调用系统自带 ssh，配合 Tailscale SSH 点击即连。

- 版本计划：[docs/PLAN.md](docs/PLAN.md)
- 变更记录：[CHANGELOG.md](CHANGELOG.md)

当前状态：v0.1 骨架（Tauri 2 + Vite + 原生 TypeScript）。本地预览：`npm ci && npm run tauri dev`。开发由 AI 工程团队以草稿 PR 方式进行，合并需仓库所有者授权。

## 拓扑首页

首页按区域显示本机 `topology.json`。它和 `config.json` 在同一目录：

- macOS：`~/Library/Application Support/my-terminal/topology.json`
- Windows：`%APPDATA%\my-terminal\topology.json`

仓库里的 [topology.example.json](topology.example.json) 只含占位地址。复制到上面的路径后，点「重新加载」。文件缺失或格式不对时，首页会写出具体原因。原来的 Tailscale 列表和手动添加仍在「手动服务器」。

点击一台设备时，界面只把设备名交给后端。主机、用户、端口、密钥路径和跳板都由后端按文件拼出来，再调用系统 ssh。跳板机如果写了密钥路径，会生成一份临时 ssh 配置：Unix 上权限是 `0600`；Windows 上用 `icacls` 去掉继承，并尽量只留给当前用户。ssh 通过 `-F` 读取这份文件，文件里用 `Match all` 后的 `Include ~/.ssh/config` 引入用户自己的配置。会话结束或退出应用时删除临时文件。配了 `tailscaleAddress` 的设备会对照 Tailscale 状态显示在线、离线或「未知」；连上去用的仍是 `host`。
