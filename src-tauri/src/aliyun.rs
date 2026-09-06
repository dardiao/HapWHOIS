//! 阿里云域名服务 CheckDomain API（官方可用性核验）。
//! 文档：https://help.aliyun.com/zh/dws/developer-reference/api-domain-2018-01-29-checkdomain

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

const ENDPOINT: &str = "https://domain.aliyuncs.com/";
const VERSION: &str = "2018-01-29";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AliyunConfig {
    pub access_key: String,
    pub secret: String,
}

impl AliyunConfig {
    pub fn is_ready(&self) -> bool {
        !self.access_key.is_empty() && !self.secret.is_empty()
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliyunResult {
    /// "1"=可注册 "0"=不可注册 "-1"=查询异常
    pub avail: String,
    pub premium: bool,
    pub price: Option<u64>,
}

/// RFC 3986 百分号编码（阿里云 RPC 签名要求）
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn hmac_sha1_base64(secret: &str, message: &str) -> String {
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes()).expect("HMAC 接受任意长度密钥");
    mac.update(message.as_bytes());
    BASE64.encode(mac.finalize().into_bytes())
}

fn timestamp() -> String {
    // 阿里云要求 UTC 时间：YYYY-MM-DDTHH:mm:ssZ
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let secs_of_day = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let h = secs_of_day / 3600;
    let mi = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// 天数 → 公历（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn signature_nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("hapwhois-{now}")
}

/// 复用同一个 HTTP 客户端（连接池 + TLS 复用），批量核验时不重复握手
fn shared_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .user_agent("HapWHOIS/0.3 (aliyun check)")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

/// 调用 CheckDomain 检查单个域名是否可注册。
pub async fn check_domain(domain: &str, cfg: &AliyunConfig) -> Result<AliyunResult, String> {
    if !cfg.is_ready() {
        return Err("未配置阿里云 AccessKey".into());
    }

    let mut params: BTreeMap<&str, String> = BTreeMap::new();
    params.insert("Action", "CheckDomain".into());
    params.insert("DomainName", domain.to_string());
    params.insert("Format", "JSON".into());
    params.insert("Version", VERSION.into());
    params.insert("AccessKeyId", cfg.access_key.clone());
    params.insert("SignatureMethod", "HMAC-SHA1".into());
    params.insert("SignatureVersion", "1.0".into());
    params.insert("SignatureNonce", signature_nonce());
    params.insert("Timestamp", timestamp());

    let canonical = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let string_to_sign = format!("GET&%2F&{}", percent_encode(&canonical));
    let signature = hmac_sha1_base64(&format!("{}&", cfg.secret), &string_to_sign);

    let query = format!("{canonical}&Signature={}", percent_encode(&signature));
    let url = format!("{ENDPOINT}?{query}");

    let resp = shared_client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("阿里云请求失败: {e}"))?;
    let raw: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("阿里云响应解析失败: {e}"))?;

    if let Some(code) = raw.get("Code").and_then(|c| c.as_str()) {
        let msg = raw
            .get("Message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("阿里云返回错误 {code}: {msg}"));
    }

    let avail = raw
        .get("Avail")
        .map(|v| {
            v.as_str()
                .map(String::from)
                .unwrap_or_else(|| v.to_string())
        })
        .unwrap_or_else(|| "-1".into());
    let premium = raw.get("Premium").and_then(|p| p.as_bool()).unwrap_or(false);
    let price = raw.get("Price").and_then(|p| p.as_u64());
    Ok(AliyunResult {
        avail,
        premium,
        price,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_matches_rfc3986() {
        assert_eq!(percent_encode("a b&c+"), "a%20b%26c%2B");
        assert_eq!(percent_encode("Abc-_.~"), "Abc-_.~");
        assert_eq!(percent_encode("中文"), "%E4%B8%AD%E6%96%87");
    }

    #[test]
    fn canonical_query_is_sorted() {
        let mut params = BTreeMap::new();
        params.insert("DomainName", "example.com".to_string());
        params.insert("Action", "CheckDomain".to_string());
        params.insert("Format", "JSON".to_string());
        let canonical = params
            .iter()
            .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        assert_eq!(
            canonical,
            "Action=CheckDomain&DomainName=example.com&Format=JSON"
        );
    }
}
