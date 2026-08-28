use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RdapInfo {
    pub registrar: Option<String>,
    pub creation_date: Option<String>,
    pub expiration_date: Option<String>,
    pub updated_date: Option<String>,
    pub status: Vec<String>,
    pub nameservers: Vec<String>,
}

fn string_list(value: &Value, key: &str, pick: fn(&Value) -> Option<String>) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(pick).collect())
        .unwrap_or_default()
}

fn find_event(raw: &Value, action: &str) -> Option<String> {
    raw.get("events")
        .and_then(|v| v.as_array())
        .and_then(|events| {
            events
                .iter()
                .find(|e| e.get("eventAction").and_then(|s| s.as_str()) == Some(action))
        })
        .and_then(|e| e.get("eventDate"))
        .and_then(|s| s.as_str())
        .map(String::from)
}

fn vcard_value(entities: Option<&Value>, kind: &str) -> Option<String> {
    entities?
        .as_array()?
        .iter()
        .find(|e| {
            e.get("roles")
                .and_then(|r| r.as_array())
                .map(|roles| roles.iter().any(|r| r.as_str() == Some("registrar")))
                .unwrap_or(false)
        })
        .and_then(|e| e.get("vcardArray"))
        .and_then(|v| v.get(1))
        .and_then(|v| v.as_array())
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.get(0).and_then(|x| x.as_str()) == Some(kind))
        })
        .and_then(|entry| entry.get(3))
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub async fn lookup(domain: &str) -> Result<RdapInfo, String> {
    let url = format!("https://rdap.org/domain/{domain}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("HapWHOIS/0.1 (desktop app)")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("RDAP 请求失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("RDAP 返回 HTTP {}", resp.status().as_u16()));
    }

    let raw: Value = resp
        .json()
        .await
        .map_err(|e| format!("RDAP 响应解析失败: {e}"))?;

    let entities = raw.get("entities");
    let registrar = vcard_value(entities, "fn")
        .or_else(|| vcard_value(entities, "org"))
        .or_else(|| {
            entities
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter().find(|e| {
                        e.get("roles")
                            .and_then(|r| r.as_array())
                            .map(|roles| roles.iter().any(|r| r.as_str() == Some("registrar")))
                            .unwrap_or(false)
                    })
                })
                .and_then(|e| e.get("handle"))
                .and_then(|h| h.as_str())
                .map(String::from)
        });

    Ok(RdapInfo {
        registrar,
        creation_date: find_event(&raw, "registration"),
        expiration_date: find_event(&raw, "expiration"),
        updated_date: find_event(&raw, "last changed"),
        status: string_list(&raw, "status", |s| s.as_str().map(String::from)),
        nameservers: string_list(&raw, "nameservers", |ns| {
            ns.get("ldhName").and_then(|s| s.as_str()).map(String::from)
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_example_com() {
        let info = lookup("example.com").await.expect("RDAP 查询失败");
        assert!(!info.nameservers.is_empty());
        assert!(info.creation_date.is_some());
        println!("registrar={:?} created={:?} ns={:?}", info.registrar, info.creation_date, info.nameservers);
    }
}
