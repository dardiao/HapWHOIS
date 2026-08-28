mod rdap;
mod whois;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tokio::sync::Semaphore;
use tauri::Emitter;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupResult {
    domain: String,
    rdap: Option<rdap::RdapInfo>,
    whois_raw: Option<String>,
    whois_server: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    domain: String,
    rdap: Option<rdap::RdapInfo>,
    whois_raw: Option<String>,
    whois_server: Option<String>,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    done: usize,
    total: usize,
    item: BatchItem,
}

/// 同时进行的域名查询数上限（对 WHOIS 服务器保持礼貌，避免触发限流）
const MAX_CONCURRENT: usize = 6;

/// 批量查询取消标记：一个桌面窗口内同时只有一个批量任务
static CANCEL: OnceLock<AtomicBool> = OnceLock::new();

fn cancel_flag() -> &'static AtomicBool {
    CANCEL.get_or_init(|| AtomicBool::new(false))
}

#[tauri::command]
fn cancel_lookup() {
    cancel_flag().store(true, Ordering::Relaxed);
}

fn normalize_domain(input: &str) -> Result<String, String> {
    let d = input.trim().to_lowercase();
    if d.is_empty() {
        return Err("请输入域名".into());
    }
    if !d
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
    {
        return Err("域名只能包含字母、数字、点号和连字符".into());
    }
    if d.starts_with('.') || d.ends_with('.') {
        return Err("域名格式不正确".into());
    }
    Ok(d)
}

#[tauri::command(rename_all = "camelCase")]
async fn lookup(domain: String, use_dns_discovery: bool) -> Result<LookupResult, String> {
    let domain = normalize_domain(&domain)?;

    let rdap = rdap::lookup(&domain).await.ok();
    let (whois_raw, whois_server) = match whois::lookup(&domain, use_dns_discovery).await {
        Ok((text, server)) => (Some(text), Some(server)),
        Err(_) => (None, None),
    };

    if rdap.is_none() && whois_raw.is_none() {
        return Err("查询失败：RDAP 与 WHOIS 均未返回有效结果（可能是域名不存在或网络异常）".into());
    }

    Ok(LookupResult {
        domain,
        rdap,
        whois_raw,
        whois_server,
        error: None,
    })
}

/// 批量查询：支持每行一个域名，也兼容逗号/分号/空格分隔；
/// 自动去重（保持输入顺序）。
fn parse_domains(domains: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<String> = Vec::new();

    for raw in domains {
        for chunk in raw.split(|c: char| matches!(c, '\n' | ',' | ';' | '\t')) {
            for piece in chunk.split_whitespace() {
                if let Ok(d) = normalize_domain(piece) {
                    if seen.insert(d.clone()) {
                        list.push(d);
                    }
                }
            }
        }
    }
    list
}

/// 批量查询核心：逐条完成后回调 emit 推送进度；并发受限流控制。
async fn run_batch<F>(
    mut emit: F,
    domains: Vec<String>,
    use_dns_discovery: bool,
) -> Result<Vec<BatchItem>, String>
where
    F: FnMut(ProgressPayload),
{
    let list = parse_domains(domains);
    if list.is_empty() {
        return Err("没有可查询的域名，请至少输入一个有效域名".into());
    }

    cancel_flag().store(false, Ordering::Relaxed);
    let total = list.len();

    let semaphore = std::sync::Arc::new(Semaphore::new(MAX_CONCURRENT));
    let mut tasks = tokio::task::JoinSet::new();

    for (index, domain) in list.into_iter().enumerate() {
        let sem = semaphore.clone();
        tasks.spawn(async move {
            let _permit = sem
                .acquire()
                .await
                .expect("信号量被关闭");
            let item = query_one(&domain, use_dns_discovery).await;
            (index, item)
        });
    }

    let mut collected: Vec<(usize, BatchItem)> = Vec::new();
    let mut done = 0usize;
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(pair) => {
                done += 1;
                let payload = ProgressPayload {
                    done,
                    total,
                    item: pair.1.clone(),
                };
                emit(payload);
                collected.push(pair);
            }
            Err(e) => eprintln!("批量查询任务异常: {e}"),
        }
    }
    collected.sort_by_key(|(index, _)| *index);
    Ok(collected.into_iter().map(|(_, item)| item).collect())
}

#[tauri::command(rename_all = "camelCase")]
async fn lookup_batch(
    app: tauri::AppHandle,
    domains: Vec<String>,
    use_dns_discovery: bool,
) -> Result<Vec<BatchItem>, String> {
    let total = parse_domains(domains.clone()).len();
    let _ = app.emit("lookup-start", serde_json::json!({ "total": total }));
    run_batch(
        |payload| {
            let _ = app.emit("lookup-progress", payload);
        },
        domains,
        use_dns_discovery,
    )
    .await
}

async fn query_one(domain: &str, use_dns_discovery: bool) -> BatchItem {
    if cancel_flag().load(Ordering::Relaxed) {
        return BatchItem {
            domain: domain.to_string(),
            rdap: None,
            whois_raw: None,
            whois_server: None,
            error: Some("已停止（未执行）".into()),
        };
    }

    let (rdap, whois) =
        tokio::join!(rdap::lookup(domain), whois::lookup(domain, use_dns_discovery));
    if cancel_flag().load(Ordering::Relaxed) {
        return BatchItem {
            domain: domain.to_string(),
            rdap: None,
            whois_raw: None,
            whois_server: None,
            error: Some("已停止".into()),
        };
    }
    let rdap = rdap.ok();
    let (whois_raw, whois_server) = match whois {
        Ok((text, server)) => (Some(text), Some(server)),
        Err(_) => (None, None),
    };
    let error = if rdap.is_none() && whois_raw.is_none() {
        Some("RDAP 与 WHOIS 均未返回结果".into())
    } else {
        None
    };
    BatchItem {
        domain: domain.to_string(),
        rdap,
        whois_raw,
        whois_server,
        error,
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![lookup, lookup_batch, cancel_lookup])
        .run(tauri::generate_context!())
        .expect("运行 HapWHOIS 失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_trims_and_validates() {
        assert_eq!(normalize_domain("  Example.COM \n"), Ok("example.com".into()));
        assert!(normalize_domain("").is_err());
        assert!(normalize_domain("bad domain!").is_err());
    }

    #[tokio::test]
    async fn batch_dedupe_and_query() {
        let mut progress_count = 0usize;
        let items = run_batch(
            |payload| {
                progress_count += 1;
                assert!(payload.total == 2);
            },
            vec!["example.com".into(), "example.com".into(), "example.org".into()],
            true,
        )
        .await
        .expect("批量查询失败");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].domain, "example.com");
        assert_eq!(items[1].domain, "example.org");
        assert_eq!(progress_count, 2);
        for item in &items {
            assert!(
                item.rdap.is_some() || item.whois_raw.is_some(),
                "{} 应至少有一种数据源",
                item.domain
            );
        }
    }
}
