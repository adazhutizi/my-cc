# my-cc

Rust 终端代理程序。通过 PTY 代理运行任意终端程序（如 Claude Code、vim、htop），让本地终端和远程 Web 客户端实时同步操作。

## 功能

- **本地 + Web 双端同步** — 任一端操作，另一端实时可见
- **多客户端** — 支持多个浏览器同时连接
- **输出回放** — 新客户端连接时通过 PTY 原始输出日志恢复画面
- **断点续传** — 重连时从上次接收位置继续，避免重复回放（如 iOS 后台切换场景）
- **窗口自适应** — 本地或 Web 端 resize 自动同步
- **等比缩放** — Web 端按实际浏览器宽度缩放终端画面，桌面和移动端一致
- **移动端快捷键** — Web 端在移动设备显示 Esc、Tab、Shift+Tab、方向键、`/` 等常用操作按钮
- **软键盘避让** — 移动端输入法弹出时，底部状态栏和快捷键栏自动上移，避免被遮挡
- **Token 认证** — 可选的 WebSocket 连接认证
- **自动重连** — Web 客户端断线后指数退避重试
![实机演示](docs/imgs/demo.png)
## 快速开始

```bash
# 构建
cargo build --release

# 代理运行 shell（默认命令）
my-cc

# 代理运行 Claude Code
my-cc claude

# 指定端口和认证
my-cc --port 9090 --token mysecret

# 带参数运行
my-cc -- vim /path/to/file
```

浏览器打开 `http://localhost:8080` 即可看到同步的终端画面。

使用 token 时需带上查询参数：`http://localhost:8080?token=mysecret`

## CLI 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `command` | `$SHELL`（Windows 为 `powershell`） | 要代理运行的命令 |
| `args...` | 无 | 命令参数（`--` 后传入） |
| `--port` | `8080` | Web 服务端口 |
| `--token` | 无 | WebSocket 连接认证 token |

## 架构

```
                    ┌──────────────────┐
                    │   my-cc (Rust)   │
                    │                  │
                    │  ┌─── PTY ────┐  │
                    │  │  Reader    │  │
                    │  │  Writer    │  │
                    │  └─────┬──────┘  │
                    │        │         │
                    │  ┌─ Event Bus ─┐  │
                    │  │ broadcast   │  │
                    │  │   output    │  │
                    │  │   resize    │  │
                    │  │ mpsc input  │  │
                    │  └──┬───┬───┬──┘  │
                    │     │   │   │     │
                    │  ┌──┘     ┌─┘┐   │
                    │  │        │  │   │
                    │  本地     Web │
                    │  终端     服务│
                    └──────────────────┘
                         ↕        ↕
                    本地终端    Web 客户端
```

- **PTY Manager** — `portable-pty` 创建伪终端，独立线程处理阻塞 I/O；Windows 上非原生可执行文件（`.ps1`、`.cmd`、`.bat`）通过 `powershell -NoLogo -Command` 包装执行，使命令解析遵循 PowerShell 的优先级
- **Event Bus** — tokio channels：broadcast 分发输出，mpsc 合并输入，并保留最近 PTY 输出用于 Web 端回放
- **Network Server** — hyper 提供 HTTP 静态页面 + hyper-tungstenite 处理 WebSocket
- **Web Frontend** — xterm.js 渲染终端，二进制 WebSocket 协议通信；保持服务端 PTY 行列数不变，按浏览器可用宽度等比缩放显示，并在移动端提供快捷操作按钮
- **Terminal** — 跨平台终端控制：Unix 通过 SIGWINCH 监听窗口变化；Windows 通过轮询检测窗口变化，并启用 VT input 保留 Esc、Shift+Tab 等终端按键序列

### 二进制 WebSocket 协议

服务端与 Web 客户端之间使用二进制协议通信，格式为 `[1字节类型][payload]`：

| 类型 | 值 | 方向 | Payload | 说明 |
|------|----|------|---------|------|
| Output | `0x01` | S→C | `[seq u64 BE][data]` | PTY 输出，携带序列号用于断点续传 |
| Input | `0x02` | C→S | `[data]` | 用户输入 |
| Resize | `0x03` | 双向 | `[rows u16 BE][cols u16 BE]` | 窗口大小变更 |
| Mouse | `0x05` | C→S | — | 鼠标事件（保留） |
| FeatureToggle | `0x06` | C→S | — | 功能切换（保留） |
| Ping | `0x07` | C→S | — | 心跳 |
| Pong | `0x08` | S→C | — | 心跳回复 |
| ReplayEnd | `0x09` | S→C | — | 回放结束，客户端可恢复输入 |
| ReplayMode | `0x0A` | S→C | `[mode u8]` | 回放模式：`0`=增量续传，`1`=全量回放 |

### Web 端恢复与缩放

新 Web 客户端连接时，服务端会先发送当前 PTY 尺寸，再回放最近的原始 PTY 输出日志。浏览器端由 xterm.js 解析同一份终端输出流，减少字符残影或布局偏差。

**断点续传**：客户端维护已接收的最后一条输出序列号（seq），重连时通过 URL 参数 `lastSeq` 告知服务端。服务端只回放 seq 之后的新数据，避免全量重复。当缓存日志已被淘汰（无法从该 seq 继续）时，自动降级为全量回放。这一机制对 iOS 后台切换、网络短暂中断等场景特别有效。

Web 端不会根据浏览器宽度改变 PTY 的逻辑列数，而是保持服务端 PTY 的 `rows/cols`，再对 xterm 的实际渲染区域做 CSS 等比缩放：

- 宽度按浏览器可用宽度贴合，可放大也可缩小
- 高度超出时由终端容器滚动
- 桌面端和移动端使用同一套缩放逻辑

### 移动端操作

移动端 Web 客户端会在终端下方显示快捷键栏，用于弥补软键盘缺少终端控制键的问题。当前包含：

- `Esc`、`Tab`、`Shift+Tab`
- `Ctrl+C`、`Ctrl+D`
- `Enter`、`Backspace`
- 上下左右方向键
- `/`，用于快速调用支持斜杠指令的程序

这些按钮不会改变后端协议，本质上仍通过 WebSocket 向 PTY 发送普通输入字节或终端控制序列。输入法弹出时，前端会根据浏览器可视视口调整底部间距，让快捷键栏和状态栏保持在软键盘上方。

## 项目结构

```
src/
├── main.rs        # CLI 入口，组件串联
├── lib.rs         # 模块重导出，供集成测试使用
├── protocol.rs    # 二进制消息协议编解码
├── bus.rs         # 事件总线
├── pty.rs         # PTY 管理
├── terminal/      # 本地终端控制
│   ├── mod.rs     # raw mode、本地终端转发
│   ├── unix.rs    # SIGWINCH 监听窗口变化
│   └── windows.rs # 轮询检测窗口变化，启用 Windows VT input
└── server.rs      # HTTP + WebSocket 服务
static/
└── index.html     # xterm.js 前端（编译时嵌入）
tests/
├── test_protocol.rs
└── test_bus.rs
```

## 开发

```bash
# 只运行测试
cargo test

# 开发模式运行
cargo run -- /bin/sh

# 查看日志
RUST_LOG=debug cargo run -- claude
```

## 依赖

- [tokio](https://tokio.rs) — 异步运行时
- [portable-pty](https://docs.rs/portable-pty) — 跨平台 PTY
- [hyper](https://hyper.rs) + [hyper-tungstenite](https://docs.rs/hyper-tungstenite) — HTTP/WebSocket 服务
- [xterm.js](https://xtermjs.org) — 浏览器终端渲染
- [clap](https://docs.rs/clap) — CLI 参数解析
- [windows-sys](https://docs.rs/windows-sys) — Windows 控制台 VT input 模式

## License

MIT
