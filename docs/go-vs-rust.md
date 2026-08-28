# Go vs Rust：本项目选型对比

> 结论先行：本项目（macOS + Windows 桌面版、WHOIS/RDAP 查询）最终选择 **Rust + Tauri 2**。

## 项目真实负载

WHOIS 查询应用的核心工作：发网络请求（RDAP 走 HTTPS、传统 WHOIS 走 TCP 43）、解析混乱的注册局文本、按 TLD 路由、对外提供结构化结果。

瓶颈在**网络 I/O 与解析逻辑**，不在 CPU。因此两种语言在"性能"上对这个项目没有实质差别，真正的差异在开发体验、生态，以及最重要的——**客户端覆盖能力**。

## 逐项对比

| 维度 | Go | Rust |
|---|---|---|
| 并发 | goroutine + channel，随手 `go func()` | async/await + tokio，需要理解 Future / Send / 生命周期 |
| 开发速度 | 快，编译秒级 | 慢，borrow checker 约束多，编译时间长 |
| 错误处理 | `if err != nil`，直白但啰嗦 | `Result` + `?`，优雅但要求更严谨 |
| WHOIS 脏文本解析 | 宽松，适合快速处理脏数据 | 需要更多缺省分支设计，代码更"用力" |
| 生态 | 有成熟的 whois 库（如 likexian/whois） | whois 库偏早期，本项目直接手写 socket 客户端（约 60 行） |
| 部署 | 单二进制，交叉编译容易 | 单二进制；完全静态需 musl 目标 |
| 桌面客户端 | Wails（Go + WebView），成熟可用 | Tauri 2，更活跃、包体积更小（本应用 .app 约 20 MB） |
| 移动端 | 无成熟 UI 方案 | Tauri 2 支持 iOS/Android（本项目未用到） |
| 学习成本 | 低 | 高 |

## 关键差异：客户端

需求是 macOS + Windows 桌面版。两条路线：

- **Go 路线**：后端写 Go 没问题，桌面端用 Wails 也可以，但语言生态无法延伸到 Windows 原生 UI，最终仍要引入 Web 前端技术栈。
- **Rust 路线**：Tauri 2 让 Rust 直接承载桌面端（系统 WebView + Rust 内核），**后端逻辑与客户端同一语言**，一份代码覆盖 macOS / Windows。

考虑到本机环境只有 Rust，Rust 路线是唯一能一条线走到底的选择。

## 具体代码差异（同样一个查询）

Go 风格：goroutine 并发，错误手动传播，代码直来直去。

Rust 风格：async + tokio，类型系统在编译期强制处理所有错误分支与并发边界；代价是开发节奏更慢，收益是长期项目的下限更高。

## 最终决策

- 只看后端 API：**Go 更省事**。
- 结合"桌面客户端 + 单一语言生态 + 本机只有 Rust"：**Rust + Tauri 2**。
- 本项目落地：Rust（reqwest + tokio）直接内置在 Tauri 应用内查询，无需独立后端服务。

