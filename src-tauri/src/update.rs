//! 在线更新：检查 GitHub Releases → 应用内下载（进度）→ 替换安装 → 重启。
//! 与 HapCLI 的更新流程一致：公开仓库、匿名调用 GitHub API、不做签名校验。

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// 发布仓库 API 与允许打开的发布页前缀
const API_LATEST: &str = "https://api.github.com/repos/dardiao/HapWHOIS/releases/latest";
const REPO_PREFIX: &str = "https://github.com/dardiao/HapWHOIS/";

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize, Clone)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    available: bool,
    current_version: String,
    latest_version: String,
    notes: String,
    asset_name: String,
    asset_url: String,
    asset_size: u64,
    release_url: String,
    published_at: String,
    /// 没有匹配当前平台的安装包，只能手动打开下载页
    manual_only: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
    speed_bps: f64,
}

fn parse_version(input: &str) -> Vec<u64> {
    let trimmed = input.trim().trim_start_matches('v');
    let core = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
    let mut parts: Vec<u64> = core
        .split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0))
        .collect();
    while parts.len() < 3 {
        parts.push(0);
    }
    parts
}

/// candidate 是否比 current 新（只比较 x.y.z，忽略预发布后缀）
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse_version(candidate) > parse_version(current)
}

/// 按当前平台挑选安装包：macOS 优先 .app.zip（再退 .dmg），Windows 优先 setup.exe
fn pick_asset(assets: &[GhAsset]) -> Option<GhAsset> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    // 同一架构在不同工具链下的叫法不一致（uname -m 会给 arm64 / x86_64）
    let arch_keys: &[&str] = match arch {
        "aarch64" => &["aarch64", "arm64"],
        "x86_64" => &["x86_64", "x64", "amd64"],
        other => &[other],
    };
    let suffixes: &[&str] = match os {
        "macos" => &[".app.zip", ".dmg"],
        "windows" => &["-setup.exe", ".msi"],
        _ => &[],
    };
    let matches_arch =
        |name: &str| arch_keys.iter().any(|key| name.contains(key)) || name.contains(arch);
    for suffix in suffixes {
        if let Some(asset) = assets
            .iter()
            .find(|a| a.name.ends_with(suffix) && matches_arch(&a.name))
        {
            return Some(asset.clone());
        }
    }
    // 放宽架构匹配，避免命名差异导致找不到包
    for suffix in suffixes {
        if let Some(asset) = assets.iter().find(|a| a.name.ends_with(suffix)) {
            return Some(asset.clone());
        }
    }
    None
}

fn http_client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(format!("HapWHOIS/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| format!("创建请求失败：{e}"))
}

#[tauri::command]
pub async fn check_update() -> Result<UpdateInfo, String> {
    let resp = http_client(Duration::from_secs(20))?
        .get(API_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("检查更新失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("检查更新失败：GitHub 返回 HTTP {}", resp.status()));
    }
    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("解析发布信息失败：{e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let asset = pick_asset(&release.assets);
    Ok(UpdateInfo {
        available: is_newer(&latest, &current),
        current_version: current,
        latest_version: latest,
        notes: release.body.unwrap_or_default(),
        asset_name: asset.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
        asset_url: asset
            .as_ref()
            .map(|a| a.browser_download_url.clone())
            .unwrap_or_default(),
        asset_size: asset.as_ref().map(|a| a.size).unwrap_or(0),
        release_url: release
            .html_url
            .unwrap_or_else(|| format!("{REPO_PREFIX}releases")),
        published_at: release.published_at.unwrap_or_default(),
        manual_only: asset.is_none(),
    })
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    url: String,
    name: String,
) -> Result<String, String> {
    if !url.starts_with(REPO_PREFIX) {
        return Err("下载地址不是 HapWHOIS 的发布资源".into());
    }
    let dir = std::env::temp_dir().join("hapwhois-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建下载目录失败：{e}"))?;
    let file_name = name.rsplit('/').next().unwrap_or("update.bin").to_string();
    let path = dir.join(file_name);

    let mut resp = http_client(Duration::from_secs(600))?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(&path).map_err(|e| format!("写入文件失败：{e}"))?;
    let mut downloaded = 0u64;
    let mut last_tick = Instant::now();
    let mut last_bytes = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("下载中断：{e}"))? {
        file.write_all(&chunk)
            .map_err(|e| format!("写入文件失败：{e}"))?;
        downloaded += chunk.len() as u64;
        if last_tick.elapsed() >= Duration::from_millis(200) {
            let speed = (downloaded - last_bytes) as f64 / last_tick.elapsed().as_secs_f64();
            let _ = app.emit(
                "update-progress",
                DownloadProgress {
                    downloaded,
                    total,
                    speed_bps: speed,
                },
            );
            last_tick = Instant::now();
            last_bytes = downloaded;
        }
    }
    let _ = app.emit(
        "update-progress",
        DownloadProgress {
            downloaded,
            total: if total == 0 { downloaded } else { total },
            speed_bps: 0.0,
        },
    );
    Ok(path.to_string_lossy().to_string())
}

fn run(command: &str, args: &[&OsStr]) -> Result<(), String> {
    let status = Command::new(command)
        .args(args)
        .status()
        .map_err(|e| format!("执行 {command} 失败：{e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command} 退出码 {:?}", status.code()))
    }
}

/// shell 单引号转义，避免路径里的特殊字符破坏脚本
#[cfg(target_os = "macos")]
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn install_macos(file: &Path, exe: &Path) -> Result<(), String> {
    let base = std::env::temp_dir().join("hapwhois-update");
    let staging = base.join("stage");
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建暂存目录失败：{e}"))?;

    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let staged_app = staging.join("HapWHOIS.app");
    if name.ends_with(".dmg") {
        let mount = base.join("mnt");
        let _ = std::fs::remove_dir_all(&mount);
        std::fs::create_dir_all(&mount).map_err(|e| format!("创建挂载点失败：{e}"))?;
        run(
            "hdiutil",
            &[
                OsStr::new("attach"),
                OsStr::new("-nobrowse"),
                OsStr::new("-readonly"),
                OsStr::new("-mountpoint"),
                mount.as_os_str(),
                file.as_os_str(),
            ],
        )?;
        let entry = std::fs::read_dir(&mount)
            .map_err(|e| format!("读取 dmg 内容失败：{e}"))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| path.extension().map(|ext| ext == "app").unwrap_or(false));
        let copied = entry
            .ok_or_else(|| "安装包里没有找到 HapWHOIS.app".to_string())
            .and_then(|app| run("ditto", &[app.as_os_str(), staged_app.as_os_str()]));
        let _ = Command::new("hdiutil")
            .args(["detach"])
            .arg(&mount)
            .status();
        copied?;
    } else {
        run(
            "ditto",
            &[
                OsStr::new("-x"),
                OsStr::new("-k"),
                file.as_os_str(),
                staging.as_os_str(),
            ],
        )?;
    }
    if !staged_app.exists() {
        return Err("安装包里没有找到 HapWHOIS.app".into());
    }

    // .../HapWHOIS.app/Contents/MacOS/hapwhois → .../HapWHOIS.app
    let target = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .ok_or_else(|| "无法定位当前应用位置".to_string())?
        .to_path_buf();
    if !target.exists() {
        return Err(format!("应用目录不存在：{}", target.display()));
    }

    // 等本应用退出后替换 .app 并重新打开
    let script = base.join("apply-update.sh");
    let body = format!(
        "#!/bin/sh\n\
         sleep 1\n\
         i=0\n\
         while [ $i -lt 240 ]; do\n\
         \x20 pgrep -f {pattern} >/dev/null 2>&1 || break\n\
         \x20 sleep 0.5\n\
         \x20 i=$((i+1))\n\
         done\n\
         rm -rf {target}\n\
         ditto {staged} {target}\n\
         xattr -dr com.apple.quarantine {target} >/dev/null 2>&1\n\
         open {target}\n",
        pattern = sh_quote(&exe.to_string_lossy()),
        target = sh_quote(&target.to_string_lossy()),
        staged = sh_quote(&staged_app.to_string_lossy()),
    );
    std::fs::write(&script, body).map_err(|e| format!("写入更新脚本失败：{e}"))?;
    Command::new("/bin/sh")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动更新脚本失败：{e}"))?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows(file: &Path, exe: &Path) -> Result<(), String> {
    let dir = std::env::temp_dir().join("hapwhois-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建更新目录失败：{e}"))?;
    let script = dir.join("apply-update.cmd");
    let body = format!(
        "@echo off\r\n\
         timeout /t 2 /nobreak >nul\r\n\
         start \"\" /wait \"{}\" /S\r\n\
         start \"\" \"{}\"\r\n",
        file.display(),
        exe.display()
    );
    std::fs::write(&script, body).map_err(|e| format!("写入更新脚本失败：{e}"))?;
    let script_arg = script.to_string_lossy().to_string();
    Command::new("cmd")
        .args(["/C", &script_arg])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动更新脚本失败：{e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn install_update(app: AppHandle, path: String) -> Result<(), String> {
    let file = PathBuf::from(&path);
    if !file.exists() {
        return Err("安装包不存在，请重新下载".into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("定位当前程序失败：{e}"))?;

    // 保护：开发版（cargo run / target/debug）不在 .app 包内，替换逻辑会误删开发目录
    #[cfg(target_os = "macos")]
    {
        let bundle = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent());
        let is_app_bundle = bundle
            .and_then(|p| p.extension())
            .map(|ext| ext == "app")
            .unwrap_or(false);
        if !is_app_bundle {
            return Err("当前运行的是开发版（不在 HapWHOIS.app 内），请到发布页下载正式版安装".into());
        }
    }
    #[cfg(target_os = "windows")]
    {
        let path_text = exe.to_string_lossy().to_lowercase();
        if path_text.contains("\\target\\debug\\") || path_text.contains("\\target\\release\\") {
            return Err("当前运行的是开发版，请到发布页下载正式版安装".into());
        }
    }

    #[cfg(target_os = "macos")]
    install_macos(&file, &exe)?;
    #[cfg(target_os = "windows")]
    install_windows(&file, &exe)?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (&file, &exe);
        return Err("当前平台暂不支持自动安装，请打开发布页手动下载".into());
    }

    // 更新脚本已在后台等待；本进程退出后它才会替换安装并重启
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn open_release_page(url: String) -> Result<(), String> {
    if !url.starts_with(REPO_PREFIX) {
        return Err("只允许打开 HapWHOIS 发布页".into());
    }
    #[cfg(target_os = "macos")]
    let spawned = Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let spawned = Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let spawned = Command::new("xdg-open").arg(&url).spawn();
    spawned.map_err(|e| format!("打开发布页失败：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.3.8", "0.3.7"));
        assert!(is_newer("v0.4.0", "0.3.9"));
        assert!(!is_newer("0.3.7", "0.3.7"));
        assert!(!is_newer("0.3.6", "0.3.7"));
        assert!(is_newer("1.0.0", "0.99.99"));
    }

    #[test]
    fn picks_macos_app_zip_first() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let assets = vec![
            GhAsset {
                name: "HapWHOIS_0.3.8_aarch64.dmg".into(),
                browser_download_url: "https://example.invalid/a.dmg".into(),
                size: 1,
            },
            GhAsset {
                name: "HapWHOIS_0.3.8_aarch64.app.zip".into(),
                browser_download_url: "https://example.invalid/a.zip".into(),
                size: 2,
            },
        ];
        assert_eq!(
            pick_asset(&assets).map(|a| a.name).unwrap_or_default(),
            "HapWHOIS_0.3.8_aarch64.app.zip"
        );
    }

    #[test]
    fn matches_arm64_alias() {
        if !cfg!(target_os = "macos") {
            return;
        }
        let assets = vec![
            GhAsset {
                name: "HapWHOIS_0.3.8_aarch64.dmg".into(),
                browser_download_url: "https://example.invalid/a.dmg".into(),
                size: 1,
            },
            GhAsset {
                name: "HapWHOIS_0.3.8_arm64.app.zip".into(),
                browser_download_url: "https://example.invalid/a.zip".into(),
                size: 2,
            },
        ];
        assert_eq!(
            pick_asset(&assets).map(|a| a.name).unwrap_or_default(),
            "HapWHOIS_0.3.8_arm64.app.zip"
        );
    }

    #[tokio::test]
    async fn checks_latest_release_from_github() {
        let info = check_update().await.expect("检查更新失败");
        println!(
            "current={} latest={} available={} asset={} manual_only={}",
            info.current_version, info.latest_version, info.available, info.asset_name, info.manual_only
        );
        assert!(!info.latest_version.is_empty());
        assert!(!info.asset_name.is_empty(), "应能选中当前平台的安装包");
        if cfg!(target_os = "macos") {
            assert!(
                info.asset_name.ends_with(".app.zip") || info.asset_name.ends_with(".dmg"),
                "意外的 macOS 安装包：{}",
                info.asset_name
            );
        }
    }
}
