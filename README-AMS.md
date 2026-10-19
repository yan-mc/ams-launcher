# AMS Studio Launcher

Tauri 2 + Vue 3 启动器（M0 骨架）。

## 当前状态（M0.2）

- 已定制：productName `AMS Studio`、identifier `com.yanmc.ams`、窗口尺寸、深色占位首页。
- 待实现：环境检测（Rust command）、组件/音源库/更新中心、内核启动与健康检查。

## 开发

```powershell
corepack pnpm install
corepack pnpm tauri dev     # 需要 MSVC Build Tools + WebView2
corepack pnpm tauri build   # 生成 NSIS/MSI（未签名）
```

## 约束

- 启动器是**唯一更新入口**（launcher/kernel/plugins/services/tools 分模块更新）。
- 不内置音源；默认音源曼波由安装包/离线资产包安装。
- 不接触用户 DeepSeek Key。
## M1 新增（v0.1.2）

- 真实环境检测：Windows/CPU/内存/GPU/Python/Node/FFmpeg/Git/磁盘/AMS 数据目录。
- Gateway 控制：状态/健康检查、端口/版本/任务数、启动（独立进程）/停止、5s 自动刷新。
- 组件状态：内核、插件、Gateway、音源库（读取 AMS_HOME/voices）。
- 开发默认路径可用 `AMS_DEV_ROOT` 覆盖，数据目录可用 `AMS_HOME` 覆盖。
