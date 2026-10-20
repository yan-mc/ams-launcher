use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ed25519_dalek::pkcs8::DecodePublicKey;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::copy;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    let output = hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
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

fn ams_home() -> PathBuf {
    if let Ok(value) = env::var("AMS_HOME") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    let local = env::var("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local).join("AMS")
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
    let default_home = env::var("LOCALAPPDATA")
        .ok()
        .map(|local| PathBuf::from(local).join("AMS"))
        .and_then(|p| fs::canonicalize(p).ok());
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
    let log_path = env::temp_dir().join("ams-uninstall.log");
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
    let expected = env::var("LOCALAPPDATA")
        .ok()
        .map(|local| PathBuf::from(local).join("AMS Studio"));
    let canonical_dir = fs::canonicalize(install_dir).map_err(|e| format!("解析安装目录失败: {e}"))?;
    let expected_ok = expected
        .and_then(|p| fs::canonicalize(p).ok())
        .map(|p| canonical_lower(&p) == canonical_lower(&canonical_dir))
        .unwrap_or(false);
    if !expected_ok {
        return Err(format!(
            "拒绝启动卸载程序：当前目录不是预期的 AMS Studio 安装目录（{}）",
            canonical_dir.display()
        ));
    }
    Command::new(&uninstaller)
        .spawn()
        .map_err(|e| format!("启动卸载程序失败: {e}"))?;
    Ok("已打开卸载程序；清理只针对 AMS 自己的数据目录".to_string())
}

fn dev_root() -> PathBuf {
    if let Ok(value) = env::var("AMS_DEV_ROOT") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    PathBuf::from("F:\\Deepseek_harness\\AI音乐处理\\08_项目代码")
}

fn gateway_dir() -> PathBuf {
    dev_root().join("ams-services").join("gateway")
}

fn gateway_python() -> PathBuf {
    gateway_dir().join(".venv").join("Scripts").join("python.exe")
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
    let python = {
        let v = run_cmd("py.exe", &["-3", "--version"]);
        if v.is_empty() { run_cmd("python.exe", &["--version"]) } else { v }
    };
    items.push(item("Python", python, true));
    let node = run_cmd("node.exe", &["-v"]);
    let node_ok = !node.is_empty();
    items.push(item("Node.js", if node_ok { node } else { "未安装".to_string() }, node_ok));
    let ffmpeg = first_line(&run_cmd("ffmpeg.exe", &["-version"]));
    let ffmpeg_ok = !ffmpeg.is_empty();
    items.push(item("FFmpeg", if ffmpeg_ok { ffmpeg } else { "未安装".to_string() }, ffmpeg_ok));
    let git = run_cmd("git.exe", &["--version"]);
    let git_ok = !git.is_empty();
    items.push(item("Git", if git_ok { git } else { "未安装".to_string() }, git_ok));
    let disk = run_ps("(Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Free -ne $null } | ForEach-Object { $_.Name + ' ' + [math]::Round($_.Free/1GB) + 'GB' }) -join ' / '");
    items.push(item("磁盘可用", disk, true));
    items.push(item("AMS 数据目录", ams_home().display().to_string(), true));
    items
}

#[tauri::command]
fn gateway_status() -> GatewayInfo {
    let home = ams_home().display().to_string();
    let gdir = gateway_dir().display().to_string();
    let installed = gateway_python().is_file();
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

#[tauri::command]
fn start_gateway() -> Result<String, String> {
    let python = gateway_python();
    if !python.is_file() {
        return Err(format!("找不到 Gateway Python：{}", python.display()));
    }
    let gdir = gateway_dir();
    let home = ams_home();
    fs::create_dir_all(&home).map_err(|e| format!("创建 AMS_HOME 失败: {e}"))?;
    let mut command = Command::new(&python);
    command
        .current_dir(&gdir)
        .args(["-m", "app", "--ams-home"])
        .arg(&home)
        .args(["--port", "0", "--log-level", "INFO"])
        .env("AMS_HOME", &home);
    if let Ok(value) = env::var("AMS_RVC_PYTHON") {
        if !value.trim().is_empty() {
            command.env("AMS_RVC_PYTHON", value);
        }
    } else {
        let default_rvc = PathBuf::from("F:\\ai歌曲\\RVC20260311\\runtime\\python.exe");
        if default_rvc.is_file() {
            command.env("AMS_RVC_PYTHON", default_rvc);
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
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            Ok(format!("已停止 Gateway（pid={pid}, port={port}）：{text}"))
        }
        Err(e) => Err(format!("停止失败: {e}")),
    }
}

#[tauri::command]
fn component_status() -> Vec<ComponentItem> {
    let root = dev_root();
    let mut items = Vec::new();

    let kernel = root.join("ams-harness");
    let kernel_ok = kernel.join("package.json").is_file();
    let kernel_built = kernel.join("node_modules").is_dir();
    items.push(ComponentItem {
        name: "内核（DSH fork）".to_string(),
        state: if kernel_built { "已构建".to_string() } else if kernel_ok { "已安装未构建".to_string() } else { "未安装".to_string() },
        detail: kernel.display().to_string(),
        ok: kernel_ok,
    });

    let plugins_dir = root.join("ams-plugins");
    let plugin_pkgs = plugins_dir.join("packages").is_dir();
    items.push(ComponentItem {
        name: "音乐插件".to_string(),
        state: if plugin_pkgs { "源码就绪".to_string() } else { "未安装".to_string() },
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
    let pth = source_dir.join("manbo.pth");
    if !pth.is_file() {
        return Err(format!("默认音源文件不存在：{}（可用 AMS_DEFAULT_VOICE_PACK 指定 voicepack.zip）", pth.display()));
    }
    fs::create_dir_all(target_dir).map_err(|e| format!("创建音源目录失败: {e}"))?;
    let target_pth = target_dir.join("曼波.pth");
    if target_pth.is_file() {
        return Ok(format!("默认音源已存在：{}", target_pth.display()));
    }
    fs::copy(&pth, &target_pth).map_err(|e| format!("复制 pth 失败: {e}"))?;
    let mut message = format!("已安装默认音源曼波（pth {:.1} MB）", fs::metadata(&target_pth).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0));
    let index = source_dir.join("manbo.index");
    if index.is_file() {
        let target_index = target_dir.join("曼波.index");
        fs::copy(&index, &target_index).map_err(|e| format!("复制 index 失败: {e}"))?;
        message.push_str(&format!("，index {:.1} MB", fs::metadata(&target_index).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0)));
    }
    Ok(message + "；可在 Gateway 启动后扫描")
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
    let source = env::var("AMS_DEFAULT_VOICE_DIR").unwrap_or_else(|_| "F:\\ai歌曲\\0 曼波".to_string());
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
    fs::create_dir_all(runtime_dir()).map_err(|e| format!("创建 runtime 失败: {e}"))?;
    let log = fs::File::create(kernel_log_path()).map_err(|e| format!("创建日志失败: {e}"))?;
    let log_err = log.try_clone().map_err(|e| format!("复制日志句柄失败: {e}"))?;

    if let Some(exe) = packaged_kernel_exe() {
        let electron_data = ams_home().join("kernel").join("electron-data");
        fs::create_dir_all(&electron_data).map_err(|e| format!("创建 electron userData 失败: {e}"))?;
        let mut command = Command::new(&exe);
        command
            .current_dir(ams_home().join("kernel"))
            .arg("--user-data-dir")
            .arg(&electron_data)
            .env("AMS_HOME", ams_home())
            .env("DSH_HOME", &dsh_home)
            .stdout(std::process::Stdio::from(log))
            .stderr(std::process::Stdio::from(log_err));
        for (key, _) in std::env::vars_os() {
            if let Some(k) = key.to_str() {
                if k.starts_with("DSH_") || k == "NODE_OPTIONS" || k == "NODE_PATH" || k == "ELECTRON_RUN_AS_NODE" {
                    command.env_remove(k);
                }
            }
        }
        command.env("DSH_HOME", &dsh_home).env("AMS_HOME", ams_home());
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
        Ok(out) => Ok(format!("已停止内核（pid={}）：{}", info.pid, String::from_utf8_lossy(&out.stdout).trim())),
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
        dev_root().join("ams-release").join("examples").join("keys").join("public.pem").display().to_string()
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
struct ComponentAsset {
    id: String,
    kind: String,
    version: String,
    url: String,
    size: u64,
    sha256: String,
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
                })
                .filter(|item| !item.id.is_empty() && !item.url.is_empty())
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
        copy(&mut response, &mut file).map_err(|e| format!("写入下载文件失败: {e}"))?;
    } else {
        let raw = asset.url.strip_prefix("file://").unwrap_or(&asset.url);
        let src = raw.trim_start_matches('/');
        fs::copy(src, dest).map_err(|e| format!("复制本地组件失败（{src}）: {e}"))?;
    }
    Ok(fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
}

fn install_component_blocking(asset: &ComponentAsset) -> ComponentResult {
    let result = (|| -> Result<(String, u64), String> {
        let downloads = ams_home().join("components").join("downloads");
        let target = if asset.kind == "kernel" {
            ams_home().join("kernel")
        } else {
            ams_home().join("components").join(&asset.id)
        };
        fs::create_dir_all(&downloads).map_err(|e| format!("创建下载目录失败: {e}"))?;
        let zip_path = downloads.join(format!("{}-{}.zip", asset.id, asset.version));
        let size = download_asset_to(asset, &zip_path)?;
        if asset.size > 0 && size != asset.size {
            return Err(format!("{} 大小不匹配：期望 {}，实际 {}", asset.id, asset.size, size));
        }
        let sha = sha256_of(&zip_path)?;
        if !sha.eq_ignore_ascii_case(&asset.sha256) {
            return Err(format!("{} sha256 不匹配：期望 {}，实际 {}", asset.id, asset.sha256, sha));
        }
        let extracted = extract_zip(&zip_path, &target)?;
        Ok((format!("已安装 {} {}（{} B 下载，{} B 解包）", asset.id, asset.version, size, extracted), size))
    })();
    match result {
        Ok((message, size)) => ComponentResult {
            id: asset.id.clone(),
            ok: true,
            message,
            path: ams_home().join("components").join(&asset.id).display().to_string(),
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
        let mut results = Vec::new();
        for asset in component_assets(&value) {
            if let Some(ref filter) = ids {
                if !filter.iter().any(|id| id == &asset.id) {
                    continue;
                }
            }
            results.push(install_component_blocking(&asset));
        }
        Ok(results)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--uninstall-cleanup") {
        let result = cleanup_ams_data();
        let line = match &result {
            Ok(message) => format!("cleanup ok: {message}"),
            Err(message) => format!("cleanup failed: {message}"),
        };
        let _ = fs::write(env::temp_dir().join("ams-uninstall-result.log"), line);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let _ = ensure_ams_home_marker();
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
            kernel_status,
            start_kernel,
            stop_kernel,
            submit_job,
            job_status,
            read_update_manifest,
            verify_update_manifest,
            fetch_components,
            install_components,
            app_version,
            open_uninstaller
        ])
        .run(tauri::generate_context!())
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