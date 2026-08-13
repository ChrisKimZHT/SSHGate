# SSHGate

SSHGate 是一个轻量级 Tauri 2 桌面应用：把远端服务器的 Web 服务通过内嵌 SSH 会话映射到本机，并用 `*.localhost` 域名访问；同一 SSH 连接也承载内嵌终端。

## MVP 功能

- SSH 私钥或临时密码登录（密码不落盘）
- 首次连接记录 SSH 主机密钥指纹，后续连接校验
- 同一服务器的 Web channel 和 Terminal channel 复用一个 `russh` session
- Rust 内置 HTTP/1.1 透明反向代理，支持流式响应和 WebSocket
- `jupyter.gpu.localhost` 等多级 `.localhost` 域名，无需修改 hosts/DNS
- xterm.js PTY、ANSI、UTF-8、Ctrl+C、Vim/tmux/top 和 window-change
- 多终端标签、服务启停、私钥连接自动重连
- JSON 配置持久化及 `~/.ssh/config` 基础导入

## 开发环境

需要 Node.js 20+、Rust 1.85+，以及 [Tauri 2 对应的平台依赖](https://v2.tauri.app/start/prerequisites/)。Windows 需要 WebView2（Windows 10/11 通常已包含）和 Microsoft C++ Build Tools。

```bash
npm install
npm run tauri dev
```

如果刚安装 Rust/Build Tools 的 Windows 终端尚未刷新环境变量，可使用项目附带的包装脚本：

```powershell
.\scripts\tauri-msvc.cmd dev
```

仅检查前端：

```bash
npm run build
```

检查 Rust：

```bash
cd src-tauri
cargo test
cargo check
```

## 使用

1. 添加服务器，选择私钥路径或密码认证。
2. 连接服务器。首次连接会信任并保存服务器公钥指纹；指纹发生变化时连接会被拒绝。
3. 添加 Web 服务，例如远端 `127.0.0.1:8888`，域名 `jupyter.gpu.localhost`。
4. 启动服务，再用 Open 按钮访问。

本地代理默认绑定 `127.0.0.1:80`。若端口 80 已被占用，设置页会显示错误；可释放端口后保存设置重试，或改用其他端口（此时 URL 会自动带端口）。部分 Linux 环境限制普通用户绑定 1024 以下端口，需要为二进制授予 `CAP_NET_BIND_SERVICE` 或将系统的非特权端口下限调低。

## 配置与安全

配置保存在 Tauri 的应用配置目录 `app.sshgate.desktop/config.json`。文件只记录私钥路径，不记录私钥内容或 SSH 密码。当前 MVP 不支持加密私钥的 passphrase、SSH agent、ProxyJump 和 HTTPS 本地入口。
