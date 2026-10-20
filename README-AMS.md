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
- v0.1.3：音源库卡片（扫描/状态/大小/index），安装默认曼波（从 `AMS_DEFAULT_VOICE_DIR` 复制到 `AMS_HOME/voices/曼波/`）；启动 Gateway 时自动注入 `AMS_RVC_PYTHON`（可用环境变量覆盖）。
- v0.1.4：内核卡片（启动/停止/状态/token URL 打开）；`DSH_HOME` 隔离在 `AMS_HOME/kernel/dsh`，日志 `AMS_HOME/runtime/kernel.log`；手动实测 DSH web 9911 启动/URL 解析/进程树停止。
- v0.1.5+：更新中心支持 **Ed25519 签名验证**（`verify_update_manifest`，默认公钥 `ams-release/examples/keys/public.pem`），验证 Python 签名清单通过、篡改/sequence 修改被拒；Rust 单元测试 3 passed。
## M2 新增（v0.2.0）

- 组件安装：读取 `AMS_COMPONENTS_FEED`（本地 JSON 路径或 HTTPS URL），Ed25519 验签后下载/校验/解包到 `AMS_HOME/components/<id>`；
- 组件包：plugin bundle 0.3.0 / gateway 0.1.0 / catalog 2026.10.19；
- 本地示例：
  ```powershell
  $env:AMS_COMPONENTS_FEED = "F:/Deepseek_harness/AI音乐处理/09_发布/staging/updates/ams-components.json"
  $env:AMS_HOME = "F:/Deepseek_harness/AI音乐处理/07_执行计划/_m2_home"
  ams-launcher.exe
  ```
- 注意：GitHub 仓库已私有，默认 GitHub feed 不能匿名下载；正式环境需更新服务器/鉴权，M2 用本地 feed 演示。
