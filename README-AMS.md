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
