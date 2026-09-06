mod aliyun;
mod rdap;
mod whois;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tokio::sync::Semaphore;
use tauri::Emitter;
use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupResult {
    domain: String,
    rdap: Option<rdap::RdapInfo>,
    whois_raw: Option<String>,
    whois_server: Option<String>,
    available: bool,
    error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    domain: String,
    rdap: Option<rdap::RdapInfo>,
    whois_raw: Option<String>,
    whois_server: Option<String>,
    available: bool,
    error: Option<String>,
    /// C：本地初步判定“可注册”，但域名已有 DNS NS/A 记录 → 存疑，不能显示可注册
    ns_conflict: bool,
    /// A：阿里云 CheckDomain 核验（"1" 可注册 / "0" 已注册 / "-1" 查询异常）
    aliyun_avail: Option<String>,
    aliyun_premium: Option<bool>,
    aliyun_price: Option<u64>,
    aliyun_error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub aliyun_access_key: String,
    pub aliyun_secret: String,
    pub aliyun_enabled: bool,
    /// "cn" = 中国站（aliyun.com）；"intl" = 国际站（alibabacloud.com）
    pub aliyun_site: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AliyunSettingsView {
    access_key: String,
    secret_set: bool,
    enabled: bool,
    site: String,
}

fn normalize_site(site: &str) -> String {
    if site == "intl" { "intl".into() } else { "cn".into() }
}

fn aliyun_cfg_from(s: &AppSettings) -> Option<aliyun::AliyunConfig> {
    (s.aliyun_enabled && !s.aliyun_access_key.is_empty() && !s.aliyun_secret.is_empty()).then(
        || aliyun::AliyunConfig {
            access_key: s.aliyun_access_key.clone(),
            secret: s.aliyun_secret.clone(),
            intl: s.aliyun_site == "intl",
        },
    )
}

/// 设置保存在 ~/.hapwhois/settings.json（Windows 为 %USERPROFILE%\.hapwhois\settings.json）
fn settings_path() -> Result<std::path::PathBuf, String> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| "无法确定用户主目录".to_string())?;
    Ok(std::path::PathBuf::from(home)
        .join(".hapwhois")
        .join("settings.json"))
}

fn load_settings() -> AppSettings {
    let Ok(path) = settings_path() else {
        return AppSettings::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建设置目录失败: {e}"))?;
    }
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| format!("序列化设置失败: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("写入设置失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[tauri::command]
fn get_aliyun_settings() -> AliyunSettingsView {
    let s = load_settings();
    AliyunSettingsView {
        access_key: s.aliyun_access_key,
        secret_set: !s.aliyun_secret.is_empty(),
        enabled: s.aliyun_enabled,
        site: normalize_site(&s.aliyun_site),
    }
}

#[tauri::command]
fn save_aliyun_settings(
    access_key: String,
    secret: String,
    enabled: bool,
    site: String,
) -> Result<AliyunSettingsView, String> {
    let key = access_key.trim().to_string();
    if key.is_empty() {
        return Err("AccessKey ID 不能为空，请到 RAM 控制台创建后填写".into());
    }
    let mut s = load_settings();
    s.aliyun_access_key = key;
    // 密码框留空 = 保留已保存的 Secret，避免每次保存都要重填
    if !secret.trim().is_empty() {
        s.aliyun_secret = secret.trim().to_string();
    }
    if s.aliyun_secret.is_empty() {
        return Err("AccessKey Secret 不能为空（如需更换，请先点“移除密钥”再填入）".into());
    }
    s.aliyun_enabled = enabled;
    s.aliyun_site = normalize_site(&site);
    save_settings(&s)?;
    Ok(AliyunSettingsView {
        access_key: s.aliyun_access_key,
        secret_set: true,
        enabled: s.aliyun_enabled,
        site: normalize_site(&s.aliyun_site),
    })
}

#[tauri::command]
fn remove_aliyun_settings() -> Result<AliyunSettingsView, String> {
    let mut s = load_settings();
    s.aliyun_access_key.clear();
    s.aliyun_secret.clear();
    s.aliyun_enabled = false;
    s.aliyun_site = "cn".into();
    save_settings(&s)?;
    Ok(AliyunSettingsView {
        access_key: String::new(),
        secret_set: false,
        enabled: false,
        site: "cn".into(),
    })
}

#[tauri::command]
async fn test_aliyun_settings() -> Result<aliyun::AliyunResult, String> {
    let s = load_settings();
    let cfg = aliyun::AliyunConfig {
        access_key: s.aliyun_access_key.clone(),
        secret: s.aliyun_secret.clone(),
        intl: s.aliyun_site == "intl",
    };
    if !cfg.is_ready() {
        return Err("请先填写 AccessKey 与 Secret".into());
    }
    aliyun::check_domain("example.com", &cfg).await
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

    let (rdap, rdap_not_found) = match rdap::lookup(&domain).await {
        Ok(info) => (Some(info), false),
        Err(rdap::RdapError::NotFound) => (None, true),
        Err(e) => {
            eprintln!("RDAP 查询失败: {e}");
            (None, false)
        }
    };
    let (whois_raw, whois_server, whois_available) = match whois::lookup(&domain, use_dns_discovery).await {
        Ok(data) => (Some(data.text), Some(data.server), data.available),
        Err(_) => (None, None, false),
    };
    // 可注册判定保持保守：
    // - WHOIS 明确返回 free / no match → 可注册
    // - 注册局 RDAP 明确 404 且 WHOIS 无任何数据 → 可注册
    // - WHOIS 返回了注册数据（即使 RDAP 404，如 .de 无 RDAP）→ 按已注册处理
    let available = whois_available || (rdap_not_found && whois_raw.is_none());
    let error = if !available && rdap.is_none() && whois_raw.is_none() {
        Some("查询失败：RDAP 与 WHOIS 均未返回有效结果（可能是域名不存在或网络异常）".into())
    } else {
        None
    };

    Ok(LookupResult {
        domain,
        rdap,
        whois_raw,
        whois_server,
        available,
        error,
    })
}

#[tauri::command]
fn write_dict_file(path: String, content: String) -> Result<String, String> {
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {e}"))?;
    Ok(path)
}

#[tauri::command]
fn read_dict_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {e}"))
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

/// 一次查询收集到的三个数据源（RDAP / WHOIS / 阿里云），并发发起
struct Sources {
    rdap: Option<rdap::RdapInfo>,
    rdap_not_found: bool,
    whois_raw: Option<String>,
    whois_server: Option<String>,
    whois_available: bool,
}

async fn gather_sources(
    domain: &str,
    use_dns_discovery: bool,
    aliyun_cfg: Option<&aliyun::AliyunConfig>,
) -> (
    Sources,
    Option<aliyun::AliyunResult>,
    Option<String>,
) {
    let aliyun_fut = async {
        match aliyun_cfg {
            Some(cfg) if cfg.is_ready() => match aliyun::check_domain(domain, cfg).await {
                Ok(res) => (Some(res), None),
                Err(e) => (None, Some(e)),
            },
            _ => (None, None),
        }
    };
    let (rdap_r, whois_r, (aliyun_res, aliyun_err)) = tokio::join!(
        rdap::lookup(domain),
        whois::lookup(domain, use_dns_discovery),
        aliyun_fut
    );

    let (rdap, rdap_not_found) = match rdap_r {
        Ok(info) => (Some(info), false),
        Err(rdap::RdapError::NotFound) => (None, true),
        Err(e) => {
            eprintln!("RDAP 查询失败: {e}");
            (None, false)
        }
    };
    let (whois_raw, whois_server, whois_available) = match whois_r {
        Ok(data) => (Some(data.text), Some(data.server), data.available),
        Err(_) => (None, None, false),
    };
    (
        Sources {
            rdap,
            rdap_not_found,
            whois_raw,
            whois_server,
            whois_available,
        },
        aliyun_res,
        aliyun_err,
    )
}

/// 最终可注册判定：
/// A. 阿里云 CheckDomain 权威结果优先（"1" 可注册、"0" 已注册）；
/// C. 否则本地判定“可注册”前，先做 DNS NS/A 交叉校验：域名已有 DNS 记录，
///    说明注册局/WHOIS 数据不可靠，改标“待确认”，避免误报可注册。
async fn decide_available(
    domain: &str,
    s: &Sources,
    aliyun: Option<&aliyun::AliyunResult>,
) -> (bool, bool) {
    if let Some(a) = aliyun {
        match a.avail.as_str() {
            "1" => return (true, false),
            "0" => return (false, false),
            _ => {}
        }
    }
    // 本地判定可注册的两个候选来源（RDAP 有数据 → 已注册，跳过）
    let provisional = s.rdap.is_none()
        && (s.whois_available || (s.rdap_not_found && s.whois_raw.is_none()));
    if !provisional {
        return (false, false);
    }
    if whois::has_dns_records(domain).await {
        (false, true)
    } else {
        (true, false)
    }
}

fn stopped_item(domain: &str, note: &str) -> BatchItem {
    BatchItem {
        domain: domain.to_string(),
        rdap: None,
        whois_raw: None,
        whois_server: None,
        available: false,
        error: Some(note.into()),
        ns_conflict: false,
        aliyun_avail: None,
        aliyun_premium: None,
        aliyun_price: None,
        aliyun_error: None,
    }
}

/// 批量查询核心：逐条完成后回调 emit 推送进度；并发受限流控制。
async fn run_batch<F>(
    mut emit: F,
    domains: Vec<String>,
    use_dns_discovery: bool,
    aliyun_cfg: Option<aliyun::AliyunConfig>,
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
        let cfg = aliyun_cfg.clone();
        tasks.spawn(async move {
            let _permit = sem
                .acquire()
                .await
                .expect("信号量被关闭");
            let item = query_one(&domain, use_dns_discovery, cfg.as_ref()).await;
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
    let settings = load_settings();
    let aliyun_cfg = aliyun_cfg_from(&settings);
    run_batch(
        |payload| {
            let _ = app.emit("lookup-progress", payload);
        },
        domains,
        use_dns_discovery,
        aliyun_cfg,
    )
    .await
}

async fn query_one(
    domain: &str,
    use_dns_discovery: bool,
    aliyun_cfg: Option<&aliyun::AliyunConfig>,
) -> BatchItem {
    if cancel_flag().load(Ordering::Relaxed) {
        return stopped_item(domain, "已停止（未执行）");
    }

    let (s, aliyun_res, aliyun_err) = gather_sources(domain, use_dns_discovery, aliyun_cfg).await;
    if cancel_flag().load(Ordering::Relaxed) {
        return stopped_item(domain, "已停止");
    }

    let (available, ns_conflict) = decide_available(domain, &s, aliyun_res.as_ref()).await;

    let aliyun_registered = matches!(
        aliyun_res.as_ref().map(|a| a.avail.as_str()),
        Some("0")
    );
    let error = if !available
        && !ns_conflict
        && !aliyun_registered
        && s.rdap.is_none()
        && s.whois_raw.is_none()
    {
        let mut msg = "RDAP 与 WHOIS 均未返回结果".to_string();
        if let Some(e) = &aliyun_err {
            msg.push_str(&format!("；阿里云核验失败：{e}"));
        }
        Some(msg)
    } else {
        None
    };

    BatchItem {
        domain: domain.to_string(),
        rdap: s.rdap,
        whois_raw: s.whois_raw,
        whois_server: s.whois_server,
        available,
        error,
        ns_conflict,
        aliyun_avail: aliyun_res.as_ref().map(|a| a.avail.clone()),
        aliyun_premium: aliyun_res.as_ref().map(|a| a.premium),
        aliyun_price: aliyun_res.as_ref().and_then(|a| a.price),
        aliyun_error: aliyun_err,
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .menu(|app| {
            let about = PredefinedMenuItem::about(
                app,
                Some("About HapWHOIS"),
                Some(AboutMetadata {
                    name: Some("HapWHOIS".into()),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    copyright: Some(
                        "© 2026 HapX™ 保留所有权利。HapX™ 是 HapX 的注册商标。".into(),
                    ),
                    ..Default::default()
                }),
            )?;
            let app_menu = Submenu::with_items(
                app,
                "HapWHOIS",
                true,
                &[
                    &about,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::show_all(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?;
            let edit_menu = Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?;
            let window_menu = Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::fullscreen(app, None)?,
                ],
            )?;
            Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])
        })
        .invoke_handler(tauri::generate_handler![
            lookup,
            lookup_batch,
            cancel_lookup,
            write_dict_file,
            read_dict_file,
            get_aliyun_settings,
            save_aliyun_settings,
            remove_aliyun_settings,
            test_aliyun_settings
        ])
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
            None,
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
