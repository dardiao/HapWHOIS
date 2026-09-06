# HapWHOIS

- RDAP 优先（主流 TLD 返回结构化 JSON）
- 传统 WHOIS（TCP 43 端口）兜底，展示原始输出
- 显示注册商、注册/到期/更新时间、域名状态、Name Server
- 批量查询：每行一个域名，自动去重、并发限流、逐域名结果
- 按后缀自动路由 WHOIS 服务器：内置表优先，可开启
  `{后缀}.whois-servers.net` DNS 发现作为兜底（whoisthisdomain 同款机制）
- 阿里云 CheckDomain 官方核验（可选）：结果列直接标注「阿里云·可注册 / 已注册」，
  对 .de 等易限流/易误报的后缀尤其有效
- DNS NS 交叉校验：本地 WHOIS/RDAP 提示可注册但域名已有 NS 记录时，标记「待确认」，
  避免把已注册域名误报成可注册

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
│   ├── src/aliyun.rs     # 阿里云 CheckDomain 官方核验（RPC 签名）
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

## 阿里云核验（可选，推荐开启）

App 内「设置」页配置一次即可。先选站点：**中国站**（aliyun.com，端点
`domain.aliyuncs.com`）或**国际站**（alibabacloud.com，端点
`domain-intl.aliyuncs.com`）。两站账号体系完全独立、可查后缀范围不同：
国际站能查 .us / .de / .co.uk 等更多国家/地区域名；选哪个站点，就要配哪个站点
RAM 账号里创建的 AccessKey。无需单独开通 RAM 产品，普通账号即可用：

1. 中国站账号登录 <https://ram.console.aliyun.com/users>；国际站账号登录
   <https://ram.console.alibabacloud.com/users>（国际站需先在
   account.alibabacloud.com 用邮箱注册，账号与中国站互不相通）；
2. 「创建用户」→ 勾选 **OpenAPI 调用访问**，保存弹出的 AccessKey ID / Secret；
3. 给该用户授权 **AliyunDomainFullAccess**；
4. 回到 App「设置」填入并保存。CheckDomain 免费，无额外费用。

设置只写本机 `~/.hapwhois/settings.json`（Windows 为
`%USERPROFILE%\.hapwhois\settings.json`，Unix 权限 600）。

> aliyun.com 与 alibabacloud.com 虽界面相似，但属于两个独立平台；
> GoDaddy / Namecheap / OVHcloud 等注册商搜索框查的也是注册局数据
> （RDAP/WHOIS），并不存在更权威的独立公开接口；阿里云 CheckDomain
> 与它们基于同一份事实，且官方渠道不受 .de 等注册局限流影响。

## 已知限制

- 传统 WHOIS 目前内置了常见 TLD（.com/.net/.org/.io 等）的服务器路由表；其余 TLD 依赖 RDAP（rdap.org 已覆盖绝大多数主流顶级域）。
- 阿里云账号级 QPS 约 10，App 内并发上限 6，适合中批量查询。
- 未做查询缓存与域名监控，可作为后续扩展。

## 版权

© 2026 HapX™。保留所有权利。

HapX™ 是 HapX 的注册商标，HapWHOIS 为 HapX 旗下产品。未经许可，不得擅自复制、分发或修改本软件及其相关素材。
