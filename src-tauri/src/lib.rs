use serde::Serialize;
use std::process::Command;

#[derive(Serialize, Clone)]
struct EnvItem {
    label: String,
    value: String,
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
            if text.is_empty() && !err.is_empty() {
                err
            } else {
                text
            }
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

#[tauri::command]
fn get_environment() -> Vec<EnvItem> {
    let mut items: Vec<EnvItem> = Vec::new();

    let os = run_ps(
        "(Get-CimInstance Win32_OperatingSystem).Caption + ' build ' + (Get-CimInstance Win32_OperatingSystem).BuildNumber",
    );
    items.push(item("Windows", os, true));

    let cpu = run_ps("(Get-CimInstance Win32_Processor | Select-Object -First 1).Name");
    items.push(item("CPU", cpu, true));

    let ram = run_ps("[math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB,1)");
    let ram_text = if ram.is_empty() || ram.starts_with("ERR") {
        ram
    } else {
        format!("{ram} GB")
    };
    items.push(item("内存", ram_text, true));

    let gpu = run_cmd(
        "nvidia-smi.exe",
        &[
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader",
        ],
    );
    if gpu.is_empty() {
        let wmi = run_ps(
            "(Get-CimInstance Win32_VideoController | Select-Object -First 1).Name",
        );
        items.push(item("GPU", wmi, false));
    } else {
        items.push(item("GPU", gpu.lines().next().unwrap_or("").trim().to_string(), true));
    }

    let python = {
        let v = run_cmd("py.exe", &["-3", "--version"]);
        if v.is_empty() { run_cmd("python.exe", &["--version"]) } else { v }
    };
    items.push(item("Python", python, true));

    let node = run_cmd("node.exe", &["-v"]);
    let node_ok = !node.is_empty();
    items.push(item(
        "Node.js",
        if node_ok { node } else { "未安装".to_string() },
        node_ok,
    ));

    let ffmpeg = first_line(&run_cmd("ffmpeg.exe", &["-version"]));
    let ffmpeg_ok = !ffmpeg.is_empty();
    items.push(item(
        "FFmpeg",
        if ffmpeg_ok { ffmpeg } else { "未安装".to_string() },
        ffmpeg_ok,
    ));

    let git = run_cmd("git.exe", &["--version"]);
    let git_ok = !git.is_empty();
    items.push(item(
        "Git",
        if git_ok { git } else { "未安装".to_string() },
        git_ok,
    ));

    let disk = run_ps(
        "(Get-PSDrive -PSProvider FileSystem | Where-Object { $_.Free -ne $null } | ForEach-Object { $_.Name + ' ' + [math]::Round($_.Free/1GB) + 'GB' }) -join ' / '",
    );
    items.push(item("磁盘可用", disk, true));

    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    items.push(item(
        "AMS 数据目录（默认）",
        format!("{local}\\AMS"),
        true,
    ));
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|x| x.to_path_buf()))
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    items.push(item("启动器安装目录", exe, true));

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
        .invoke_handler(tauri::generate_handler![get_environment, app_version])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}