use serde::Serialize;
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
    let output = Command::new("powershell.exe")
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
    match Command::new(program).args(args).output() {
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
    let output = Command::new("taskkill.exe")
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

#[tauri::command]
fn install_default_voice() -> Result<String, String> {
    let source = env::var("AMS_DEFAULT_VOICE_DIR").unwrap_or_else(|_| "F:\\ai歌曲\\0 曼波".to_string());
    let source_dir = PathBuf::from(&source);
    let pth = source_dir.join("manbo.pth");
    let index = source_dir.join("manbo.index");
    if !pth.is_file() {
        return Err(format!("默认音源文件不存在：{}（可用 AMS_DEFAULT_VOICE_DIR 覆盖）", pth.display()));
    }
    let target_dir = ams_home().join("voices").join("曼波");
    let target_pth = target_dir.join("曼波.pth");
    if target_pth.is_file() {
        return Ok(format!("默认音源已存在：{}", target_pth.display()));
    }
    fs::create_dir_all(&target_dir).map_err(|e| format!("创建音源目录失败: {e}"))?;
    fs::copy(&pth, &target_pth).map_err(|e| format!("复制 pth 失败: {e}"))?;
    let mut message = format!("已安装默认音源曼波（pth {:.1} MB）", fs::metadata(&target_pth).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0));
    if index.is_file() {
        let target_index = target_dir.join("曼波.index");
        fs::copy(&index, &target_index).map_err(|e| format!("复制 index 失败: {e}"))?;
        message.push_str(&format!("，index {:.1} MB", fs::metadata(&target_index).map(|m| m.len()).unwrap_or(0) as f64 / (1024.0 * 1024.0)));
    }
    Ok(message + "；可在 Gateway 启动后扫描")
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
    let output = Command::new("tasklist.exe")
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

#[tauri::command]
fn kernel_status() -> KernelInfo {
    let dsh_home = ams_home().join("kernel").join("dsh").display().to_string();
    let mut info = KernelInfo {
        installed: dev_root().join("ams-harness").join("package.json").is_file(),
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
        info.running = !info.url.is_empty();
        info.starting = info.url.is_empty();
        if info.starting {
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
    let harness = dev_root().join("ams-harness");
    if !harness.join("package.json").is_file() {
        return Err(format!("找不到内核源码：{}", harness.display()));
    }
    let current = kernel_status();
    if current.pid > 0 && process_alive(current.pid) {
        return Ok(format!("内核已在运行（pid={}）", current.pid));
    }
    let dsh_home = ams_home().join("kernel").join("dsh");
    fs::create_dir_all(&dsh_home).map_err(|e| format!("创建 DSH_HOME 失败: {e}"))?;
    fs::create_dir_all(runtime_dir()).map_err(|e| format!("创建 runtime 失败: {e}"))?;
    let log = fs::File::create(kernel_log_path()).map_err(|e| format!("创建日志失败: {e}"))?;
    let log_err = log.try_clone().map_err(|e| format!("复制日志句柄失败: {e}"))?;
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
    let output = Command::new("taskkill.exe")
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

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}