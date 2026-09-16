# HapWHOIS

域名批量信息查询桌面应用（macOS / Windows）。可注册判断基于 **IANA / ICANN 权威数据源**：
RDAP 直达各后缀的注册局端点，没有 RDAP 的后缀走注册局官方 WHOIS，并附带 DNS NS 交叉校验，
不需要注册任何平台账号或配置 API 密钥。

本仓库是**发布仓库**，只提供安装包与更新说明，源码不在此仓库。

## 下载

| 平台 | 安装包 |
|---|---|
| macOS（Apple Silicon） | [HapWHOIS-macos-arm64.dmg](https://github.com/dardiao/HapWHOIS/releases/latest) |
| Windows（x64） | [HapWHOIS-windows-x64.exe](https://github.com/dardiao/HapWHOIS/releases/latest) |

也可以直接打开 [Releases](https://github.com/dardiao/HapWHOIS/releases) 选择历史版本。

## 主要功能

- 批量查询：每行一个域名，自动去重、并发限流、结果实时滚出
- 按后缀精确路由：IANA RDAP bootstrap（1200+ 后缀直达注册局权威端点）
- IANA TLD 选择器：从 IANA 根区 1438 个 TLD 中挑选后缀，批量生成/查询
- 显示注册商、注册/到期时间、域名状态、Name Server，可导出 CSV / HTML
- 内置字典生成器（手动输入 / 词根组合 / 字母组合），支持导出到批量查询
- 应用内在线更新：发现新版本自动下载安装

## 更新说明

已安装的用户无需手动下载：应用启动后会自动检查新版本，也可以在
「设置 → 软件更新」里手动检查。

## 版权

© 2026 HapX™。保留所有权利。HapX™ 是 HapX 的注册商标。
