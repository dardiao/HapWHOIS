# HapWHOIS

类 whoisthisdomain 的**域名注册信息查询桌面应用**，同时支持 macOS 与 Windows。

- RDAP 优先（主流 TLD 返回结构化 JSON）
- 传统 WHOIS（TCP 43 端口）兜底，展示原始输出
- 显示注册商、注册/到期/更新时间、域名状态、Name Server
- 批量查询：每行一个域名，自动去重、并发限流、逐域名结果
- 按后缀自动路由 WHOIS 服务器：内置表优先，可开启
  `{后缀}.whois-servers.net` DNS 发现作为兜底（whoisthisdomain 同款机制）

## 技术栈

| 层 | 技术 |
|---|---|
| 客户端框架 | Tauri 2（Rust 内核 + 系统 WebView） |
| 前端 | React 18 + Vite 6（纯 JSX，无额外构建依赖） |
| 网络层 | Rust：reqwest（RDAP / HTTPS）+ tokio（WHOIS TCP 43） |

> 语言选型的详细对比见 [docs/go-vs-rust.md](docs/go-vs-rust.md)。

## 目录结构

```
HapWHOIS/
├── src/                  # React 前端（搜索框 + 结果展示）
├── src-tauri/
│   ├── src/lib.rs        # Tauri 命令入口（lookup）
│   ├── src/rdap.rs       # RDAP 查询与解析
│   ├── src/whois.rs      # 传统 WHOIS 查询（按 TLD 路由）
│   └── tauri.conf.json   # 窗口 / 打包配置
├── scripts/make-icon.mjs # 图标生成脚本（纯 Node，无依赖）
└── .github/workflows/    # 跨平台打包 CI
```

## 在 macOS 上运行

```bash
npm install
npm run tauri dev        # 开发模式（热更新）
```

打包：

```bash
npm run tauri build                    # 发布版
npm run tauri build -- --debug         # 调试版（更快）
```

产物在 `src-tauri/target/release/bundle/macos/`（.app 与 .dmg）。

## 在 Windows 上构建

macOS 无法直接交叉编译 Windows 安装包，需要在 Windows 机器上执行（或推送 tag 触发 [CI](.github/workflows/build.yml)）：

1. 安装 [Rust](https://rustup.rs)（MSVC 工具链）与 Node.js 18+
2. 系统需带 WebView2 运行时（Win10/11 默认已带）
3. 执行同样命令：

```powershell
npm install
npm run tauri build
```

产物在 `src-tauri\target\release\bundle\nsis\`（.exe 安装包）。

## 测试

```bash
cd src-tauri
cargo test              # 需要联网，真实请求 RDAP / WHOIS
```

## 已知限制

- 传统 WHOIS 目前内置了常见 TLD（.com/.net/.org/.io 等）的服务器路由表；其余 TLD 依赖 RDAP（rdap.org 已覆盖绝大多数主流顶级域）。
- 未做查询缓存与域名监控，可作为后续扩展。
