# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 构建与测试

```bash
cargo build                          # 调试构建
cargo build --release                # 发布构建
cargo test                           # 运行所有测试
cargo test test_bus                  # 运行某个测试模块
cargo test --test test_protocol      # 运行某个测试文件
cargo clippy                         # 代码检查
cargo run -- bash                    # 代理 shell
cargo run -- -- vim /some/file       # 代理 vim（用 -- 分隔 clap 参数和命令参数）
RUST_LOG=debug cargo run -- bash     # 开启调试日志运行
```

## CLI 与运行方式

- 默认命令：Unix 使用 `$SHELL`，没有时回退 `/bin/sh`；Windows 使用 `powershell`
- `command` 是要代理运行的程序；命令自己的参数必须放在 `--` 后，避免被 clap 当作 `my-cc` 参数解析
- `--port` 默认 `8080`；端口被占用时服务端会自动尝试下一个端口
- `--token` 只保护 WebSocket 连接，浏览器连接时通过查询参数传入：`?token=...`

## 架构

终端代理：本地终端和远程 Web 客户端通过 WebSocket 同步操作 PTY 子进程（vim、htop、Claude Code 等）。

### 数据流

```
PTY ←→ EventBus（broadcast + mpsc 通道 + 输出回放日志） ←→ { 本地终端, WebSocket 客户端 }
```

- **PTY 输出** → broadcast channel → 本地 stdout 和 WebSocket 客户端；最近的输出会被保留，用于新客户端回放
- **输入**（本地 stdin 或 Web） → mpsc channel → PTY 写入线程
- **窗口调整**（SIGWINCH 或 Web 端） → broadcast channel → `PtyProcess::resize()`
- **新 WebSocket 客户端连接** → 发送当前 PTY 尺寸，根据客户端携带的 `lastSeq` 参数决定回放范围（增量续传或全量回放），发送 `ReplayMode` 告知回放类型，回放数据，发送 `ReplayEnd`，跳过已回放事件后切换到实时转发

### 关键模块

| 模块 | 职责 |
|------|------|
| `src/pty.rs` | PTY 创建、阻塞 I/O 线程、通过 `Arc<Mutex<PtyProcess>>` 调整窗口大小；Windows 上通过 `powershell -NoLogo -Command` 包装命令以支持 `.ps1`/`.cmd`/`.bat` 等非原生可执行文件 |
| `src/bus.rs` | `EventBus` — tokio broadcast（输出、resize）+ mpsc（输入）通道；保留最近输出用于回放；`output_replay_from(last_seq)` 支持断点续传 |
| `src/server.rs` | hyper HTTP + WebSocket 服务器；端口占用时递增重试；Token 认证；前端通过 `include_str!` 嵌入 |
| `src/protocol.rs` | 二进制 WebSocket 协议：`[1字节类型][payload]`。类型：Output/Input/Resize/Mouse/FeatureToggle/Ping/Pong/ReplayEnd/ReplayMode。Output payload 格式为 `[seq u64 BE][data]`，ReplayMode payload 为 1 字节（0=增量，1=全量） |
| `src/terminal/mod.rs` | 本地终端 raw mode、异步 stdin/stdout、本地 resize watcher 入口 |
| `src/terminal/unix.rs` | Unix SIGWINCH 监听窗口变化 |
| `src/terminal/windows.rs` | Windows 轮询检测窗口变化，并启用 VT input 保留 Esc、Shift+Tab 等终端按键序列 |
| `static/index.html` | 内嵌 xterm.js 客户端，二进制 WebSocket 通信，自适应缩放布局，移动端快捷键，自动重连 |

### 并发模型

- PTY reader/writer/exit watcher：`std::thread`，因为 PTY I/O 是阻塞式
- 异步任务：tokio 运行时，负责本地终端转发、HTTP/WS 服务、WebSocket 输入输出处理
- 共享状态：`Arc<PtyProcess>` 内部用 `std::sync::Mutex` 包装 child/killer/master；`Arc<StdMutex<Vec<OutputEvent>>>` 保存输出日志
- resize 监听器运行在 `std::thread` 上，因为 `PtyProcess` 在 async 上下文中不是 `Send`
- 关闭：PTY reader 在子进程退出时发送 `oneshot` 信号；`main.rs` 通过 `tokio::select!` 等待本地终端退出或 PTY 关闭

### Web 客户端行为

- HTTP 非 WebSocket 请求始终返回 `static/index.html`，不提供运行时静态文件目录
- WebSocket 使用二进制协议；客户端只发送 `Input` 和 `Resize`，服务端忽略其他客户端消息类型
- 连接建立后客户端进入 replay 状态；收到 `ReplayMode(全量)` 时 `term.reset()`；收到 `ReplayEnd` 后恢复正常输入转发
- 客户端维护 `lastReceivedSeq`，重连时通过 URL 参数 `lastSeq` 实现断点续传；服务端只回放 seq 之后的数据，日志被淘汰时降级为全量回放
- Web 端保持服务端 PTY 的逻辑 `rows/cols`，用 CSS transform 按浏览器可用宽度等比缩放，不根据浏览器宽度改变 PTY 列数
- 自动重连使用指数退避，最大 30 秒；连接期间每 25 秒发送一次 `Ping`
- 移动端显示快捷键栏：Esc、Tab、Shift+Tab、Ctrl+C、Ctrl+D、Ctrl+O、方向键、修饰键、常用符号和半角/全角切换
- 移动端使用 `visualViewport` 计算软键盘占位，调整底部状态栏和快捷键栏位置

## 项目约定

- UI 文字和文档使用中文
- 无外部配置文件；CLI 参数通过 clap 解析，运行时配置只来自 CLI 和环境变量
- 服务器与 Web 客户端之间使用二进制协议（非 JSON）：`[1字节类型][payload]`
- Resize payload 固定 4 字节：`rows` 和 `cols` 都是 big-endian `u16`
- Output payload 格式为 `[seq u64 BE][data]`，每条输出都携带序列号
- ReplayMode payload 固定 1 字节：`0x00`=增量续传，`0x01`=全量回放
- Web 客户端通过回放最近的 PTY 原始输出恢复画面；服务端不做终端状态重建，也不解析 ANSI 状态
- `src/lib.rs` 将模块以 `pub` 重新导出，供集成测试使用（`tests/` 目录中使用 `use my_cc::...`）
- 前端（`static/index.html`）在编译时通过 `include_str!` 嵌入，无运行时文件服务
- EventBus 输出日志上限 16384 条；broadcast channel 缓冲区 16384
- `EventBus::take_input_receiver()` 只能调用一次，PTY writer 是唯一输入消费者
- 异步测试使用 `#[tokio::test]`；协议测试是同步的
