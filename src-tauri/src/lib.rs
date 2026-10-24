use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::copy;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

// Live component-install progress. The launcher polls this through a cheap Tauri
// command while the blocking downloader runs on a worker thread.
static INSTALL_ACTIVE: AtomicBool = AtomicBool::new(false);
static INSTALL_DOWNLOADED: AtomicU64 = AtomicU64::new(0);
static INSTALL_TOTAL: AtomicU64 = AtomicU64::new(0);
static INSTALL_CURRENT: OnceLock<Mutex<String>> = OnceLock::new();

fn install_current() -> &'static Mutex<String> {
    INSTALL_CURRENT.get_or_init(|| Mutex::new(String::new()))
}

fn set_install_current(value: impl Into<String>) {
    if let Ok(mut current) = install_current().lock() {
        *current = value.into();
    }
}

#[derive(Serialize, Clone, Default)]
struct InstallProgress {
    active: bool,
    downloaded: u64,
    total: u64,
    percent: f64,
    current: String,
}

/// Spawn a console child without flashing a console window from the GUI launcher.
fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[derive(Serialize, Clone)]
struct EnvItem {
    label: String,
    value: String,
    ok: bool,
}

#[derive(Serialize, Clone)]
struct GatewayInfo {
    installed: bool,
    running: bool,
    healthy: bool,
    port: u16,
    version: String,
    jobs: u64,
    home: String,
    gateway_dir: String,
    message: String,
}

#[derive(Serialize, Clone)]
struct VoiceInfo {
    name: String,
    status: String,
    info: String,
    sr: String,
    size_mb: u64,
    has_index: bool,
    message: String,
}

#[derive(Serialize, Clone)]
struct KernelInfo {
    installed: bool,
    running: bool,
    starting: bool,
    url: String,
    port: u16,
    pid: u32,
    dsh_home: String,
    log: String,
    message: String,
}

#[derive(Serialize, Clone)]
struct JobInfo {
    id: String,
    job_type: String,
    status: String,
    progress: f64,
    stage: String,
    error: String,
}

#[derive(Serialize, Clone)]
struct UpdateComponent {
    id: String,
    kind: String,
    version: String,
    size: u64,
}

#[derive(Serialize, Clone)]
struct UpdateManifest {
    schema: String,
    channel: String,
    sequence: u64,
    signature_present: bool,
    components: Vec<UpdateComponent>,
    path: String,
}

#[derive(Serialize, Clone)]
struct ComponentItem {
    name: String,
    state: String,
    detail: String,
    ok: bool,
}

fn item(label: &str, value: String, ok: bool) -> EnvItem {
    EnvItem {
        label: label.to_string(),
        value,
        ok,
    }
}

fn run_ps(script: &str) -> String {
    // PowerShell 5.1 writes GBK/ANSI when stdout is redirected from a windowless
    // process. Force UTF-8 before the probe so Chinese values are not mojibake.
    let wrapped = format!(
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new(); $OutputEncoding=[Console]::OutputEncoding; {script}"
    );
    let output = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &wrapped])
        .output();
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if text.is_empty() && !err.is_empty() { err } else { text }
        }
        Err(e) => format!("ERR: {e}"),
    }
}

fn run_cmd(program: &str, args: &[&str]) -> String {
    match hidden_command(program).args(args).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if stdout.is_empty() { stderr } else { stdout }
        }
        Err(_) => String::new(),
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// The packaged install keeps every byte of AMS state below its own directory.
fn installed_data_dir() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let dir = exe.parent()?;
    if dir.join("uninstall.exe").is_file() {
        Some(dir.join("data"))
    } else {
        None
    }
}

fn ams_home() -> PathBuf {
    if let Some(data) = installed_data_dir() {
        return data;
    }
    // Development fallback only. The installed build never reads AMS_HOME.
    if let Ok(value) = env::var("AMS_HOME") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    // Never fall back to %LOCALAPPDATA%: all AMS state belongs under the install root.
    // A standalone dev binary uses <exe-dir>\data so it cannot leak into C:.
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}


fn ams_home_marker_path() -> PathBuf {
    ams_home().join(".ams-home.json")
}

fn ensure_ams_home_marker() -> Result<(), String> {
    let home = ams_home();
    fs::create_dir_all(&home).map_err(|e| format!("创建 AMS_HOME 失败: {e}"))?;
    let marker = ams_home_marker_path();
    if marker.is_file() {
        return Ok(());
    }
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let value = serde_json::json!({
        "schema": "ams-home/1",
        "app_id": "com.yanmc.ams",
        "created_at": created
    });
    fs::write(&marker, serde_json::to_string_pretty(&value).unwrap_or_default())
        .map_err(|e| format!("写入 AMS_HOME 归属标记失败: {e}"))
}

const DATA_LAYOUT_DIRS: &[&str] = &[
    "artifacts",
    "assets",
    "backups",
    "cache",
    "components",
    "config",
    "datasets",
    "kernel",
    "logs",
    "models",
    "projects",
    "projects/imports",
    "recordings/vc",
    "runtime",
    "tmp",
    "tools",
    "tts_voices",
    "voices",
];

/// Create the managed data layout once at launcher start. This is additive:
/// existing user files are never removed, and every path stays under data/.
fn ensure_data_layout() {
    let home = ams_home();
    for name in DATA_LAYOUT_DIRS {
        let _ = fs::create_dir_all(home.join(name));
    }
    seed_tts_voices(&home);
}

/// Copy the bundled Piper model into data/tts_voices only when the user folder
/// is empty. The runtime copy remains the fallback; the data copy is editable.
fn seed_tts_voices(home: &PathBuf) {
    let src = home.join("runtime").join("tts").join("0.1.0").join("models");
    let dst_root = home.join("tts_voices");
    if !src.is_dir() {
        return;
    }
    let has_user_models = fs::read_dir(&dst_root)
        .map(|entries| entries.filter_map(|entry| entry.ok()).any(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("onnx") || entry.path().is_dir()))
        .unwrap_or(false);
    if has_user_models {
        return;
    }
    let _ = fs::create_dir_all(&dst_root);
    if let Ok(entries) = fs::read_dir(&src) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("onnx") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else { continue };
            let model_dir = dst_root.join(stem);
            if fs::create_dir_all(&model_dir).is_err() { continue; }
            let Some(file_name) = path.file_name() else { continue };
            let _ = fs::copy(&path, model_dir.join(file_name));
            let config = path.with_extension("onnx.json");
            if config.is_file() {
                if let Some(config_name) = config.file_name() {
                    let _ = fs::copy(&config, model_dir.join(config_name));
                }
            }
        }
    }
}

fn data_folder_path(kind: &str) -> Result<PathBuf, String> {
    let home = ams_home();
    let path = match kind {
        "rvc_voices" => home.join("voices"),
        "tts_voices" => home.join("tts_voices"),
        "artifacts" => home.join("artifacts"),
        "projects" => home.join("projects"),
        "datasets" => home.join("datasets"),
        "logs" => home.join("logs"),
        _ => return Err(format!("不支持的数据目录类型：{kind}")),
    };
    fs::create_dir_all(&path).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let canonical_home = home.canonicalize().map_err(|e| format!("解析数据目录失败: {e}"))?;
    let canonical_path = path.canonicalize().map_err(|e| format!("解析目标目录失败: {e}"))?;
    if !canonical_path.starts_with(&canonical_home) {
        return Err("目标目录不在 AMS data 内".to_string());
    }
    Ok(canonical_path)
}

#[tauri::command]
fn open_data_folder(kind: String) -> Result<String, String> {
    let path = data_folder_path(&kind)?;
    // Explorer is a GUI process; do not attach CREATE_NO_WINDOW here.
    Command::new("explorer.exe")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("打开目录失败: {e}"))?;
    Ok(path.display().to_string())
}


fn is_reparse_point(path: &PathBuf) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return fs::symlink_metadata(path)
            .map(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        fs::symlink_metadata(path).map(|m| m.file_type().is_symlink()).unwrap_or(false)
    }
}

fn marker_is_ams_owned() -> bool {
    let text = match fs::read_to_string(ams_home_marker_path()) {
        Ok(text) => text,
        Err(_) => return false,
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return false,
    };
    value.get("schema").and_then(|v| v.as_str()) == Some("ams-home/1")
        && value.get("app_id").and_then(|v| v.as_str()) == Some("com.yanmc.ams")
}

fn canonical_lower(path: &PathBuf) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn owned_ams_home() -> Result<PathBuf, String> {
    let home = ams_home();
    if !home.exists() {
        return Err(format!("AMS_HOME 不存在，无需清理：{}", home.display()));
    }
    if is_reparse_point(&home) {
        return Err(format!("拒绝清理：AMS_HOME 是符号链接/目录联接：{}", home.display()));
    }
    let canonical = fs::canonicalize(&home).map_err(|e| format!("解析 AMS_HOME 失败: {e}"))?;
    let default_home = installed_data_dir().and_then(|p| fs::canonicalize(p).ok());
    let is_default = default_home
        .map(|p| canonical_lower(&p) == canonical_lower(&canonical))
        .unwrap_or(false);
    let marker_ok = marker_is_ams_owned();
    if !is_default && !marker_ok {
        return Err(format!(
            "拒绝清理：{} 既不是默认 AMS 数据目录，也没有有效的 .ams-home.json 归属标记",
            canonical.display()
        ));
    }
    let text = canonical_lower(&canonical);
    if text.is_empty() || text.ends_with("\\users") || text.ends_with("\\appdata\\local") || text.ends_with("\\appdata\\roaming") {
        return Err(format!("拒绝清理异常路径：{}", canonical.display()));
    }
    Ok(canonical)
}

fn stop_pid_from_json(path: &PathBuf) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(_) => return,
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(_) => return,
    };
    let pid = value.get("pid").and_then(|v| v.as_u64()).unwrap_or(0);
    if pid > 0 {
        let _ = hidden_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

fn remove_owned_path(path: &PathBuf, removed: &mut Vec<String>, kept: &mut Vec<String>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if is_reparse_point(path) {
        kept.push(format!("{}（符号链接/目录联接，未删除）", path.display()));
        return Ok(());
    }
    if path.is_dir() {
        match fs::remove_dir_all(path) {
            Ok(()) => removed.push(path.display().to_string()),
            Err(e) => kept.push(format!("{}（删除失败：{e}）", path.display())),
        }
    } else {
        match fs::remove_file(path) {
            Ok(()) => removed.push(path.display().to_string()),
            Err(e) => kept.push(format!("{}（删除失败：{e}）", path.display())),
        }
    }
    Ok(())
}

fn cleanup_ams_data() -> Result<String, String> {
    let home = owned_ams_home()?;
    let mut removed: Vec<String> = Vec::new();
    let mut kept: Vec<String> = Vec::new();

    stop_pid_from_json(&home.join("runtime").join("kernel.json"));
    stop_pid_from_json(&home.join("runtime").join("ports.json"));
    std::thread::sleep(Duration::from_millis(800));

    // Only rebuildable/runtime-owned entries are deleted. User models and generated
    // artifacts stay in place unless the user removes them explicitly.
    for name in ["kernel", "components", "runtime", "logs"] {
        remove_owned_path(&home.join(name), &mut removed, &mut kept)?;
    }
    for leftover in ["voices", "artifacts", "models", "downloads"] {
        if home.join(leftover).exists() {
            kept.push(format!("{}（用户数据，保留）", home.join(leftover).display()));
        }
    }

    let mut root_removed = false;
    if fs::read_dir(&home).map(|mut it| it.next().is_none()).unwrap_or(false) {
        if fs::remove_dir(&home).is_ok() {
            root_removed = true;
        }
    }

    let report = format!(
        "AMS 数据清理完成\nAMS_HOME={}\n删除：{}\n保留：{}\n数据目录整体删除：{}",
        home.display(),
        if removed.is_empty() { "（无）".to_string() } else { removed.join("；") },
        if kept.is_empty() { "（无）".to_string() } else { kept.join("；") },
        root_removed
    );
    // Keep the uninstall report inside the AMS-owned install data tree; never write
    // outside the install directory.
    let log_path = home.join("uninstall.log");
    let _ = fs::write(&log_path, &report);
    Ok(report)
}

#[tauri::command]
fn open_uninstaller() -> Result<String, String> {
    let exe = env::current_exe().map_err(|e| format!("读取当前程序失败: {e}"))?;
    let install_dir = exe.parent().ok_or_else(|| "当前程序没有安装目录".to_string())?;
    let uninstaller = install_dir.join("uninstall.exe");
    if !uninstaller.is_file() {
        return Err(format!("找不到卸载程序：{}", uninstaller.display()));
    }
    let canonical_dir = fs::canonicalize(install_dir).map_err(|e| format!("解析安装目录失败: {e}"))?;
    let canonical_exe = fs::canonicalize(&exe).map_err(|e| format!("解析当前程序失败: {e}"))?;
    if canonical_exe.parent() != Some(canonical_dir.as_path()) {
        return Err(format!("拒绝启动卸载程序：{} 不在当前安装目录", canonical_exe.display()));
    }
    Command::new(&uninstaller)
        .spawn()
        .map_err(|e| format!("启动卸载程序失败: {e}"))?;
    Ok("已打开卸载程序；清理只针对 AMS 自己的数据目录".to_string())
}


#[derive(Serialize, Deserialize, Clone, Default)]
struct ConnectionConfig {
    base_url: String,
    api_key: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct RuntimeConfig {
    rvc_python: String,
    rvc_root: String,
    pymss_python: String,
    pymss_root: String,
    msvc_tool: String,
}

#[derive(Serialize)]
struct ConnectionInfo {
    base_url: String,
    has_api_key: bool,
    configured: bool,
    message: String,
}

fn connection_path() -> PathBuf {
    ams_home().join("config").join("connection.json")
}

fn read_connection() -> Option<ConnectionConfig> {
    let text = fs::read_to_string(connection_path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn yaml_quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("'{}'", escaped.replace('\'', "''"))
}

/// Normalize the profile patch: a default empty array marker must not coexist
/// with top-level list entries, and the AMS shell should not expose account/settings UI.
fn normalize_profile_patch(input: &str) -> String {
    input
        .lines()
        .filter(|line| line.trim() != "[]")
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_patch_entry(patch: &mut String, marker: &str, block: &str) {
    if patch.contains(marker) {
        return;
    }
    if !patch.is_empty() && !patch.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str(block);
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
}

/// Ensure the AMS profile overlay is valid YAML and carries the no-login shell defaults.
fn ensure_profile_base_patch(dsh_home: &PathBuf) -> Result<PathBuf, String> {
    let profile = dsh_home.join("profiles").join("desktop");
    fs::create_dir_all(&profile).map_err(|e| format!("创建 profile 失败: {e}"))?;
    let path = profile.join("cordis.patch.yml");
    let mut patch = normalize_profile_patch(&fs::read_to_string(&path).unwrap_or_default());
    append_patch_entry(
        &mut patch,
        "id: agent-default-model",
        "- id: agent-default-model\n  name: '@deepseek-ai/dsh-agent-default-model'\n  config:\n    provider: deepseek-official\n    model: deepseek-flash\n    reasoningEffort: high\n",
    );
    append_patch_entry(
        &mut patch,
        "id: ui-settings-account",
        "- id: ui-settings-account\n  disabled: true\n  name: '@deepseek-ai/dsh-client-ui-settings-account'\n  config:\n    version: 1\n    step: done\n    purpose: null\n    process: standard\n    completion: skipped\n    usage: compact\n    developerTools: false\n",
    );
    append_patch_entry(
        &mut patch,
        "id: ui-settings\n",
        "- id: ui-settings\n  name: '@deepseek-ai/dsh-client-ui-settings'\n  config:\n    enabled: false\n",
    );
    append_patch_entry(
        &mut patch,
        "id: llm-deepseek-account",
        "- id: llm-deepseek-account\n  disabled: true\n",
    );
    fs::write(&path, patch).map_err(|e| format!("写入 profile patch 失败: {e}"))?;
    Ok(path)
}

/// Remove one top-level patch entry by id, preserving the rest of the overlay.
fn remove_patch_entry(input: &str, id: &str) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let mut output: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in lines {
        if line.trim_start().starts_with("- id: ") && line.contains(id) {
            skipping = true;
            continue;
        }
        if skipping && line.trim_start().starts_with("- ") {
            skipping = false;
        }
        if !skipping {
            output.push(line);
        }
    }
    output.join("\n")
}

/// Write the two-user-input connection into the AMS Kernel profile and credential store.
fn apply_connection_config(config: &ConnectionConfig) -> Result<(), String> {
    let base_url = config.base_url.trim();
    let api_key = config.api_key.trim();
    if base_url.is_empty() || api_key.is_empty() {
        return Err("网址和 Key 都不能为空".to_string());
    }
    let dsh_home = ams_home().join("kernel").join("dsh");
    let patch_path = ensure_profile_base_patch(&dsh_home)?;
    let credentials = format!("version: 1\nrefs:\n  AMS_API_KEY: {}\n  DEEPSEEK_API_KEY: {}\n", yaml_quote(api_key), yaml_quote(api_key));
    fs::write(dsh_home.join(".credentials.yaml"), credentials).map_err(|e| format!("写入凭据失败: {e}"))?;
    let mut patch = normalize_profile_patch(&fs::read_to_string(&patch_path).unwrap_or_default());
    patch = remove_patch_entry(&patch, "llm-deepseek-ams");
    patch = remove_patch_entry(&patch, "llm-deepseek");
    if !patch.is_empty() && !patch.ends_with('\n') { patch.push('\n'); }
    patch.push_str(&format!("- id: llm-deepseek\n  name: '@deepseek-ai/dsh-llm-deepseek-api-key'\n  config:\n    baseURL: {}\n    apiKeyEnv: DEEPSEEK_API_KEY\n", yaml_quote(base_url)));
    fs::write(&patch_path, patch).map_err(|e| format!("写入 profile patch 失败: {e}"))?;
    Ok(())
}

#[tauri::command]
fn connection_status() -> ConnectionInfo {
    match read_connection() {
        Some(config) => {
            let configured = !config.base_url.is_empty() && !config.api_key.is_empty();
            ConnectionInfo {
                base_url: config.base_url,
                has_api_key: !config.api_key.is_empty(),
                configured,
                message: "连接配置只保存在 AMS 安装目录".to_string(),
            }
        },
        None => ConnectionInfo { base_url: String::new(), has_api_key: false, configured: false, message: "尚未配置".to_string() },
    }
}

#[tauri::command]
fn save_connection(base_url: String, api_key: String) -> Result<String, String> {
    let mut api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        if let Some(existing) = read_connection() { api_key = existing.api_key; }
    }
    let config = ConnectionConfig { base_url: base_url.trim().to_string(), api_key };
    apply_connection_config(&config)?;
    let path = connection_path();
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?; }
    fs::write(&path, serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {e}"))? )
        .map_err(|e| format!("保存连接配置失败: {e}"))?;
    Ok(format!("已保存到 {}；重启 AMS Kernel 后生效", path.display()))
}

fn dev_root() -> PathBuf {
    // Development-only convenience. Packaged installs must never depend on a
    // machine-specific checkout, so this only honours an explicit override or
    // a source tree that happens to sit next to the current directory/executable.
    if let Ok(value) = env::var("AMS_DEV_ROOT") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("ams-services").is_dir() {
                return dir.to_path_buf();
            }
        }
    }
    if let Ok(cwd) = env::current_dir() {
        if cwd.join("ams-services").is_dir() {
            return cwd;
        }
    }
    ams_home().join("components").join("install-root")
}

fn gateway_dir() -> PathBuf {
    if let Ok(value) = env::var("AMS_GATEWAY_DIR") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    let installed = ams_home().join("components").join("service.gateway");
    if installed.join("ams-gateway.exe").is_file()
        || installed.join(".venv").join("Scripts").join("python.exe").is_file()
    {
        return installed;
    }
    let dev = dev_root().join("ams-services").join("gateway");
    if dev.join(".venv").join("Scripts").join("python.exe").is_file()
        || dev.join("app").join("__main__.py").is_file()
    {
        return dev;
    }
    installed
}

fn gateway_python() -> PathBuf {
    gateway_dir().join(".venv").join("Scripts").join("python.exe")
}

fn gateway_installed() -> bool {
    let dir = gateway_dir();
    dir.join("ams-gateway.exe").is_file() || gateway_python().is_file()
}

fn read_ports() -> Option<(u16, u32)> {
    let path = ams_home().join("runtime").join("ports.json");
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let gateway = value.get("gateway")?;
    let port = gateway.get("port")?.as_u64()? as u16;
    let pid = gateway.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    Some((port, pid))
}

fn gateway_token() -> Option<String> {
    let path = ams_home().join("runtime").join("gateway.token");
    let text = fs::read_to_string(path).ok()?;
    let token = text.trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

fn http_get(port: u16, path: &str, token: &str) -> Result<String, String> {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().map_err(|e| format!("地址解析失败: {e}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("连接失败: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("发送失败: {e}"))?;
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer).map_err(|e| format!("读取失败: {e}"))?;
    let body = buffer.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    let status_line = buffer.lines().next().unwrap_or("");
    if !status_line.contains(" 200 ") {
        return Err(format!("HTTP 状态异常: {status_line}"));
    }
    Ok(body)
}

fn path_is_inside_ams(path: &PathBuf) -> bool {
    let Ok(target) = fs::canonicalize(path) else { return false };
    let Ok(home) = fs::canonicalize(ams_home()) else { return false };
    target.starts_with(home)
}

fn bundled_node_path() -> PathBuf {
    ams_home().join("kernel").join("resources").join("runtime").join("primary-runtime")
        .join("dependencies").join("node").join("bin").join("node.exe")
}

#[tauri::command]
fn get_environment() -> Vec<EnvItem> {
    let mut items: Vec<EnvItem> = Vec::new();
    let os = run_ps("(Get-CimInstance Win32_OperatingSystem).Caption + ' build ' + (Get-CimInstance Win32_OperatingSystem).BuildNumber");
    items.push(item("Windows", os, true));
    let cpu = run_ps("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name");
    items.push(item("CPU", cpu, true));
    let ram = run_ps("[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)");
    let ram_text = if ram.is_empty() || ram.starts_with("ERR") { ram } else { format!("{ram} GB") };
    items.push(item("内存", ram_text, true));
    let gpu = run_cmd("nvidia-smi.exe", &["--query-gpu=name,memory.total,driver_version", "--format=csv,noheader"]);
    if gpu.is_empty() {
        let wmi = run_ps("(Get-CimInstance Win32_VideoController | Select-Object -First 1).Name");
        items.push(item("GPU", wmi, false));
    } else {
        items.push(item("GPU", first_line(&gpu), true));
    }
    let webview = run_ps("$p='HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'; if(Test-Path $p){ (Get-ItemProperty $p).pv } else { '未安装' }");
    let webview_ok = !webview.is_empty() && !webview.contains("未安装");
    items.push(item("WebView2（系统组件）", if webview.is_empty() { "未检测到".to_string() } else { webview }, webview_ok));
    let vc = run_ps("$p=Join-Path $env:SystemRoot 'System32\\vcruntime140.dll'; if(Test-Path $p){ (Get-Item $p).VersionInfo.FileVersion } else { '缺失' }");
    let vc_ok = !vc.is_empty() && !vc.contains("缺失");
    items.push(item("VC++ 运行库", if vc.is_empty() { "未检测到".to_string() } else { vc }, vc_ok));
    let node_path = bundled_node_path();
    let node_version = if node_path.is_file() {
        run_cmd(&node_path.to_string_lossy(), &["-v"])
    } else { String::new() };
    let node_ok = !node_version.is_empty() && path_is_inside_ams(&node_path);
    items.push(item("内核 Node（内置）", if node_version.is_empty() { "缺失".to_string() } else { node_version }, node_ok));
    let gateway_exe = gateway_dir().join("ams-gateway.exe");
    items.push(item(
        "Gateway Python（内置）",
        if gateway_exe.is_file() { "已 bundled".to_string() } else { "未安装".to_string() },
        gateway_exe.is_file() && path_is_inside_ams(&gateway_exe),
    ));
    let runtime = read_runtime_config();
    let rvc_python = PathBuf::from(runtime.rvc_python.trim());
    let rvc_root = PathBuf::from(runtime.rvc_root.trim());
    let rvc_ok = rvc_python.is_file() && rvc_root.is_dir() && path_is_inside_ams(&rvc_python) && path_is_inside_ams(&rvc_root);
    items.push(item(
        "RVC 运行时（受管）",
        if rvc_ok { rvc_root.display().to_string() } else if runtime.rvc_root.trim().is_empty() { "未配置；请安装 runtime.rvc".to_string() } else { format!("外部路径，需迁移到 data：{}", runtime.rvc_root) },
        rvc_ok,
    ));
    let ffmpeg_candidates = [
        ams_home().join("tools").join("ffmpeg").join("bin").join("ffmpeg.exe"),
        rvc_root.join("ffmpeg.exe"),
    ];
    let ffmpeg = ffmpeg_candidates.iter().find(|path| path.is_file());
    items.push(item(
        "FFmpeg（受管）",
        match ffmpeg { Some(path) => path.display().to_string(), None => "缺失；需安装 tool.ffmpeg".to_string() },
        ffmpeg.map(|path| path_is_inside_ams(path)).unwrap_or(false),
    ));
    let voice = ams_home().join("voices").join("曼波").join("曼波.pth");
    items.push(item("曼波音源（内置资产）", if voice.is_file() { "已安装".to_string() } else { "缺失".to_string() }, voice.is_file()));
    let python = run_cmd("py.exe", &["-3", "--version"]);
    let python_ok = !python.is_empty();
    items.push(item("系统 Python（仅开发，可选）", if python_ok { python } else { "未安装（不影响运行）".to_string() }, python_ok));
    let system_node = run_cmd("node.exe", &["-v"]);
    let system_node_ok = !system_node.is_empty();
    items.push(item("系统 Node（仅开发，可选）", if system_node_ok { system_node } else { "未安装（不影响运行）".to_string() }, system_node_ok));
    let git = run_cmd("git.exe", &["--version"]);
    let git_ok = !git.is_empty();
    items.push(item("Git（开发工具，可选）", if git_ok { git } else { "未安装".to_string() }, git_ok));
    let disk = run_ps("(Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Free -ne $null } | ForEach-Object { $_.Name + ' ' + [math]::Round($_.Free/1GB) + 'GB' }) -join ' / '");
    items.push(item("磁盘可用", disk, true));
    items.push(item("AMS 数据目录", ams_home().display().to_string(), true));
    items
}

#[tauri::command]
fn gateway_status() -> GatewayInfo {
    let home = ams_home().display().to_string();
    let gdir = gateway_dir().display().to_string();
    let installed = gateway_installed();
    let mut info = GatewayInfo {
        installed,
        running: false,
        healthy: false,
        port: 0,
        version: String::new(),
        jobs: 0,
        home,
        gateway_dir: gdir,
        message: String::new(),
    };
    let (port, _pid) = match read_ports() {
        Some(pair) => pair,
        None => {
            info.message = if installed { "未运行（无 ports.json）".to_string() } else { "Gateway 未安装（缺少 .venv）".to_string() };
            return info;
        }
    };
    info.port = port;
    info.running = true;
    let token = match gateway_token() {
        Some(t) => t,
        None => {
            info.message = "运行中但缺少 token 文件".to_string();
            return info;
        }
    };
    match http_get(port, "/api/v1/health", &token) {
        Ok(body) => {
            info.healthy = true;
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                info.version = v.get("version").and_then(|x| x.as_str()).unwrap_or("").to_string();
                info.jobs = match v.get("db").and_then(|d| d.get("jobs")) {
                    Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
                    Some(serde_json::Value::Object(map)) => map.values().filter_map(|x| x.as_u64()).sum(),
                    _ => 0,
                };
            }
            info.message = "运行中".to_string();
        }
        Err(e) => {
            info.message = format!("端口已监听但健康检查失败: {e}");
        }
    }
    info
}

fn runtime_config_path() -> PathBuf {
    ams_home().join("config").join("runtime.json")
}

fn read_runtime_config() -> RuntimeConfig {
    fs::read_to_string(runtime_config_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn runtime_status() -> RuntimeConfig { read_runtime_config() }

#[tauri::command]
fn save_runtime(rvc_python: String, rvc_root: String, pymss_python: String, pymss_root: String, msvc_tool: String) -> Result<String, String> {
    let config = RuntimeConfig {
        rvc_python: rvc_python.trim().to_string(),
        rvc_root: rvc_root.trim().to_string(),
        pymss_python: pymss_python.trim().to_string(),
        pymss_root: pymss_root.trim().to_string(),
        msvc_tool: msvc_tool.trim().to_string(),
    };
    let path = runtime_config_path();
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?; }
    fs::write(&path, serde_json::to_string_pretty(&config).map_err(|e| format!("序列化运行环境失败: {e}"))?)
        .map_err(|e| format!("保存运行环境失败: {e}"))?;
    Ok(format!("运行环境已保存到 {}；请重启 Gateway 后重新扫描音源", path.display()))
}

#[tauri::command]
fn start_gateway() -> Result<String, String> {
    let gdir = gateway_dir();
    let home = ams_home();
    fs::create_dir_all(&home).map_err(|e| format!("创建 AMS_HOME 失败: {e}"))?;
    let mut command = if gdir.join("ams-gateway.exe").is_file() {
        // Packaged component: a self-contained executable shipped under AMS_HOME/components.
        let mut c = Command::new(gdir.join("ams-gateway.exe"));
        c.current_dir(&gdir)
            .arg("--ams-home")
            .arg(&home)
            .args(["--port", "0", "--log-level", "INFO"]);
        c
    } else {
        let python = gateway_python();
        if !python.is_file() {
            return Err(format!("Gateway 未安装：{} 或 {}", gdir.join("ams-gateway.exe").display(), python.display()));
        }
        let mut c = Command::new(&python);
        c.current_dir(&gdir)
            .args(["-m", "app", "--ams-home"])
            .arg(&home)
            .args(["--port", "0", "--log-level", "INFO"]);
        c
    };
    command.env("AMS_HOME", &home);
    let temp = home.join("tmp");
    let _ = fs::create_dir_all(&temp);
    command.env("TEMP", &temp).env("TMP", &temp);
    let tts_root = home.join("runtime").join("tts").join("0.1.0");
    if tts_root.join("piper").join("piper.exe").is_file() {
        command.env("AMS_TTS_ROOT", &tts_root);
    }
    let runtime = read_runtime_config();
    if !runtime.rvc_python.trim().is_empty() { command.env("AMS_RVC_PYTHON", runtime.rvc_python.trim()); }
    if !runtime.rvc_root.trim().is_empty() { command.env("AMS_RVC_ROOT", runtime.rvc_root.trim()); }
    if !runtime.pymss_python.trim().is_empty() { command.env("AMS_PYMSS_PYTHON", runtime.pymss_python.trim()); }
    if !runtime.pymss_root.trim().is_empty() { command.env("AMS_PYMSS_ROOT", runtime.pymss_root.trim()); }
    let vcrun = ams_home().join("tools").join("vcrun").join("0.1.0");
    if vcrun.is_dir() {
        let old_path = env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{};{}", vcrun.display(), old_path));
    }
    let pymss_base = PathBuf::from(runtime.pymss_python.trim());
    if pymss_base.file_name().and_then(|v| v.to_str()) == Some("python.exe") {
        if pymss_base.parent().and_then(|p| p.file_name()).and_then(|v| v.to_str()) == Some("python") {
            if let Some(root) = pymss_base.parent().and_then(|p| p.parent()) {
                let site = root.join(".venv").join("Lib").join("site-packages");
                if site.is_dir() {
                    command.env("PYTHONHOME", pymss_base.parent().unwrap_or(&pymss_base));
                    command.env("PYTHONPATH", site);
                }
            }
        }
    }
    if !runtime.msvc_tool.trim().is_empty() { command.env("AMS_MSVC_TOOL", runtime.msvc_tool.trim()); }
    // Backend roots are user/environment configuration, never a fixed drive path.
    if let Ok(value) = env::var("AMS_RVC_PYTHON") {
        if !value.trim().is_empty() {
            command.env("AMS_RVC_PYTHON", value);
        }
    } else if let Ok(root) = env::var("AMS_RVC_ROOT") {
        let candidate = PathBuf::from(root).join("runtime").join("python.exe");
        if candidate.is_file() {
            command.env("AMS_RVC_PYTHON", candidate);
        }
    }
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let child = command.spawn().map_err(|e| format!("启动 Gateway 失败: {e}"))?;
    Ok(format!("Gateway 启动中，pid={}；端口将在 runtime/ports.json 就绪", child.id()))
}

#[tauri::command]
fn stop_gateway() -> Result<String, String> {
    let (port, pid) = match read_ports() {
        Some(pair) => pair,
        None => return Ok("Gateway 未运行".to_string()),
    };
    if pid == 0 {
        return Err("ports.json 中没有有效 pid".to_string());
    }
    let output = hidden_command("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
    match output {
        Ok(_out) => Ok(format!("已停止 Gateway（pid={pid}, port={port}）")),
        Err(e) => Err(format!("停止失败: {e}")),
    }
}

#[tauri::command]
fn component_status() -> Vec<ComponentItem> {
    let mut items = Vec::new();

    let kernel = ams_home().join("kernel");
    let kernel_exe = kernel.join("AMS Kernel.exe");
    let kernel_ok = kernel_exe.is_file() || kernel.join("package.json").is_file();
    let kernel_built = kernel_exe.is_file() || kernel.join("node_modules").is_dir();
    items.push(ComponentItem {
        name: "内核（AMS Kernel）".to_string(),
        state: if kernel_built { "已安装".to_string() } else if kernel_ok { "未完成".to_string() } else { "未安装".to_string() },
        detail: kernel.display().to_string(),
        ok: kernel_exe.is_file(),
    });

    let plugins_dir = ams_home().join("components").join("plugin.ams.music.bundle");
    let plugin_pkgs = plugins_dir.join("ams-music-bundle-0.3.0.tgz").is_file()
        || plugins_dir.join("ams-plugin-core-0.3.0.tgz").is_file();
    items.push(ComponentItem {
        name: "音乐插件".to_string(),
        state: if plugin_pkgs { "已下载".to_string() } else { "未安装".to_string() },
        detail: plugins_dir.display().to_string(),
        ok: plugin_pkgs,
    });

    let gateway = gateway_status();
    items.push(ComponentItem {
        name: "Gateway".to_string(),
        state: if gateway.healthy { "运行中".to_string() } else if gateway.installed { "已安装未运行".to_string() } else { "未安装".to_string() },
        detail: if gateway.healthy { format!("v{} · 127.0.0.1:{} · {} jobs", gateway.version, gateway.port, gateway.jobs) } else { gateway.message },
        ok: gateway.healthy,
    });

    let voices_dir = ams_home().join("voices");
    let voices_count = fs::read_dir(&voices_dir).map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).count()).unwrap_or(0);
    items.push(ComponentItem {
        name: "音源库".to_string(),
        state: if voices_count > 0 { format!("{voices_count} 个音源") } else { "空".to_string() },
        detail: voices_dir.display().to_string(),
        ok: voices_count > 0,
    });

    items
}


fn gateway_get(path: &str) -> Result<String, String> {
    let (port, _pid) = read_ports().ok_or_else(|| "Gateway 未运行（缺少 ports.json）".to_string())?;
    let token = gateway_token().ok_or_else(|| "Gateway token 缺失".to_string())?;
    http_get(port, path, &token)
}

#[tauri::command]
fn scan_voices() -> Result<Vec<VoiceInfo>, String> {
    let body = gateway_get("/api/v1/voices")?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("解析 voices JSON 失败: {e}"))?;
    let data = value.get("data").ok_or_else(|| "voices 响应缺少 data".to_string())?;
    let list = data.get("voices").and_then(|v| v.as_array()).ok_or_else(|| "voices 响应缺少 voices 数组".to_string())?;
    let mut result = Vec::new();
    for item in list {
        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let status = item.get("status").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let meta = item.get("meta");
        let info = meta.and_then(|m| m.get("info")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sr = meta.and_then(|m| m.get("sr")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let size_mb = item.get("pth").and_then(|p| p.get("size_bytes")).and_then(|v| v.as_u64()).unwrap_or(0) / (1024 * 1024);
        let has_index = item.get("index").map(|v| !v.is_null()).unwrap_or(false);
        let message = item.get("issues").and_then(|v| v.as_array()).map(|issues| {
            issues.iter().filter_map(|x| x.get("message").and_then(|m| m.as_str())).collect::<Vec<_>>().join("; ")
        }).unwrap_or_default();
        result.push(VoiceInfo { name, status, info, sr, size_mb, has_index, message });
    }
    Ok(result)
}


fn install_voicepack_zip(pack: &PathBuf, target_dir: &PathBuf) -> Result<String, String> {
    let file = File::open(pack).map_err(|e| format!("打开 voicepack 失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 voicepack 失败: {e}"))?;
    fs::create_dir_all(target_dir).map_err(|e| format!("创建音源目录失败: {e}"))?;
    let mut installed: Vec<String> = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("读取 zip 条目失败: {e}"))?;
        let Some(name) = entry.enclosed_name().and_then(|p| p.file_name().map(|n| n.to_os_string())) else { continue };
        let name_text = name.to_string_lossy().to_string();
        if !(name_text.ends_with(".pth") || name_text.ends_with(".index") || name_text == "voice.json") {
            continue;
        }
        let dest = target_dir.join(&name);
        let mut out = File::create(&dest).map_err(|e| format!("写出 {} 失败: {e}", dest.display()))?;
        copy(&mut entry, &mut out).map_err(|e| format!("解包 {} 失败: {e}", dest.display()))?;
        installed.push(name_text);
    }
    if !installed.iter().any(|n| n.ends_with(".pth")) {
        return Err("voicepack 中没有 .pth".to_string());
    }
    Ok(format!("已从 voicepack 安装默认音源：{}", installed.join(", ")))
}

fn install_voicepack_dir(source_dir: &PathBuf, target_dir: &PathBuf) -> Result<String, String> {
    let find_file = |exts: &[&str]| -> Option<PathBuf> {
        let entries = fs::read_dir(source_dir).ok()?;
        let mut candidates: Vec<PathBuf> = entries.flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().and_then(|value| value.to_str()).map(|ext| exts.contains(&ext)).unwrap_or(false))
            .collect();
        candidates.sort();
        candidates.into_iter().next()
    };
    let pth = find_file(&["pth"])
        .ok_or_else(|| format!("默认音源目录中没有 .pth：{}", source_dir.display()))?;
    fs::create_dir_all(target_dir).map_err(|e| format!("创建音源目录失败: {e}"))?;
    let target_pth = target_dir.join("曼波.pth");
    if target_pth.is_file() {
        return Ok(format!("默认音源已存在：{}", target_pth.display()));
    }
    fs::copy(&pth, &target_pth).map_err(|e| format!("复制 pth 失败: {e}"))?;
    let mut message = format!("已安装默认音源曼波（pth {:.1} MB）", fs::metadata(&target_pth).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0));
    if let Some(index) = find_file(&["index"]) {
        let target_index = target_dir.join("曼波.index");
        fs::copy(&index, &target_index).map_err(|e| format!("复制 index 失败: {e}"))?;
        message.push_str(&format!("，index {:.1} MB", fs::metadata(&target_index).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0)));
    }
    Ok(message + "；可在 Gateway 启动后扫描")
}

#[tauri::command]
fn install_default_voice_from(source: String) -> Result<String, String> {
    let source = source.trim().to_string();
    if source.is_empty() {
        return Err("请先选择 voicepack.zip 或曼波目录".to_string());
    }
    let source_path = PathBuf::from(&source);
    let target_dir = ams_home().join("voices").join("曼波");
    if target_dir.join("曼波.pth").is_file() {
        return Ok(format!("默认音源已存在：{}", target_dir.join("曼波.pth").display()));
    }
    if source_path.is_file() {
        install_voicepack_zip(&source_path, &target_dir)
    } else if source_path.is_dir() {
        install_voicepack_dir(&source_path, &target_dir)
    } else {
        Err(format!("音源路径不存在：{}", source))
    }
}

#[tauri::command]
fn install_default_voice() -> Result<String, String> {
    let target_dir = ams_home().join("voices").join("曼波");
    if target_dir.join("曼波.pth").is_file() {
        return Ok(format!("默认音源已存在：{}", target_dir.join("曼波.pth").display()));
    }
    if let Ok(pack) = env::var("AMS_DEFAULT_VOICE_PACK") {
        let pack_path = PathBuf::from(pack);
        if pack_path.is_file() {
            return install_voicepack_zip(&pack_path, &target_dir);
        }
    }
    let source = env::var("AMS_DEFAULT_VOICE_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未配置默认音源：请设置 AMS_DEFAULT_VOICE_PACK 或 AMS_DEFAULT_VOICE_DIR".to_string())?;
    install_voicepack_dir(&PathBuf::from(source), &target_dir)
}


fn runtime_dir() -> PathBuf {
    ams_home().join("runtime")
}

fn kernel_log_path() -> PathBuf {
    runtime_dir().join("kernel.log")
}

fn kernel_json_path() -> PathBuf {
    runtime_dir().join("kernel.json")
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let output = hidden_command("tasklist.exe")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output();
    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.contains(&pid.to_string())
        }
        Err(_) => false,
    }
}

fn read_kernel_url(log_path: &PathBuf) -> Option<(String, u16)> {
    let text = fs::read_to_string(log_path).ok()?;
    let mut result = None;
    for line in text.lines() {
        if let Some(pos) = line.find("dsh web: ") {
            let url = line[pos + "dsh web: ".len()..].trim().to_string();
            if url.starts_with("http://") {
                if let Some(port) = url
                    .split("//")
                    .nth(1)
                    .and_then(|rest| rest.split(&['/', '?'][..]).next())
                    .and_then(|authority| authority.rsplit(':').next())
                    .and_then(|p| p.parse::<u16>().ok())
                {
                    result = Some((url, port));
                }
            }
        }
    }
    result
}

fn write_kernel_json(pid: u32, url: &str, port: u16) {
    let value = serde_json::json!({
        "pid": pid,
        "url": url,
        "port": port,
        "dsh_home": ams_home().join("kernel").join("dsh").display().to_string(),
        "log": kernel_log_path().display().to_string()
    });
    let _ = fs::create_dir_all(runtime_dir());
    let _ = fs::write(kernel_json_path(), serde_json::to_string_pretty(&value).unwrap_or_default());
}

fn packaged_kernel_exe() -> Option<PathBuf> {
    let root = ams_home().join("kernel");
    let candidates = [
        root.join("AMS Kernel.exe"),
        root.join("DeepSeek Harness.exe"),
        root.join("AMS Studio.exe"),
        root.join("AMS Studio Kernel.exe"),
        root.join("ams-kernel.exe"),
        root.join("dsh.exe"),
        root.join("win-unpacked").join("DeepSeek Harness.exe"),
        root.join("app").join("DeepSeek Harness.exe"),
        root.join("dsh").join("DeepSeek Harness.exe"),
        root.join("dsh").join("win-unpacked").join("DeepSeek Harness.exe"),
    ];
    for path in candidates {
        if path.is_file() {
            return Some(path);
        }
    }
    // Last resort: shallow scan for an exe whose name mentions Harness/AMS/kernel.
    fn scan(dir: &PathBuf, depth: u8) -> Option<PathBuf> {
        if depth == 0 { return None; }
        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = scan(&path, depth - 1) { return Some(found); }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_lowercase();
                if lower.ends_with(".exe") && (lower.contains("harness") || lower.contains("ams") || lower.contains("kernel")) {
                    return Some(path);
                }
            }
        }
        None
    }
    scan(&root, 3)
}

#[tauri::command]
fn kernel_status() -> KernelInfo {
    let dsh_home = ams_home().join("kernel").join("dsh").display().to_string();
    let packaged = packaged_kernel_exe();
    let dev_installed = dev_root().join("ams-harness").join("package.json").is_file();
    let mut info = KernelInfo {
        installed: packaged.is_some() || dev_installed,
        running: false,
        starting: false,
        url: String::new(),
        port: 0,
        pid: 0,
        dsh_home,
        log: kernel_log_path().display().to_string(),
        message: String::new(),
    };
    let log_path = kernel_log_path();
    if let Some((url, port)) = read_kernel_url(&log_path) {
        info.url = url;
        info.port = port;
    }
    let json_path = kernel_json_path();
    if let Ok(text) = fs::read_to_string(&json_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            info.pid = value.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            if info.url.is_empty() {
                info.url = value.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
            if info.port == 0 {
                info.port = value.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            }
        }
    }
    if info.pid > 0 && process_alive(info.pid) {
        info.running = true;
        info.starting = !packaged.is_none() && info.url.is_empty();
        if packaged.is_some() {
            info.message = "运行中（打包内核）".to_string();
        } else if info.starting {
            info.message = "内核启动中（等待 token URL）".to_string();
        } else {
            info.message = "运行中".to_string();
            write_kernel_json(info.pid, &info.url, info.port);
        }
    } else if info.pid > 0 {
        info.message = "进程已退出（可重新启动）".to_string();
    } else if !info.installed {
        info.message = "内核未安装（缺少 ams-harness）".to_string();
    } else {
        info.message = "未运行".to_string();
    }
    info
}

#[tauri::command]
fn start_kernel() -> Result<String, String> {
    let current = kernel_status();
    if current.pid > 0 && process_alive(current.pid) {
        return Ok(format!("内核已在运行（pid={}）", current.pid));
    }
    let dsh_home = ams_home().join("kernel").join("dsh");
    fs::create_dir_all(&dsh_home).map_err(|e| format!("创建 DSH_HOME 失败: {e}"))?;
    ensure_profile_base_patch(&dsh_home)?;
    fs::create_dir_all(runtime_dir()).map_err(|e| format!("创建 runtime 失败: {e}"))?;
    let log = fs::File::create(kernel_log_path()).map_err(|e| format!("创建日志失败: {e}"))?;
    let log_err = log.try_clone().map_err(|e| format!("复制日志句柄失败: {e}"))?;
    if let Some(config) = read_connection() {
        if !config.base_url.trim().is_empty() && !config.api_key.trim().is_empty() {
            apply_connection_config(&config)?;
        }
    }

    if let Some(exe) = packaged_kernel_exe() {
        let electron_data = ams_home().join("kernel").join("electron-data");
        fs::create_dir_all(&electron_data).map_err(|e| format!("创建 electron userData 失败: {e}"))?;
        let mut command = Command::new(&exe);
        command
            .current_dir(ams_home().join("kernel"))
            .arg("--user-data-dir")
            .arg(&electron_data)
            .env("DSH_HOME", &dsh_home)
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log_err));
        for (key, _) in std::env::vars_os() {
            if let Some(k) = key.to_str() {
                if k.starts_with("DSH_") || k.starts_with("AMS_") || k == "NODE_OPTIONS" || k == "NODE_PATH" || k == "ELECTRON_RUN_AS_NODE" {
                    command.env_remove(k);
                }
            }
        }
        command.env("DSH_HOME", &dsh_home);
        let temp = ams_home().join("tmp");
        let _ = fs::create_dir_all(&temp);
        command.env("TEMP", &temp).env("TMP", &temp);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let child = command.spawn().map_err(|e| format!("启动打包内核失败: {e}"))?;
        write_kernel_json(child.id(), "", 0);
        return Ok(format!("打包内核已启动，pid={}；exe={}", child.id(), exe.display()));
    }

    let harness = dev_root().join("ams-harness");
    if !harness.join("package.json").is_file() {
        return Err(format!("找不到内核源码：{}", harness.display()));
    }
    let mut command = if let Some(corepack) = find_in_path("corepack.cmd") {
        let mut c = Command::new(corepack);
        c.args(["pnpm", "dsh", "web", "--no-open", "--port", "0"]);
        c
    } else {
        let mut c = Command::new("cmd.exe");
        c.args(["/C", "corepack pnpm dsh web --no-open --port 0"]);
        c
    };
    command
        .current_dir(&harness)
        .env("DSH_HOME", &dsh_home)
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let child = command.spawn().map_err(|e| format!("启动内核失败: {e}"))?;
    write_kernel_json(child.id(), "", 0);
    Ok(format!("内核启动中，pid={}；日志：{}", child.id(), kernel_log_path().display()))
}

#[tauri::command]
fn stop_kernel() -> Result<String, String> {
    let info = kernel_status();
    if info.pid == 0 || !process_alive(info.pid) {
        let _ = fs::remove_file(kernel_json_path());
        return Ok("内核未运行".to_string());
    }
    let output = hidden_command("taskkill.exe")
        .args(["/PID", &info.pid.to_string(), "/T", "/F"])
        .output();
    let _ = fs::remove_file(kernel_json_path());
    match output {
        Ok(_) => Ok(format!("已停止内核（pid={}）", info.pid)),
        Err(e) => Err(format!("停止失败: {e}")),
    }
}


fn http_post(port: u16, path: &str, token: &str, body: &str) -> Result<String, String> {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().map_err(|e| format!("地址解析失败: {e}"))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("连接失败: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("发送失败: {e}"))?;
    let mut buffer = String::new();
    stream.read_to_string(&mut buffer).map_err(|e| format!("读取失败: {e}"))?;
    let status_line = buffer.lines().next().unwrap_or("");
    let body_text = buffer.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    if !(status_line.contains(" 200 ") || status_line.contains(" 201 ")) {
        return Err(format!("HTTP 失败: {status_line} {body_text}"));
    }
    Ok(body_text)
}

#[tauri::command]
fn submit_job(job_type: String, params: serde_json::Value) -> Result<String, String> {
    let (port, _pid) = read_ports().ok_or_else(|| "Gateway 未运行（缺少 ports.json）".to_string())?;
    let token = gateway_token().ok_or_else(|| "Gateway token 缺失".to_string())?;
    let payload = serde_json::json!({ "type": job_type, "params": params }).to_string();
    let body = http_post(port, "/api/v1/jobs", &token, &payload)?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("解析任务响应失败: {e}"))?;
    let id = value.get("id").and_then(|v| v.as_str()).ok_or_else(|| format!("任务响应缺少 id: {body}"))?;
    Ok(id.to_string())
}

#[tauri::command]
fn job_status(job_id: String) -> Result<JobInfo, String> {
    let (port, _pid) = read_ports().ok_or_else(|| "Gateway 未运行".to_string())?;
    let token = gateway_token().ok_or_else(|| "Gateway token 缺失".to_string())?;
    let body = http_get(port, &format!("/api/v1/jobs/{job_id}"), &token)?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("解析任务失败: {e}"))?;
    let error = value.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("").to_string();
    Ok(JobInfo {
        id: value.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        job_type: value.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        status: value.get("status").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        progress: value.get("progress").and_then(|v| v.as_f64()).unwrap_or(0.0),
        stage: value.get("stage").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        error,
    })
}

#[tauri::command]
fn read_update_manifest(path: String) -> Result<UpdateManifest, String> {
    let text = fs::read_to_string(&path).map_err(|e| format!("读取清单失败: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON: {e}"))?;
    let components = value
        .get("components")
        .and_then(|v| v.as_array())
        .map(|items| {
            items.iter().map(|item| UpdateComponent {
                id: item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                kind: item.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                version: item.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                size: item.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
            }).collect()
        })
        .unwrap_or_default();
    Ok(UpdateManifest {
        schema: value.get("schema").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        channel: value.get("channel").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        sequence: value.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0),
        signature_present: value.get("signature").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false),
        components,
        path,
    })
}


#[derive(Serialize, Clone)]
struct UpdateVerify {
    valid: bool,
    message: String,
    sequence: u64,
    components: u32,
}

fn canonical_manifest_bytes(value: &serde_json::Value) -> Result<Vec<u8>, String> {
    let mut object = value.as_object().ok_or_else(|| "清单根必须是对象".to_string())?.clone();
    object.remove("signature");
    serde_json::to_string(&serde_json::Value::Object(object))
        .map(|text| text.into_bytes())
        .map_err(|e| format!("规范化清单失败: {e}"))
}

#[tauri::command]
fn verify_update_manifest(path: String, public_key: Option<String>) -> Result<UpdateVerify, String> {
    let text = fs::read_to_string(&path).map_err(|e| format!("读取清单失败: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("清单不是合法 JSON: {e}"))?;
    let signature_text = value
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "清单缺少 signature".to_string())?;
    let signature_text = signature_text
        .strip_prefix("ed25519:")
        .ok_or_else(|| "signature 格式不是 ed25519:<base64>".to_string())?;
    let signature_bytes = BASE64.decode(signature_text).map_err(|e| format!("signature base64 非法: {e}"))?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|e| format!("签名长度非法: {e}"))?;
    let key_path = public_key.unwrap_or_else(|| {
        env::var("AMS_UPDATE_PUBLIC_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| ams_home().join("runtime").join("update-public.pem").display().to_string())
    });
    let key_text = fs::read_to_string(&key_path).map_err(|e| format!("读取公钥失败（{key_path}）: {e}"))?;
    let key = VerifyingKey::from_public_key_pem(&key_text).map_err(|e| format!("解析公钥失败: {e}"))?;
    let payload = canonical_manifest_bytes(&value)?;
    match key.verify_strict(&payload, &signature) {
        Ok(()) => Ok(UpdateVerify {
            valid: true,
            message: "签名有效".to_string(),
            sequence: value.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0),
            components: value.get("components").and_then(|v| v.as_array()).map(|a| a.len() as u32).unwrap_or(0),
        }),
        Err(_) => Ok(UpdateVerify {
            valid: false,
            message: "签名无效（清单被篡改或公钥不匹配）".to_string(),
            sequence: value.get("sequence").and_then(|v| v.as_u64()).unwrap_or(0),
            components: value.get("components").and_then(|v| v.as_array()).map(|a| a.len() as u32).unwrap_or(0),
        }),
    }
}


const COMPONENT_MANIFEST_URL: &str =
    "https://github.com/yan-mc/ams-release/releases/latest/download/ams-components.json";
const UPDATES_PUBLIC_KEY: &str = include_str!("../keys/updates-public.pem");

#[derive(Serialize, Clone)]
struct ComponentPart {
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize, Clone)]
struct ComponentAsset {
    id: String,
    kind: String,
    version: String,
    url: String,
    size: u64,
    sha256: String,
    target: Option<String>,
    parts: Vec<ComponentPart>,
}

#[derive(Serialize, Clone)]
struct ComponentResult {
    id: String,
    ok: bool,
    message: String,
    path: String,
    size: u64,
}

fn component_manifest_source() -> String {
    env::var("AMS_COMPONENTS_FEED").unwrap_or_else(|_| COMPONENT_MANIFEST_URL.to_string())
}

fn read_text_source(source: &str) -> Result<String, String> {
    if source.starts_with("http://") || source.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .user_agent("ams-launcher/0.2.0")
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
        client
            .get(source)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.text())
            .map_err(|e| format!("读取组件清单失败（{source}）: {e}"))
    } else {
        let raw = source.strip_prefix("file://").unwrap_or(source);
        let path = raw.trim_start_matches('/');
        fs::read_to_string(path).map_err(|e| format!("读取本地组件清单失败（{path}）: {e}"))
    }
}

fn fetch_component_manifest() -> Result<serde_json::Value, String> {
    let source = component_manifest_source();
    let text = read_text_source(&source)?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("组件清单不是 JSON: {e}"))?;
    let signature = value.get("signature").and_then(|v| v.as_str()).ok_or_else(|| "组件清单缺少 signature".to_string())?;
    let signature = signature.strip_prefix("ed25519:").ok_or_else(|| "signature 前缀非法".to_string())?;
    let signature_bytes = BASE64.decode(signature).map_err(|e| format!("signature base64 非法: {e}"))?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|e| format!("signature 长度非法: {e}"))?;
    let key = VerifyingKey::from_public_key_pem(UPDATES_PUBLIC_KEY).map_err(|e| format!("内置公钥解析失败: {e}"))?;
    let payload = canonical_manifest_bytes(&value)?;
    key.verify_strict(&payload, &signature)
        .map_err(|_| "组件清单签名校验失败".to_string())?;
    Ok(value)
}

fn component_assets(value: &serde_json::Value) -> Vec<ComponentAsset> {
    value
        .get("components")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .map(|item| ComponentAsset {
                    id: item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    kind: item.get("kind").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    version: item.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    url: item.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    size: item.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                    sha256: item.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    target: item.get("target").and_then(|v| v.as_str()).map(|v| v.to_string()),
                    parts: item.get("parts").and_then(|v| v.as_array()).map(|parts| {
                        parts.iter().map(|part| ComponentPart {
                            url: part.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            size: part.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
                            sha256: part.get("sha256").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                        }).filter(|part| !part.url.is_empty()).collect()
                    }).unwrap_or_default(),
                })
                .filter(|item| !item.id.is_empty() && (!item.url.is_empty() || !item.parts.is_empty()))
                .collect()
        })
        .unwrap_or_default()
}

fn sha256_of(path: &PathBuf) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| format!("读取文件失败: {e}"))?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn extract_zip(zip_path: &PathBuf, target: &PathBuf) -> Result<u64, String> {
    let file = File::open(zip_path).map_err(|e| format!("打开 zip 失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("读取 zip 失败: {e}"))?;
    fs::create_dir_all(target).map_err(|e| format!("创建组件目录失败: {e}"))?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("读取 zip 条目失败: {e}"))?;
        let Some(relative) = entry.enclosed_name() else { continue };
        let dest = target.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| format!("创建目录失败: {e}"))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {e}"))?;
        }
        let mut out = File::create(&dest).map_err(|e| format!("写出失败: {e}"))?;
        total += copy(&mut entry, &mut out).map_err(|e| format!("解包失败: {e}"))?;
    }
    Ok(total)
}

fn download_asset_to(asset: &ComponentAsset, dest: &PathBuf) -> Result<u64, String> {
    if asset.url.starts_with("http://") || asset.url.starts_with("https://") {
        let client = reqwest::blocking::Client::builder()
            .user_agent("ams-launcher/0.2.0")
            .timeout(Duration::from_secs(1800))
            .build()
            .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
        let mut response = client
            .get(&asset.url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("下载 {} 失败: {e}", asset.id))?;
        let mut file = File::create(dest).map_err(|e| format!("创建下载文件失败: {e}"))?;
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let n = response.read(&mut buffer).map_err(|e| format!("读取下载流失败: {e}"))?;
            if n == 0 { break; }
            file.write_all(&buffer[..n]).map_err(|e| format!("写入下载文件失败: {e}"))?;
            INSTALL_DOWNLOADED.fetch_add(n as u64, Ordering::Relaxed);
        }
    } else {
        let raw = asset.url.strip_prefix("file://").unwrap_or(&asset.url);
        let src = raw.trim_start_matches('/');
        let copied = fs::copy(src, dest).map_err(|e| format!("复制本地组件失败（{src}）: {e}"))?;
        INSTALL_DOWNLOADED.fetch_add(copied, Ordering::Relaxed);
    }
    Ok(fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
}

fn download_parts_to(asset: &ComponentAsset, downloads: &PathBuf) -> Result<PathBuf, String> {
    let zip_path = downloads.join(format!("{}-{}.zip", asset.id, asset.version));
    let mut out = File::create(&zip_path).map_err(|e| format!("创建分卷目标文件失败: {e}"))?;
    for (index, part) in asset.parts.iter().enumerate() {
        let part_path = downloads.join(format!("{}-{}-part-{}.bin", asset.id, asset.version, index + 1));
        let part_asset = ComponentAsset {
            id: format!("{}.part{}", asset.id, index + 1),
            kind: "part".to_string(),
            version: asset.version.clone(),
            url: part.url.clone(),
            size: part.size,
            sha256: part.sha256.clone(),
            target: None,
            parts: Vec::new(),
        };
        let size = download_asset_to(&part_asset, &part_path)?;
        if part.size > 0 && size != part.size {
            return Err(format!("{} 第 {} 卷大小不匹配：期望 {}，实际 {}", asset.id, index + 1, part.size, size));
        }
        let sha = sha256_of(&part_path)?;
        if !part.sha256.is_empty() && !sha.eq_ignore_ascii_case(&part.sha256) {
            return Err(format!("{} 第 {} 卷 sha256 不匹配", asset.id, index + 1));
        }
        let mut input = File::open(&part_path).map_err(|e| format!("打开分卷失败: {e}"))?;
        copy(&mut input, &mut out).map_err(|e| format!("合并分卷失败: {e}"))?;
    }
    Ok(zip_path)
}

fn component_target(asset: &ComponentAsset) -> Result<PathBuf, String> {
    if let Some(target) = &asset.target {
        let normalized = target.replace('\\', "/");
        if normalized.starts_with('/') || normalized.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(format!("组件 target 非法：{target}"));
        }
        return Ok(ams_home().join(target));
    }
    Ok(if asset.kind == "kernel" {
        ams_home().join("kernel")
    } else if asset.kind == "asset" {
        ams_home().join("assets").join(&asset.id)
    } else {
        ams_home().join("components").join(&asset.id)
    })
}

fn apply_runtime_component_config(asset: &ComponentAsset, target: &PathBuf) -> Result<(), String> {
    let mut config = read_runtime_config();
    let changed = match asset.id.as_str() {
        "runtime.rvc" => {
            config.rvc_root = target.display().to_string();
            config.rvc_python = target.join("runtime").join("python.exe").display().to_string();
            true
        }
        "runtime.pymss" => {
            config.pymss_root = target.display().to_string();
            config.pymss_python = target.join(".venv").join("Scripts").join("python.exe").display().to_string();
            true
        }
        "runtime.pymss.python" => {
            config.pymss_root = target.parent().map(|p| p.display().to_string()).unwrap_or_default();
            config.pymss_python = target.join("python.exe").display().to_string();
            true
        }
        "tool.msvc" | "tool.msvc2pth" => {
            config.msvc_tool = target.join("msvc2pth.py").display().to_string();
            true
        }
        _ => false,
    };
    if !changed { return Ok(()); }
    let path = runtime_config_path();
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| format!("创建运行环境配置目录失败: {e}"))?; }
    fs::write(&path, serde_json::to_string_pretty(&config).map_err(|e| format!("序列化运行环境失败: {e}"))?)
        .map_err(|e| format!("保存运行环境失败: {e}"))?;
    Ok(())
}
fn install_component_blocking(asset: &ComponentAsset) -> ComponentResult {
    let result = (|| -> Result<(String, u64), String> {
        let downloads = ams_home().join("components").join("downloads");
        let target = component_target(asset)?;
        fs::create_dir_all(&downloads).map_err(|e| format!("创建下载目录失败: {e}"))?;
        let zip_path = if asset.parts.is_empty() {
            let path = downloads.join(format!("{}-{}.zip", asset.id, asset.version));
            let size = download_asset_to(asset, &path)?;
            (path, size)
        } else {
            let path = download_parts_to(asset, &downloads)?;
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            (path, size)
        };
        let (zip_path, size) = zip_path;
        if asset.size > 0 && size != asset.size {
            return Err(format!("{} 大小不匹配：期望 {}，实际 {}", asset.id, asset.size, size));
        }
        let sha = sha256_of(&zip_path)?;
        if !sha.eq_ignore_ascii_case(&asset.sha256) {
            return Err(format!("{} sha256 不匹配：期望 {}，实际 {}", asset.id, asset.sha256, sha));
        }
        let extracted = extract_zip(&zip_path, &target)?;
        apply_runtime_component_config(asset, &target)?;
        if asset.id == "asset.voicepack.manbo" {
            let voices = ams_home().join("voices").join("曼波");
            let voice_message = install_voicepack_dir(&target, &voices)?;
            return Ok((format!("已下载曼波资产并安装到 voices：{}", voice_message), size));
        }
        Ok((format!("已安装 {} {}（{} B 下载，{} B 解包）", asset.id, asset.version, size, extracted), size))
    })();
    match result {
        Ok((message, size)) => ComponentResult {
            id: asset.id.clone(),
            ok: true,
            message,
            path: component_target(asset).map(|p| p.display().to_string()).unwrap_or_default(),
            size,
        },
        Err(message) => ComponentResult {
            id: asset.id.clone(),
            ok: false,
            message,
            path: String::new(),
            size: 0,
        },
    }
}

#[tauri::command]
async fn fetch_components() -> Result<Vec<ComponentAsset>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let value = fetch_component_manifest()?;
        Ok(component_assets(&value))
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn install_components(ids: Option<Vec<String>>) -> Result<Vec<ComponentResult>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let value = fetch_component_manifest()?;
        let mut assets = Vec::new();
        for asset in component_assets(&value) {
            if let Some(ref filter) = ids {
                if !filter.iter().any(|id| id == &asset.id) {
                    continue;
                }
            }
            assets.push(asset);
        }
        let total: u64 = assets.iter().map(|asset| {
            if asset.size > 0 { asset.size } else { asset.parts.iter().map(|part| part.size).sum() }
        }).sum();
        INSTALL_DOWNLOADED.store(0, Ordering::Relaxed);
        INSTALL_TOTAL.store(total, Ordering::Relaxed);
        INSTALL_ACTIVE.store(true, Ordering::Relaxed);
        set_install_current("准备下载组件");
        let mut results = Vec::new();
        for asset in assets {
            set_install_current(format!("正在下载并安装 {} {}", asset.id, asset.version));
            results.push(install_component_blocking(&asset));
        }
        ensure_data_layout();
        set_install_current("安装完成");
        INSTALL_ACTIVE.store(false, Ordering::Relaxed);
        Ok(results)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
fn install_progress() -> InstallProgress {
    let total = INSTALL_TOTAL.load(Ordering::Relaxed);
    let downloaded = INSTALL_DOWNLOADED.load(Ordering::Relaxed);
    let percent = if total == 0 { 0.0 } else { ((downloaded.min(total) as f64) * 100.0 / total as f64).min(100.0) };
    let current = install_current().lock().map(|value| value.clone()).unwrap_or_default();
    InstallProgress {
        active: INSTALL_ACTIVE.load(Ordering::Relaxed),
        downloaded,
        total,
        percent,
        current,
    }
}

#[tauri::command]
fn install_music_plugins() -> Result<String, String> {
    let home = ams_home();
    let dsh_home = home.join("kernel").join("dsh");
    let profile = dsh_home.join("profiles").join("desktop");
    if !profile.is_dir() {
        return Err("内核 profile 尚未初始化：请先启动一次 AMS Kernel，再停止后安装插件".to_string());
    }
    let status = kernel_status();
    if status.pid > 0 && process_alive(status.pid) {
        return Err("请先停止 AMS Kernel，再安装音乐插件".to_string());
    }
    let dsh_cmd = home
        .join("kernel")
        .join("resources")
        .join("runtime")
        .join("cli")
        .join("bin")
        .join("dsh.cmd");
    if !dsh_cmd.is_file() {
        return Err(format!("找不到 DSH CLI：{}", dsh_cmd.display()));
    }
    // The packaged CLI launcher must target AMS Kernel.exe, not upstream.
    if let Ok(text) = fs::read_to_string(&dsh_cmd) {
        if text.contains("DeepSeek Harness.exe") {
            let _ = fs::write(&dsh_cmd, text.replace("DeepSeek Harness.exe", "AMS Kernel.exe"));
        }
    }
    let plugin_dir = home.join("components").join("plugin.ams.music.bundle");
    let tarballs = [
        "ams-plugin-core-0.3.0.tgz",
        "ams-plugin-ui-0.3.0.tgz",
        "ams-music-bundle-0.3.0.tgz",
    ];
    for name in tarballs {
        if !plugin_dir.join(name).is_file() {
            return Err(format!("插件组件不完整，缺少：{}", plugin_dir.join(name).display()));
        }
    }
    for name in tarballs {
        let tarball = plugin_dir.join(name);
        let mut command = hidden_command("cmd.exe");
        command
            .current_dir(&home)
            .arg("/c")
            .arg(&dsh_cmd)
            .args(["plugin", "--profile", "desktop", "add"])
            .arg(&tarball)
            .env("DSH_HOME", &dsh_home)
            .env("DSH_PROFILE", "desktop")
            .env("DSH_PROFILE_DIR", &profile);
        for (key, _) in env::vars_os() {
            if let Some(k) = key.to_str() {
                if k.starts_with("DSH_") || k.starts_with("AMS_") {
                    command.env_remove(k);
                }
            }
        }
        let temp = ams_home().join("tmp");
        let _ = fs::create_dir_all(&temp);
        command.env("DSH_HOME", &dsh_home).env("DSH_PROFILE", "desktop").env("DSH_PROFILE_DIR", &profile).env("TEMP", &temp).env("TMP", &temp);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let output = command.output().map_err(|e| format!("调用 DSH plugin 失败: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Err(format!("安装 {} 失败：{}{}", name, stdout, stderr));
        }
    }
    let modules = profile.join("node_modules").join("@ams");
    for name in ["plugin-core", "plugin-ui", "music-bundle"] {
        if !modules.join(name).is_dir() {
            return Err(format!("插件安装后缺少模块：{}", modules.join(name).display()));
        }
    }
    Ok(format!("音乐插件已安装到 {}；请重启 AMS Kernel", profile.display()))
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn cli_trace(line: &str) {
    let logs = ams_home().join("logs");
    let _ = fs::create_dir_all(&logs);
    let path = logs.join("launcher-cli-trace.log");
    let mut current = fs::read_to_string(&path).unwrap_or_default();
    current.push_str(line);
    current.push('\n');
    let _ = fs::write(path, current);
}

fn cli_install_components() -> Result<(), String> {
    cli_trace("start fetch manifest");
    let value = fetch_component_manifest()?;
    cli_trace("manifest ok");

    let assets = component_assets(&value);
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("feed components: {}", assets.len()));
    for asset in &assets {
        cli_trace(&format!("installing {} {} {}", asset.id, asset.kind, asset.version));
        let result = install_component_blocking(asset);
        cli_trace(&format!("installed {} ok={}", asset.id, result.ok));
        lines.push(format!("{} {}({}) -> ok={} size={} path={} msg={}", asset.id, asset.kind, asset.version, result.ok, result.size, result.path, result.message));
    }
    ensure_data_layout();
    let logs = ams_home().join("logs");
    fs::create_dir_all(&logs).ok();
    let log_path = logs.join("launcher-cli.log");
    let _ = fs::write(&log_path, lines.join("\n") + "\n");
    let failed = lines.iter().any(|line| line.contains("ok=false"));
    if failed {
        Err(format!("component install had failures; see {}", log_path.display()))
    } else {
        Ok(())
    }
}

fn configure_process_storage() {
    let home = ams_home();
    let temp = home.join("tmp");
    let appdirs = home.join("runtime").join("appdirs");
    let _ = fs::create_dir_all(&temp);
    let _ = fs::create_dir_all(&appdirs);
    // Process-local only; no persistent user/machine environment is changed.
    #[cfg(windows)]
    unsafe {
        env::set_var("TEMP", &temp);
        env::set_var("TMP", &temp);
        env::set_var("WEBVIEW2_USER_DATA_FOLDER", &appdirs);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_process_storage();
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--uninstall-cleanup") {
        let result = cleanup_ams_data();
        let line = match &result {
            Ok(message) => format!("cleanup ok: {message}"),
            Err(message) => format!("cleanup failed: {message}"),
        };
        // Keep this diagnostic inside the install directory as well.
        if let Some(parent) = ams_home().parent() {
            let _ = fs::write(parent.join("uninstall-result.log"), line);
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let _ = ensure_ams_home_marker();
    ensure_data_layout();
    if args.iter().any(|arg| arg == "--start-kernel") {
        let result = start_kernel();
        let logs = ams_home().join("logs");
        fs::create_dir_all(&logs).ok();
        let line = match &result {
            Ok(message) => format!("start_kernel ok: {message}"),
            Err(message) => format!("start_kernel failed: {message}"),
        };
        let _ = fs::write(logs.join("launcher-start-kernel.log"), line);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    if args.iter().any(|arg| arg == "--install-components") {
        let code = match cli_install_components() {
            Ok(()) => 0,
            Err(message) => {
                let logs = ams_home().join("logs");
                fs::create_dir_all(&logs).ok();
                let _ = fs::write(logs.join("launcher-cli-error.log"), message);
                1
            }
        };
        std::process::exit(code);
    }
    let mut context = tauri::generate_context!();
    context.config_mut().app.app_directories_override = Some(
        tauri::utils::config::AppDirectoriesOverride::Root(
            ams_home().join("runtime").join("appdirs")
        )
    );
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_environment,
            gateway_status,
            start_gateway,
            stop_gateway,
            component_status,
            scan_voices,
            install_default_voice,
            install_default_voice_from,
            runtime_status,
            save_runtime,
            kernel_status,
            start_kernel,
            stop_kernel,
            submit_job,
            job_status,
            read_update_manifest,
            verify_update_manifest,
            fetch_components,
            install_components,
            install_progress,
            open_data_folder,
            install_music_plugins,
            app_version,
            open_uninstaller,
            connection_status,
            save_connection
        ])
        .run(context)
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_refuses_unowned_custom_home() {
        let temp = std::env::temp_dir().join("ams-uninstall-safety-test");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("keep.txt"), b"keep").unwrap();
        unsafe { std::env::set_var("AMS_HOME", &temp) };
        let result = cleanup_ams_data();
        unsafe { std::env::remove_var("AMS_HOME") };
        assert!(result.is_err(), "custom home without marker must be refused");
        assert!(temp.join("keep.txt").is_file(), "unowned file must survive");
        let _ = fs::remove_dir_all(&temp);
    }
}
