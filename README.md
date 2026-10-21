# my-terminal

自用的轻量 SSH 客户端（Termius 替代），支持 macOS 和 Windows。基于 Tauri 2 + xterm.js，调用系统自带 ssh，配合 Tailscale SSH 点击即连。

- 版本计划：[docs/PLAN.md](docs/PLAN.md)
- 变更记录：[CHANGELOG.md](CHANGELOG.md)

当前状态：v0.1 骨架（Tauri 2 + Vite + 原生 TypeScript）。本地预览：`npm ci && npm run tauri dev`。开发由 AI 工程团队以草稿 PR 方式进行，合并需仓库所有者授权。

## 登录锁

启动默认进入拓扑首页，不要求登录。顶栏「重新加载」右边的齿轮可以打开设置，里面只有「启动时需要登录」。这个开关默认关闭，写在本机 `config.json` 的 `settings.loginLockEnabled`（没有这个字段的旧配置也视为关闭）。配置版本号不变。

打开开关后，从下次启动起才要求登录，当前这次仍可继续使用。如果还没设过口令，打开开关会马上进入设置：用户名 1 到 64 个字符，区分大小写；口令要输入两次，至少 8 个字符，没有别的复杂度要求。设置成功才把开关写成打开。已经有口令时，打开开关不会再问一次，下次启动用原来的口令登录。

关闭开关立即生效。还没设过口令时可以直接关。已经设过口令时，要先输入用户名和口令；输错计入下面的等待，和登录共用同一次数。关掉开关不会删除凭据库里的哈希。

口令只以 argon2id 哈希（随机 16 字节盐，PHC 字符串）和用户名一起放在系统凭据库的一条记录里，service 名是 `my-terminal`。macOS 在「钥匙串访问」，Windows 在「凭据管理器」。应用不把口令写进配置文件、日志或浏览器的 localStorage。开关打开时，每次启动都要重新登录。用户名或口令不对时，提示都是「用户名或密码错误」。配置文件损坏并被重置时，开关回到关闭，并在设置里提示。配置来自更新版本时只读加载，开关不能改。

连续输错 5 次后需要等待 30 秒才能再试，之后每再错一次，等待时间翻倍，最长 15 分钟。等待没有结束时不能提交，界面显示剩余时间。退出再打开，等待和失败次数还在。把系统时间往回拨，不会让剩余等待变短。

应用里没有「忘记口令」或重置按钮。如果真的忘了：在 macOS「钥匙串访问」或 Windows「凭据管理器」里删掉 service 为 `my-terminal` 的那一条，再打开应用，会重新出现首次设置。这道锁只防旁人打开应用。能操作本机账户的人（包括本机管理员）可以删掉这条凭据、改系统时间，或者调试进程，锁防不住他们。凭据库拒绝访问或条目损坏时，应用不会放行，也不会退回首次设置。

## 拓扑首页

首页按区域显示本机 `topology.json`。它和 `config.json` 在同一目录：

- macOS：`~/Library/Application Support/my-terminal/topology.json`
- Windows：`%APPDATA%\my-terminal\topology.json`

仓库里的 [topology.example.json](topology.example.json) 只含占位地址。复制到上面的路径后，点「重新加载」。文件缺失或格式不对时，首页会写出具体原因。原来的 Tailscale 列表和手动添加仍在「手动服务器」。

点击一台设备时，界面只把设备名交给后端。主机、用户、端口、密钥路径和跳板都由后端按文件拼出来，再调用系统 ssh。跳板机如果写了密钥路径，会生成一份临时 ssh 配置：Unix 上权限是 `0600`；Windows 上用 `%SystemRoot%\System32\icacls.exe` 去掉继承，并尽量只留给当前用户。跳板机配置了密钥路径时会用 `-F`，此时不会读取 `/etc/ssh/ssh_config`（包括 macOS 的 `ssh_config.d`）。用户自己的 `~/.ssh/config` 通过 `Match all` 之后的 `Include ~/.ssh/config` 保留。会话结束或退出应用时删除临时文件。配了 `tailscaleAddress` 的设备会对照 Tailscale 状态显示在线、离线或「未知」；连上去用的仍是 `host`。
