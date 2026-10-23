# AMS Studio Launcher

> **AMS Studio 子项目 · 桌面启动器**  
> 负责把 AMS Studio 的启动、组件安装/更新、内核与本地 Gateway 生命周期、音源库和连接设置收在一个桌面入口里。

AMS Studio 是一个本地优先的 AI 音乐工作站：启动器在这套系统里是“总开关”和“安装器”，不是编辑器本身。它负责：

- 安装/更新 AMS 组件（kernel、plugins、gateway、catalog、voicepack）
- 校验签名/sequence 的更新 feed
- 启动/停止 AMS Kernel 与本地 Gateway
- 管理 AMS 数据目录、音源库、连接设置（网址 + Key）
- 安全卸载：只处理 AMS 自己的安装目录，不触碰现有 DSH 的 `.dsh`

## 技术栈

- Tauri 2 + Vue 3 + TypeScript
- Rust 后端负责路径、安装/更新、进程生命周期
- AMS 数据默认位于安装目录下的 `data`

## 本地开发

```powershell
pnpm install
pnpm tauri dev
pnpm tauri build
```

## 目录说明

| 路径 | 作用 |
|---|---|
| `src/` | Vue 启动器界面（组件、内核、Gateway、连接设置） |
| `src-tauri/src/lib.rs` | Rust 命令：环境、组件安装、内核/Gateway、更新、卸载 |
| `src-tauri/installer-hooks.nsh` | NSIS 卸载钩子（只清理 AMS 自己的数据） |

## 相关仓库

- 总仓库：https://github.com/yan-mc/AMS-Studio
- 内核：https://github.com/yan-mc/ams-harness
- 插件：https://github.com/yan-mc/ams-plugins
- 服务：https://github.com/yan-mc/ams-services
- 工具：https://github.com/yan-mc/ams-tools
- 文档：https://github.com/yan-mc/ams-docs
- 发布：https://github.com/yan-mc/ams-release
