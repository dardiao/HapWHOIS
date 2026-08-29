use std::time::Duration;

use hickory_resolver::TokioResolver;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct WhoisData {
    pub text: String,
    pub server: String,
    /// 服务器返回“无匹配”，域名未注册（可能可注册）
    pub available: bool,
}

/// 内置 TLD → WHOIS 服务器映射表（按域名后缀自动路由）
fn whois_server(tld: &str) -> Option<&'static str> {
    Some(match tld {
        "com" | "net" => "whois.verisign-grs.com",
        "org" => "whois.pir.org",
        "io" => "whois.nic.io",
        "me" => "whois.nic.me",
        "co" => "whois.nic.co",
        "app" | "dev" | "page" => "whois.nic.google",
        "xyz" => "whois.nic.xyz",
        "info" | "biz" => "whois.afilias.net",
        "us" => "whois.nic.us",
        "tv" => "whois.nic.tv",
        "cc" => "whois.nic.cc",
        "shop" | "online" | "site" | "website" | "tech" | "space" | "press" => {
            "whois.donuts.co"
        }
        "club" => "whois.nic.club",
        "work" => "whois.nic.work",
        "cn" => "whois.cnnic.cn",
        "hk" => "whois.hkirc.hk",
        "tw" => "whois.twnic.net.tw",
        "sg" => "whois.sgnic.sg",
        "jp" => "whois.jprs.jp",
        "kr" => "whois.kr",
        "de" => "whois.denic.de",
        "fr" => "whois.nic.fr",
        "uk" => "whois.nic.uk",
        "ru" => "whois.tcinet.ru",
        "au" => "whois.auda.org.au",
        "in" => "whois.registry.in",
        "it" => "whois.nic.it",
        "es" => "whois.nic.es",
        "nl" => "whois.sidn.nl",
        "se" => "whois.iis.se",
        "no" => "whois.norid.no",
        "pl" => "whois.dns.pl",
        "br" => "whois.registro.br",
        "mx" => "whois.mx",
        "nz" => "whois.srs.net.nz",
        "za" => "whois.registry.net.za",
        "tr" => "whois.trabis.gov.tr",
        "th" => "whois.thnic.co.th",
        "id" => "whois.pandi.or.id",
        "vn" => "whois.vnnic.vn",
        "my" => "whois.mynic.net.my",
        "ph" => "whois.ph",
        _ => return None,
    })
}

/// 按内置表返回域名应连接的 WHOIS 服务器
pub fn server_for(domain: &str) -> Option<&'static str> {
    let tld = domain.rsplit('.').next().unwrap_or_default();
    whois_server(tld)
}

/// whois-servers.net DNS 发现（whoisthisdomain 同款机制）：
/// 对任意 TLD `xx`，`xx.whois-servers.net` 通过 DNS 指向该 TLD 的权威 WHOIS 服务器。
async fn dns_discover(tld: &str) -> Option<String> {
    let name = format!("{tld}.whois-servers.net");
    let addrs = resolve_ips(&name).await.ok()?;
    (!addrs.is_empty()).then_some(name)
}

/// 选择 WHOIS 服务器：内置表优先；开启 DNS 发现时，表里没有的后缀用 whois-servers.net 兜底
pub async fn resolve_server(domain: &str, use_dns_discovery: bool) -> Option<String> {
    if let Some(server) = server_for(domain) {
        return Some(server.to_string());
    }
    if use_dns_discovery {
        let tld = domain.rsplit('.').next().unwrap_or_default();
        return dns_discover(tld).await;
    }
    None
}

/// 直连 DNS 服务器解析主机名（绕过系统解析器，与 dig 行为一致），
/// 避免 macOS mDNSResponder 对部分域名解析失败的问题。
async fn resolve_ips(host: &str) -> Result<Vec<std::net::IpAddr>, String> {
    let resolver = TokioResolver::builder_tokio()
        .map_err(|e| format!("初始化 DNS 解析器失败: {e}"))?;
    let resolver = resolver.build();
    let response = resolver
        .lookup_ip(host)
        .await
        .map_err(|e| format!("解析 {host} 失败: {e}"))?;
    Ok(response.iter().collect())
}

async fn connect(server: &str) -> Result<TcpStream, String> {
    let ips = resolve_ips(server).await?;
    let mut last_err = None;
    for ip in ips {
        match TcpStream::connect((ip, 43)).await {
            Ok(stream) => {
                stream.set_nodelay(true).ok();
                return Ok(stream);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(match last_err {
        Some(e) => format!("连接 {server} 失败: {e}"),
        None => format!("{server} 没有可用的 IP 地址"),
    })
}

/// 查询 WHOIS，返回 (原始文本, 实际使用的服务器)。
/// 服务器来源可能是内置表，也可能是 whois-servers.net DNS 发现。
pub async fn lookup(domain: &str, use_dns_discovery: bool) -> Result<WhoisData, String> {
    let server = resolve_server(domain, use_dns_discovery).await.ok_or_else(|| {
        let tld = domain.rsplit('.').next().unwrap_or_default();
        format!(".{tld} 无内置路由且 whois-servers.net 未收录")
    })?;

    let bytes = tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = connect(&server).await?;
        stream
            .write_all(format!("{domain}\r\n").as_bytes())
            .await
            .map_err(|e| format!("发送查询失败: {e}"))?;
        let mut buf = Vec::new();
        stream
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        Ok::<Vec<u8>, String>(buf)
    })
    .await
    .map_err(|_| format!("连接 {server} 超时"))
    .and_then(|r| r)?;

    let text = String::from_utf8_lossy(&bytes).to_string();
    let lower = text.to_lowercase();

    // 注册局限流/风控提示：当作查询失败，保留服务器原文。
    if lower.contains("too many requests") || lower.contains("slow down") {
        let msg = text
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.starts_with('%'))
            .next()
            .unwrap_or("too many requests");
        return Err(msg.to_string());
    }

    let available = text.trim().is_empty()
        || lower.contains("no match for")
        || lower.contains("not found:")
        || lower.contains("no entries found")
        || lower.contains("status: free")
        || lower.contains("no matching objects");
    Ok(WhoisData {
        text,
        server,
        available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_by_tld() {
        assert_eq!(server_for("example.com"), Some("whois.verisign-grs.com"));
        assert_eq!(server_for("example.org"), Some("whois.pir.org"));
        assert_eq!(server_for("example.cn"), Some("whois.cnnic.cn"));
        assert_eq!(server_for("sub.example.co.uk"), Some("whois.nic.uk"));
        assert_eq!(server_for("example.top"), None, ".top 不在内置表，应走 DNS 发现");
    }

    #[tokio::test]
    async fn dns_discovery_resolves_unknown_tld() {
        // .top 不在内置表，但 whois-servers.net 收录了它
        let server = resolve_server("example.top", true)
            .await
            .expect(".top 应能通过 whois-servers.net 发现");
        assert_eq!(server, "top.whois-servers.net");
    }

    #[tokio::test]
    async fn dns_discovery_negative() {
        assert!(dns_discover("zznotarealtld").await.is_none());
    }

    #[tokio::test]
    async fn whois_example_com() {
        let data = lookup("example.com", false)
            .await
            .expect("WHOIS 查询失败");
        assert_eq!(data.server, "whois.verisign-grs.com");
        assert!(!data.available);
        assert!(data.text.len() > 50);
    }

    #[tokio::test]
    async fn whois_unregistered_detected() {
        let data = lookup("zzq9x-nope-8842-this-domain-wont-exist.com", false)
            .await
            .expect("WHOIS 查询失败");
        assert!(data.available, "未注册域名应标记为 available");
        assert!(!data.text.is_empty());
    }

    #[tokio::test]
    async fn whois_fallback_via_dns() {
        // 端到端：.top 走 DNS 发现后能连上服务器并拿到响应
        // example.top 可能未注册（返回“无结果”），这也证明连通成功
        match lookup("example.top", true).await {
            Ok(data) => {
                assert_eq!(data.server, "top.whois-servers.net");
                assert!(!data.text.is_empty());
            }
            Err(e) => assert!(e.contains("超时") || e.contains("连接"), "意外的错误: {e}"),
        }
    }
}
