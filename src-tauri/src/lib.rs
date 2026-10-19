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
            app_version
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}