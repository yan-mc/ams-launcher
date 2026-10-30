<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";

type EnvItem = { label: string; value: string; ok: boolean };
type GatewayInfo = {
  installed: boolean;
  running: boolean;
  healthy: boolean;
  port: number;
  version: string;
  jobs: number;
  home: string;
  gateway_dir: string;
  message: string;
};
type ComponentItem = { name: string; state: string; detail: string; ok: boolean };
type VoiceInfo = { name: string; status: string; info: string; sr: string; size_mb: number; has_index: boolean; message: string };
type KernelInfo = { installed: boolean; running: boolean; starting: boolean; url: string; port: number; pid: number; dsh_home: string; log: string; message: string };
type ConnectionInfo = { base_url: string; has_api_key: boolean; configured: boolean; message: string };
type JobInfo = { id: string; job_type: string; status: string; progress: number; stage: string; error: string };
type UpdateComponent = { id: string; kind: string; version: string; size: number };
type UpdateManifest = { schema: string; channel: string; sequence: number; signature_present: boolean; components: UpdateComponent[]; path: string };
type UpdateVerify = { valid: boolean; message: string; sequence: number; components: number };
type ComponentAsset = { id: string; kind: string; version: string; url: string; size: number; sha256: string };
type ComponentResult = { id: string; ok: boolean; message: string; path: string; size: number };
type InstallProgress = { active: boolean; downloaded: number; total: number; percent: number; current: string };

const version = ref("0.1.2");
const showGuide = ref(false);
const showAgentGuide = ref(false);
const showFeatureGuide = ref(false);
const loading = ref(false);
const busy = ref("");
const error = ref("");
const checks = ref<EnvItem[]>([{ label: "环境检测", value: "正在读取…", ok: false }]);
const gateway = ref<GatewayInfo | null>(null);
const components = ref<ComponentItem[]>([]);
const voices = ref<VoiceInfo[]>([]);
const voiceMsg = ref("");
const voiceBusy = ref(false);
const voiceSource = ref("");
const kernel = ref<KernelInfo | null>(null);
const kernelMsg = ref("");
const msvc = ref({ input: "", reference: "", out: "", name: "" });
const msvcJob = ref<JobInfo | null>(null);
const msvcMsg = ref("");
const updatePath = ref("");
const updateInfo = ref<UpdateManifest | null>(null);
const updateMsg = ref("");
const updateVerify = ref<UpdateVerify | null>(null);
const compAssets = ref<ComponentAsset[]>([]);
const compResults = ref<ComponentResult[]>([]);
const compMsg = ref("");
const compBusy = ref(false);
const compProgress = ref<InstallProgress | null>(null);
let progressTimer: number | undefined;
const pluginBusy = ref(false);
const pluginMsg = ref("");
const uninstallMsg = ref("");
const connection = ref<ConnectionInfo | null>(null);
const runtime = ref<{ rvc_python: string; rvc_root: string; pymss_python: string; pymss_root: string; msvc_tool: string } | null>(null);
const rvcPython = ref("");
const rvcRoot = ref("");
const pymssPython = ref("");
const pymssRoot = ref("");
const msvcTool = ref("");
const runtimeMsg = ref("");
const runtimeBusy = ref(false);
const connBase = ref("");
const connKey = ref("");
const connMsg = ref("");
const connBusy = ref(false);
let timer: number | undefined;

async function refreshEnv() {
  loading.value = true;
  error.value = "";
  try {
    checks.value = await invoke<EnvItem[]>("get_environment");
    version.value = await invoke<string>("app_version");
  } catch (e) {
    error.value = String(e);
    checks.value = [{ label: "运行环境", value: "浏览器预览模式（无法读取本机信息）", ok: false }];
  } finally {
    loading.value = false;
  }
}

async function refreshKernel() {
  try {
    kernel.value = await invoke<KernelInfo>("kernel_status");
  } catch (e) {
    kernelMsg.value = String(e);
  }
}

async function startKernel() {
  kernelMsg.value = "正在启动内核…";
  try {
    kernelMsg.value = await invoke<string>("start_kernel");
    setTimeout(refreshKernel, 3000);
  } catch (e) {
    kernelMsg.value = "启动失败：" + String(e);
  }
}

async function stopKernel() {
  try {
    kernelMsg.value = await invoke<string>("stop_kernel");
    await refreshKernel();
  } catch (e) {
    kernelMsg.value = "停止失败：" + String(e);
  }
}

async function openKernel() {
  if (kernel.value?.url) await openUrl(kernel.value.url);
}

async function refreshGateway() {
  try {
    gateway.value = await invoke<GatewayInfo>("gateway_status");
  } catch (e) {
    error.value = String(e);
  }
}

async function refreshComponents() {
  try {
    components.value = await invoke<ComponentItem[]>("component_status");
  } catch (e) {
    error.value = String(e);
  }
}

async function scanVoices() {
  try {
    voices.value = await invoke<VoiceInfo[]>("scan_voices");
    voiceMsg.value = voices.value.length ? "" : "音源库为空";
  } catch (e) {
    voiceMsg.value = "扫描失败：" + String(e);
  }
}

async function installVoiceFromSource() {
  voiceBusy.value = true;
  voiceMsg.value = "正在从本地路径安装曼波音源…";
  try {
    voiceMsg.value = await invoke<string>("install_default_voice_from", { source: voiceSource.value });
    await scanVoices();
  } catch (e) {
    voiceMsg.value = "安装失败：" + String(e);
  } finally {
    voiceBusy.value = false;
  }
}

async function installDefaultVoice() {
  voiceBusy.value = true;
  voiceMsg.value = "正在安装默认音源曼波（约 197MB）…";
  try {
    voiceMsg.value = await invoke<string>("install_default_voice");
    await scanVoices();
  } catch (e) {
    voiceMsg.value = "安装失败：" + String(e);
  } finally {
    voiceBusy.value = false;
  }
}

async function openDataFolder(kind: string) {
  try {
    await invoke<string>("open_data_folder", { kind });
  } catch (e) {
    voiceMsg.value = "打开目录失败：" + String(e);
  }
}

async function startGateway() {
  busy.value = "正在启动 Gateway…";
  error.value = "";
  try {
    const message = await invoke<string>("start_gateway");
    busy.value = message;
    setTimeout(() => { refreshGateway(); busy.value = ""; }, 2500);
  } catch (e) {
    error.value = String(e);
    busy.value = "";
  }
}

async function stopGateway() {
  busy.value = "正在停止 Gateway…";
  try {
    busy.value = await invoke<string>("stop_gateway");
    setTimeout(() => { refreshGateway(); busy.value = ""; }, 1200);
  } catch (e) {
    error.value = String(e);
    busy.value = "";
  }
}

async function submitMsvc() {
  msvcMsg.value = "";
  try {
    const outDir = msvc.value.out || (gateway.value ? gateway.value.home + "\\artifacts" : "");
    const id = await invoke<string>("submit_job", {
      jobType: "convert.msvc",
      params: { input_path: msvc.value.input, reference_pth: msvc.value.reference, out_dir: outDir, name: msvc.value.name },
    });
    msvcJob.value = { id, job_type: "convert.msvc", status: "queued", progress: 0, stage: "", error: "" };
    msvcMsg.value = "已提交任务 " + id;
    setTimeout(refreshMsvcJob, 1500);
  } catch (e) {
    msvcMsg.value = "提交失败：" + String(e);
  }
}

async function refreshMsvcJob() {
  if (!msvcJob.value?.id) return;
  try {
    msvcJob.value = await invoke<JobInfo>("job_status", { jobId: msvcJob.value.id });
  } catch (e) {
    msvcMsg.value = "查询失败：" + String(e);
  }
}

async function verifyUpdate() {
  updateMsg.value = "";
  try {
    updateVerify.value = await invoke<UpdateVerify>("verify_update_manifest", { path: updatePath.value });
    updateMsg.value = updateVerify.value.message;
  } catch (e) {
    updateVerify.value = null;
    updateMsg.value = "验签失败：" + String(e);
  }
}

async function pickMsvcInput() {
  const p = await open({ multiple: false, filters: [{ name: "MSVC", extensions: ["msvc"] }] });
  if (typeof p === "string") msvc.value.input = p;
}

async function pickReference() {
  const p = await open({ multiple: false, filters: [{ name: "RVC pth", extensions: ["pth"] }] });
  if (typeof p === "string") msvc.value.reference = p;
}

async function pickOutDir() {
  const p = await open({ directory: true, multiple: false });
  if (typeof p === "string") msvc.value.out = p;
}

async function pickUpdateFile() {
  const p = await open({ multiple: false, filters: [{ name: "Update manifest", extensions: ["json"] }] });
  if (typeof p === "string") updatePath.value = p;
}
async function loadUpdate() {
  updateMsg.value = "";
  updateVerify.value = null;
  try {
    updateInfo.value = await invoke<UpdateManifest>("read_update_manifest", { path: updatePath.value });
    updateMsg.value = "已读取 " + updateInfo.value.components.length + " 个组件";
  } catch (e) {
    updateInfo.value = null;
    updateMsg.value = "读取失败：" + String(e);
  }
}
async function loadComponents() {
  compMsg.value = "";
  try {
    compAssets.value = await invoke<ComponentAsset[]>("fetch_components");
    compMsg.value = "组件清单：" + compAssets.value.length + " 个组件";
  } catch (e) {
    compAssets.value = [];
    compMsg.value = "读取组件清单失败：" + String(e);
  }
}

async function installAllComponents() {
  compBusy.value = true;
  compResults.value = [];
  compProgress.value = { active: true, downloaded: 0, total: 0, percent: 0, current: "准备下载组件" };
  compMsg.value = "正在下载并安装组件（大文件可能较久）…";
  if (progressTimer !== undefined) window.clearInterval(progressTimer);
  progressTimer = window.setInterval(async () => {
    try { compProgress.value = await invoke<InstallProgress>("install_progress"); } catch { /* best-effort */ }
  }, 400);
  try {
    compResults.value = await invoke<ComponentResult[]>("install_components", { ids: null });
    const ok = compResults.value.filter((r) => r.ok).length;
    compMsg.value = "安装完成：" + ok + "/" + compResults.value.length + " 成功";
    if (progressTimer !== undefined) { window.clearInterval(progressTimer); progressTimer = undefined; }
    compProgress.value = { active: false, downloaded: 0, total: 0, percent: 100, current: "安装完成" };
    await refreshComponents();
    if (gateway.value?.healthy) await scanVoices();
  } catch (e) {
    compMsg.value = "安装失败：" + String(e);
  } finally {
    if (progressTimer !== undefined) { window.clearInterval(progressTimer); progressTimer = undefined; }
    compBusy.value = false;
  }
}

function formatBytes(value: number): string {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit += 1; }
  return size.toFixed(unit === 0 ? 0 : 1) + " " + units[unit];
}
async function requestUninstall() {
  const ok = window.confirm("确定卸载 嘉音Studio？\n\n只删除 AMS 的安装目录和 AMS 数据目录中的 kernel/components/runtime/logs；voices、artifacts 和现有 DSH 的 .dsh 不会被删除。");
  if (!ok) return;
  uninstallMsg.value = "正在打开卸载程序…";
  try { uninstallMsg.value = await invoke<string>("open_uninstaller"); }
  catch (e) { uninstallMsg.value = "卸载失败：" + String(e); }
}

async function refreshConnection() {
  try {
    connection.value = await invoke<ConnectionInfo>("connection_status");
    if (!connBase.value) connBase.value = connection.value.base_url;
  } catch (e) { connMsg.value = String(e); }
}

async function refreshRuntime() {
  try {
    runtime.value = await invoke<{ rvc_python: string; rvc_root: string; pymss_python: string; pymss_root: string; msvc_tool: string }>("runtime_status");
    if (!rvcPython.value) rvcPython.value = runtime.value.rvc_python;
    if (!rvcRoot.value) rvcRoot.value = runtime.value.rvc_root;
    if (!pymssPython.value) pymssPython.value = runtime.value.pymss_python;
    if (!pymssRoot.value) pymssRoot.value = runtime.value.pymss_root;
    if (!msvcTool.value) msvcTool.value = runtime.value.msvc_tool;
  } catch (e) { runtimeMsg.value = String(e); }
}

async function saveRuntime() {
  runtimeBusy.value = true;
  runtimeMsg.value = "";
  try {
    runtimeMsg.value = await invoke<string>("save_runtime", { rvcPython: rvcPython.value, rvcRoot: rvcRoot.value, pymssPython: pymssPython.value, pymssRoot: pymssRoot.value, msvcTool: msvcTool.value });
    await refreshRuntime();
  } catch (e) { runtimeMsg.value = "保存失败：" + String(e); }
  finally { runtimeBusy.value = false; }
}

async function saveConnection() {
  connBusy.value = true;
  connMsg.value = "";
  try {
    connMsg.value = await invoke<string>("save_connection", { baseUrl: connBase.value, apiKey: connKey.value });
    connKey.value = "";
    await refreshConnection();
  } catch (e) { connMsg.value = "保存失败：" + String(e); }
  finally { connBusy.value = false; }
}

async function installPlugins() {
  pluginBusy.value = true;
  pluginMsg.value = "";
  try {
    pluginMsg.value = await invoke<string>("install_music_plugins");
    await refreshKernel();
  } catch (e) {
    pluginMsg.value = "插件安装失败：" + String(e);
  } finally {
    pluginBusy.value = false;
  }
}

async function refreshAll() {
  await Promise.all([refreshEnv(), refreshGateway(), refreshComponents(), refreshKernel(), refreshConnection(), refreshRuntime()]);
}

onMounted(() => {
  document.title = "嘉音Studio";
  refreshAll();
  timer = window.setInterval(() => { refreshGateway(); refreshKernel(); }, 10000);
});
onUnmounted(() => {
  if (timer !== undefined) window.clearInterval(timer);
  if (progressTimer !== undefined) {
    window.clearInterval(progressTimer);
    progressTimer = undefined;
  }
});
</script>

<template>
  <main class="app">
    <header>
      <div class="brand-head">
        <img class="brand-logo" src="./assets/logo.png" alt="嘉音Studio" />
        <div>
          <h1>嘉音Studio</h1>
          <p class="sub">本地 AI 音乐工作站 · Launcher v{{ version }} <span class="tag">嘉音Studio</span></p>
        </div>
      </div>
      <div class="header-actions">
        <button class="guide-button" @click="showAgentGuide = true">Agent 插件功能</button>
        <button class="guide-button" @click="showFeatureGuide = true">软件功能总览</button>
        <button class="guide-button" @click="showGuide = true">使用说明 / 关于</button>
        <span class="ver">v{{ version }}</span>
      </div>
    </header>

    <section class="grid">
      <div class="card">
        <h2>连接设置（只保存在 AMS 安装目录）</h2>
        <ul>
          <li><span>网址</span><input v-model="connBase" placeholder="https://api.deepseek.com/anthropic" /></li>
          <li><span>Key</span><input v-model="connKey" type="password" placeholder="留空表示不修改已保存的 Key" /></li>
        </ul>
        <p class="note" v-if="connection">当前：{{ connection.configured ? "已配置" : "未配置" }} · {{ connection.message }}</p>
        <div class="row">
          <button :disabled="connBusy" @click="saveConnection">{{ connBusy ? "保存中…" : "保存连接" }}</button>
          <button @click="refreshConnection">刷新</button>
        </div>
        <p v-if="connMsg" class="note">{{ connMsg }}</p>
      </div>

      <div class="card">
        <h2>运行环境路径（只保存到 AMS 安装目录）</h2>
        <ul>
          <li><span>RVC Python</span><input v-model="rvcPython" placeholder="data\runtime\rvc\<ver>\runtime\python.exe" /></li>
          <li><span>RVC 根目录</span><input v-model="rvcRoot" placeholder="data\runtime\rvc\<ver>" /></li>
          <li><span>pymss Python</span><input v-model="pymssPython" placeholder="data\runtime\pymss\<ver>\.venv\Scripts\python.exe" /></li>
          <li><span>pymss 根目录</span><input v-model="pymssRoot" placeholder="data\runtime\pymss\<ver>" /></li>
          <li><span>MSVC 工具</span><input v-model="msvcTool" placeholder="data\tools\msvc2pth\<ver>\msvc2pth.py" /></li>
        </ul>
        <div class="row">
          <button :disabled="runtimeBusy" @click="saveRuntime">{{ runtimeBusy ? "保存中…" : "保存运行环境" }}</button>
          <button @click="refreshRuntime">刷新</button>
        </div>
        <p v-if="runtimeMsg" class="note">{{ runtimeMsg }}</p>
      </div>

      <div class="card">
        <h2>环境检测</h2>
        <p v-if="error" class="err">{{ error }}</p>
        <ul>
          <li v-for="c in checks" :key="c.label">
            <span>{{ c.label }}</span>
            <b :class="c.ok ? 'ok' : 'warn'">{{ c.value }}</b>
          </li>
        </ul>
        <button :disabled="loading" @click="refreshEnv">{{ loading ? "检测中…" : "重新检测" }}</button>
      </div>

      <div class="card">
        <h2>内核（DSH）</h2>
        <ul v-if="kernel">
          <li><span>状态</span><b :class="kernel.running ? 'ok' : kernel.starting ? 'warn' : 'bad'">{{ kernel.running ? "运行中" : kernel.starting ? "启动中" : "未运行" }}</b></li>
          <li><span>端口</span><b>{{ kernel.port || "—" }}</b></li>
          <li><span>PID</span><b>{{ kernel.pid || "—" }}</b></li>
          <li><span>DSH_HOME</span><b>{{ kernel.dsh_home }}</b></li>
          <li v-if="kernel.message"><span>说明</span><b class="warn">{{ kernel.message }}</b></li>
        </ul>
        <p v-if="kernelMsg" class="note">{{ kernelMsg }}</p>
        <div class="row">
          <button :disabled="!kernel?.installed || kernel?.running || kernel?.starting" @click="startKernel">启动内核</button>
          <button :disabled="!kernel?.running && !kernel?.starting" @click="stopKernel">停止</button>
          <button :disabled="!kernel?.url" @click="openKernel">打开内核</button>
          <button @click="refreshKernel">刷新</button>
        </div>
      </div>

      <div class="card">
        <h2>Gateway</h2>
        <ul v-if="gateway">
          <li><span>状态</span><b :class="gateway.healthy ? 'ok' : gateway.installed ? 'warn' : 'bad'">{{ gateway.healthy ? "运行中" : gateway.installed ? "已安装未运行" : "未安装" }}</b></li>
          <li><span>端口</span><b>{{ gateway.port || "—" }}</b></li>
          <li><span>版本 / 任务</span><b>{{ gateway.version || "—" }} / {{ gateway.jobs }}</b></li>
          <li><span>数据目录</span><b>{{ gateway.home }}</b></li>
          <li v-if="gateway.message"><span>说明</span><b class="warn">{{ gateway.message }}</b></li>
        </ul>
        <p v-if="busy" class="note">{{ busy }}</p>
        <div class="row">
          <button :disabled="!gateway?.installed || gateway?.healthy" @click="startGateway">启动</button>
          <button :disabled="!gateway?.running" @click="stopGateway">停止</button>
          <button @click="refreshGateway">刷新</button>
        </div>
      </div>

      <div class="card">
        <h2>维护</h2>
        <p class="note">卸载只清理 AMS 自己的安装目录和 AMS 数据目录（kernel/components/runtime/logs）；voices、artifacts 以及现有 DSH 的 .dsh 不会被删除。</p>
        <button @click="requestUninstall">卸载 嘉音Studio…</button>
        <p v-if="uninstallMsg" class="note">{{ uninstallMsg }}</p>
      </div>

      <div class="card">
        <h2>MSVC → pth 转换器</h2>
        <ul>
          <li><span>输入 .msvc</span><input v-model="msvc.input" placeholder="选择或输入 .msvc 文件路径" /><button class="mini" @click="pickMsvcInput">选择</button></li>
          <li><span>参考 pth</span><input v-model="msvc.reference" placeholder="可选：参考 .pth 路径" /><button class="mini" @click="pickReference">选择</button></li>
          <li><span>输出目录</span><input v-model="msvc.out" placeholder="留空=AMS_HOME/artifacts" /><button class="mini" @click="pickOutDir">选择</button></li>
          <li><span>音源名</span><input v-model="msvc.name" placeholder="可选" /></li>
        </ul>
        <div class="row">
          <button :disabled="!gateway?.healthy" @click="submitMsvc">提交转换</button>
          <button :disabled="!msvcJob" @click="refreshMsvcJob">刷新任务</button>
        </div>
        <p v-if="msvcMsg" class="note">{{ msvcMsg }}</p>
        <div v-if="msvcJob" class="job">
          <b>{{ msvcJob.status }}</b> · {{ msvcJob.progress.toFixed(0) }}% · {{ msvcJob.stage }}
          <span v-if="msvcJob.error" class="err">{{ msvcJob.error }}</span>
        </div>
      </div>

      <div class="card">
        <h2>更新中心</h2>
        <input v-model="updatePath" class="wide-input" />
        <button class="mini" @click="pickUpdateFile">选择清单</button>
        <div class="row">
          <button @click="loadUpdate">读取清单</button>
          <button @click="verifyUpdate">验证签名</button>
        </div>
        <p v-if="updateMsg" class="note">{{ updateMsg }}</p>
        <ul v-if="updateInfo">
          <li><span>schema / channel</span><b>{{ updateInfo.schema }} / {{ updateInfo.channel }}</b></li>
          <li><span>sequence</span><b>{{ updateInfo.sequence }}</b></li>
          <li><span>signature</span><b :class="updateInfo.signature_present ? 'ok' : 'bad'">{{ updateInfo.signature_present ? "存在" : "缺失" }}</b></li>
          <li v-if="updateVerify"><span>验签</span><b :class="updateVerify.valid ? 'ok' : 'bad'">{{ updateVerify.message }}</b></li>
          <li v-for="c in updateInfo.components" :key="c.id"><span>{{ c.kind }}</span><b>{{ c.id }} · {{ c.version }} · {{ c.size }} B</b></li>
        </ul>
      </div>
      <div class="card wide">
        <h2>组件安装（本地 feed / GitHub Releases）</h2>
        <p v-if="compMsg" class="note">{{ compMsg }}</p>
        <div v-if="compProgress && (compProgress.active || compBusy)" class="progress">
          <div class="progress-head">
            <span>{{ compProgress.current || "正在安装…" }}</span>
            <b>{{ compProgress.percent.toFixed(1) }}%</b>
          </div>
          <div class="progress-track"><div class="progress-fill" :style="{ width: compProgress.percent + '%' }"></div></div>
          <small>{{ formatBytes(compProgress.downloaded) }} / {{ formatBytes(compProgress.total) }}</small>
        </div>
        <ul v-if="compAssets.length">
          <li v-for="c in compAssets" :key="c.id">
            <span>{{ c.kind }}</span>
            <b>{{ c.id }} · {{ c.version }} · {{ (c.size / 1048576).toFixed(1) }} MB</b>
            <small>{{ c.url }}</small>
          </li>
        </ul>
        <ul v-if="compResults.length">
          <li v-for="r in compResults" :key="r.id"><span>{{ r.id }}</span><b :class="r.ok ? 'ok' : 'bad'">{{ r.ok ? "成功" : "失败" }} · {{ r.message }}</b></li>
        </ul>
        <div class="row">
          <button :disabled="compBusy" @click="loadComponents">检查组件清单</button>
          <button :disabled="compBusy || !compAssets.length" @click="installAllComponents">下载并安装全部</button>
          <button :disabled="pluginBusy" @click="installPlugins">安装插件到内核（音乐 + Agent）</button>
        </div>
        <p v-if="pluginMsg" class="note">{{ pluginMsg }}</p>
        <p class="note">插件安装需要内核 profile 已初始化：先启动一次 AMS Kernel，停止后再点“安装插件到内核”。会同时安装音乐插件（14 页签）和 Agent 插件（20 个 ams_* 工具 + 内核换标）。</p>
      </div>

      <div class="card wide">
        <h2>音源库</h2>
        <p v-if="voiceMsg" class="note">{{ voiceMsg }}</p>
        <ul v-if="voices.length">
          <li v-for="v in voices" :key="v.name">
            <span>{{ v.name }}</span>
            <b :class="v.status === 'ok' ? 'ok' : 'warn'">{{ v.status }} · {{ v.info }} · {{ v.sr }}</b>
            <small>{{ v.size_mb }} MB · index={{ v.has_index ? "有" : "无" }} {{ v.message }}</small>
          </li>
        </ul>
        <div class="row">
          <button :disabled="voiceBusy" @click="scanVoices">扫描音源</button>
          <button :disabled="voiceBusy" @click="installDefaultVoice">安装默认曼波</button>
          <button @click="openDataFolder('rvc_voices')">打开变声音源目录</button>
          <button @click="openDataFolder('tts_voices')">打开 TTS 音源目录</button>
        </div>
        <div class="row">
          <input v-model="voiceSource" class="wide-input" placeholder="本地曼波目录或 voicepack.zip 路径" />
          <button :disabled="voiceBusy || !voiceSource" @click="installVoiceFromSource">从本地路径安装</button>
        </div>
        <p class="note">默认音源从本地目录/voicepack 复制到 AMS_HOME/voices/曼波/，不进 Git/安装包；不再依赖环境变量。</p>
      </div>

      <div class="card wide">
        <h2>组件状态</h2>
        <ul>
          <li v-for="c in components" :key="c.name">
            <span>{{ c.name }}</span>
            <b :class="c.ok ? 'ok' : 'warn'">{{ c.state }}</b>
            <small>{{ c.detail }}</small>
          </li>
        </ul>
        <p class="note">M0 已完成：DSH fork 构建 / 启动器打包 / Gateway 23 测试 / 默认曼波 / MSVC 3/3 / 插件闭环 12/12 / RVC+pymss 冒烟 / CI / 备份。</p>
      </div>
    </section>
    <div v-if="showGuide" class="guide-overlay" @click.self="showGuide = false">
      <section class="guide-panel" role="dialog" aria-modal="true" aria-label="AMS 使用说明">
        <div class="guide-head">
          <div>
            <h1>嘉音Studio 使用说明</h1>
            <p class="sub">Launcher v{{ version }} · 本地 AI 音乐工作站</p>
          </div>
          <button class="guide-close" @click="showGuide = false">关闭</button>
        </div>
        <div class="guide-body">
          <h2>整体结构</h2>
          <p>启动器负责安装、组件、内核/Gateway 和音源；内核是本地工作区；音乐插件提供任务页面；Gateway 和 RVC/pymss/TTS 运行时执行任务。</p>
          <h2>首次使用顺序</h2>
          <ol>
            <li>检查并安装全部组件。</li>
            <li>启动内核一次，停止后安装音乐插件，再启动内核。</li>
            <li>启动 Gateway。</li>
            <li>打开内核，在左侧进入“AMS 音乐”。</li>
          </ol>
          <h2>当前可用功能</h2>
          <ul>
            <li><b>TTS 语音合成</b>：Piper，默认中文音色；用户音源放在 data/tts_voices。</li>
            <li><b>人声分离</b>：pymss，输出 vocals/drums/bass/other stems。</li>
            <li><b>歌曲换源</b>：严格输入已分离纯人声，输出目标音色纯人声，不混伴奏。</li>
            <li><b>MSVC 工具</b>：只把 .msvc 转成 .pth；index 可选，缺失时 index_rate=0。</li>
            <li><b>任务系统</b>：进度、SSE、取消、产物目录。</li>
          </ul>
          <h2>数据在哪里</h2>
          <ul>
            <li>变声音源：data/voices/&lt;名称&gt;/&lt;名称&gt;.pth（+ 可选 index）</li>
            <li>TTS 音源：data/tts_voices/&lt;模型&gt;/*.onnx + .onnx.json</li>
            <li>任务产物：data/artifacts/&lt;job_id&gt;/</li>
            <li>外部素材导入：data/projects/imports/</li>
          </ul>
          <h2>文件选择与目录</h2>
          <p>音乐插件里的文件字段可直接点“选择文件”，选中后文件复制进 data 并自动回填路径。音源目录和产物目录按钮打开资源管理器，按钮旁也显示路径。</p>
          <h2>功能总览 / Agent 插件</h2>
          <p>全部 14 个音乐页签（音源炼制、实时变声、数据集工作室、混音母带、评测器、项目管理等）均已实现；点击右上角“软件功能总览”和“Agent 插件功能”查看完整说明。</p>
          <h2>上游项目</h2>
          <p>DSH / Cordis / RVC-Project / pymss-project / Piper。具体版本和许可证以正式发布说明为准。</p>
          <h2>安全</h2>
          <p>所有 AMS 数据在安装根 data 下；不修改系统环境变量；Gateway 只监听 127.0.0.1；卸载默认保留用户数据，彻底清理需手动删除安装根。</p>
        </div>
      </section>
    </div>
    <div v-if="showAgentGuide" class="guide-overlay" @click.self="showAgentGuide = false">
      <section class="guide-panel" role="dialog" aria-modal="true" aria-label="Agent 插件功能">
        <div class="guide-head">
          <div>
            <h1>Agent 插件功能（20 个工具）</h1>
            <p class="sub">让 AI 在对话里直接操作整套嘉音Studio · 默认只读，写操作需确认</p>
          </div>
          <button class="guide-close" @click="showAgentGuide = false">关闭</button>
        </div>
        <div class="guide-body">
          <p>安装方式：启动器 → “安装插件到内核（音乐 + Agent）”。装好后在 AMS Kernel 的对话框里直接说需求，Agent 会调用 <b>ams_*</b> 工具完成；也可以让它先列清单/出报告，再确认执行。</p>
          <h2>一、查看与巡检（只读）</h2>
          <ul>
            <li><b>ams_mode_enter</b>：进入音乐工作模式（read / execute / publish），写操作前先过这一关。</li>
            <li><b>ams_status</b>：查看 Gateway 健康、版本、任务数、数据根目录。</li>
            <li><b>ams_inventory</b>：列出音源（RVC）、TTS 音色、数据集、工程。</li>
            <li><b>ams_dataset_inspect</b>：检查数据集结构、片段数、时长、采样率、缺失文件。</li>
            <li><b>ams_dataset_audit</b>：数据集质量审计——过短/过长、静音、削波、音量过低、重复片段、缺文本、语气词。</li>
            <li><b>ams_docs_lookup</b>：按需查询内置参考文档（架构/工具/安全/数据集/发布/经验）。</li>
            <li><b>ams_release_check</b>：发布前自检 feed/manifest sequence、setup 哈希、SHA256SUMS、必需资产。</li>
          </ul>
          <h2>二、整理与准备（写操作，需确认）</h2>
          <ul>
            <li><b>ams_dataset_clean</b>：安全清理数据集——先给报告和逐文件清单，确认后只移动到 data/trash，不删原件。</li>
            <li><b>ams_tts_text_clean</b>：TTS 文本清洗（去括号/标签/控制字符、标点规范化），输出 metadata.cleaned.csv，原文件不动。</li>
            <li><b>ams_dataset_prepare_rvc</b>：把 wav/目录准备成 RVC 训练数据集（统一采样率、切 3–10 秒片段）。</li>
            <li><b>ams_dataset_prepare_tts</b>：准备 TTS 训练数据集。</li>
            <li><b>ams_voice_delete_to_trash</b>：把音色/模型目录移到回收站（可恢复）。</li>
          </ul>
          <h2>三、跑任务（Gateway 串行 GPU 队列）</h2>
          <ul>
            <li><b>ams_job_submit</b>：提交 RVC 训练/换源、pymss 分离、TTS 合成/训练、数据集准备、评测、混音、MSVC 转换。</li>
            <li><b>ams_job_status / ams_job_list / ams_job_wait / ams_job_cancel</b>：查询、列表、等待、取消任务。</li>
            <li><b>ams_tts_synthesize</b>：直接合成 TTS（需确认）。</li>
            <li><b>ams_transcribe</b>：本地 Vosk 语音转写（音频不上传；依赖/模型自动探测）。</li>
            <li><b>ams_evaluate_audio</b>：客观评测（时长/采样率/峰值/RMS/削波/静音），可选 ASR 转写并算字召回。</li>
          </ul>
          <h2>四、Skill 与安全</h2>
          <ul>
            <li><b>Skill：ams-music-processing</b>：让 Agent 知道什么时候用哪个工具、写操作边界、数据集规范、发布检查清单。</li>
            <li>只读写 &lt;安装根&gt;/data，越界直接 PATH_DENIED；没有 shell；删除只进回收站；音频/转写默认不外传。</li>
            <li>审计日志：data/logs/agent-music-load.log（加载、20 tools 注册、自检）。</li>
          </ul>
          <h2>五、对话示例</h2>
          <ul>
            <li>“看看我有哪些音源和数据集，哪个数据集质量有问题？”</li>
            <li>“把 datasets/xianyun640 里太短的片段清掉，先给报告，确认后再移动。”</li>
            <li>“用曼波音色把 projects/demo/vocals.wav 换源，调 +2 半音。”</li>
            <li>“把这首歌做人声分离，再用分离出的人声换源。”</li>
            <li>“合成一段中文 TTS：‘欢迎使用嘉音Studio’，用 huayan 音色。”</li>
            <li>“评测 artifacts/job_xxx/out.wav，转写看看和文本对不对。”</li>
          </ul>
        </div>
      </section>
    </div>
    <div v-if="showFeatureGuide" class="guide-overlay" @click.self="showFeatureGuide = false">
      <section class="guide-panel" role="dialog" aria-modal="true" aria-label="软件功能总览">
        <div class="guide-head">
          <div>
            <h1>嘉音Studio 软件功能总览</h1>
            <p class="sub">14 个音乐页签 + 本地运行环境 + 任务系统 · 全部本地、GPU 加速、数据不出机</p>
          </div>
          <button class="guide-close" @click="showFeatureGuide = false">关闭</button>
        </div>
        <div class="guide-body">
          <h2>0. 它是什么</h2>
          <p>本地优先的 AI 音乐工作站：Tauri 启动器负责安装/更新/启动；DSH 内核是工作区与 AI 对话入口；Cordis 插件提供 14 个音乐页签；FastAPI Gateway 调度 RVC / pymss / TTS / TTS 训练 / MSVC / 混音 / 评测等任务；所有音频、模型、工程都在安装目录 data 下。</p>
          <h2>1. 项目 / 工程管理</h2>
          <ul>
            <li>页签“项目”：新建/切换工程，查看导入素材、历史任务与产物。</li>
            <li>外部音频先复制进 data/projects/imports 再引用，避免路径丢失。</li>
            <li>产物统一在 data/artifacts/&lt;job_id&gt;/，可一键打开目录。</li>
          </ul>
          <h2>2. 音源炼制（RVC 训练）</h2>
          <ul>
            <li>输入：已准备好的数据集（data/datasets/&lt;名称&gt;/processed，3–10 秒片段、统一采样率）。</li>
            <li>参数：模型版本 v2/v1、采样率 48k/40k/32k、F0 引导+方法（rmvpe/harvest/pm）、epochs、batch、save_every、GPU。</li>
            <li>输出：data/voices/&lt;模型名&gt;/&lt;模型名&gt;.pth（+ .index），训练完自动注册为可用音色。</li>
            <li>建议：显存不足把 batch 降到 1–2；数据集质量决定音色好坏（先用“数据集工作室”审计/清理）。</li>
          </ul>
          <h2>3. 歌曲换源（RVC 推理）</h2>
          <ul>
            <li>严格输入“已分离的纯人声”（先用“人声分离”处理整曲）；输出目标音色纯人声，不混伴奏。</li>
            <li>可调：目标音色、变调（半音）、F0 方法、index 混合率、filter_radius；支持批量队列、A/B 试听、导出纯人声。</li>
            <li>产物仍可回到“混音母带”与伴奏合成。</li>
          </ul>
          <h2>4. 实时变声（VC）</h2>
          <ul>
            <li>页签“实时变声”：选择输入/输出设备与目标音色，实时把麦克风/系统声音变成目标音色。</li>
            <li>依赖 service-vc（组件安装时部署到 data/components/vc-service），Kernel 首次启动后自动接管。</li>
            <li>适合直播/配音试音；长稳建议先做 30 分钟压测。</li>
          </ul>
          <h2>5. 音色库</h2>
          <ul>
            <li>管理 data/voices 下的 RVC 音色：扫描、校验（pth/index、采样率）、打开目录、从本地路径导入。</li>
            <li>支持把模型移到回收站（可恢复），不会直接删除原件。</li>
          </ul>
          <h2>6. 人声分离（pymss）</h2>
          <ul>
            <li>输入整曲音频，输出 vocals / drums / bass / other stems。</li>
            <li>可选模型 scnet_checkpoint_musdb18 / bs_roformer_voc_hyperacev2；可调 device、chunk、overlap、stem_batch_size、归一化/标准化。</li>
            <li>产物一键“用于换源”，直接进入歌曲换源流程。</li>
          </ul>
          <h2>7. 混音母带</h2>
          <ul>
            <li>多轨输入（分号/换行分隔），逐轨增益、声像、三段 EQ（100Hz/1kHz/8kHz）。</li>
            <li>输出响度归一 + 真峰值限幅（目标 LUFS 默认 -14，dBTP 默认 -1）。</li>
            <li>轻量 DSP，定位是“快速可用母带”，不是完整 DAW。</li>
          </ul>
          <h2>8. 数据集工作室</h2>
          <ul>
            <li>输入 wav 或目录，统一目标采样率（48k/40k/32k）并切 3–10 秒片段 → data/datasets/&lt;名称&gt;/processed。</li>
            <li>同时生成 metadata；配合“数据集审计”检查静音/削波/重复/缺文本。</li>
          </ul>
          <h2>9. 评测器</h2>
          <ul>
            <li>客观指标：时长、采样率、声道、削波、静音比例、峰值、RMS；可给参考音频算 SNR。</li>
            <li>可接 ASR 转写并与预期文本比对（字召回），用于快速验收。</li>
          </ul>
          <h2>10. TTS 语音合成（Piper）</h2>
          <ul>
            <li>页签“TTS 语音合成”：选择 TTS 音色（data/tts_voices/&lt;模型&gt;/*.onnx + .onnx.json），输入文本直接合成。</li>
            <li>默认中文音色 huayan；英文可用 lessac；支持批量文本队列。</li>
          </ul>
          <h2>11. TTS 批量 / TTS 数据集 / TTS 音源炼制</h2>
          <ul>
            <li>TTS 批量：长文本/多段文本批量合成，产物在 artifacts。</li>
            <li>TTS 数据集：准备训练数据（切片、文本、清洗后的 metadata.cleaned.csv）。</li>
            <li>TTS 音源炼制：基于 tts-train runtime 训练/微调音色并导出 onnx；质量调优按项目决定封存，当前优先可用性。</li>
          </ul>
          <h2>12. MSVC 工具</h2>
          <ul>
            <li>把 .msvc 文件转换为 .pth（内嵌模型），转换后可直接作为 RVC 音色使用。</li>
            <li>index 可选；缺失时 index_rate 自动按 0 处理。</li>
          </ul>
          <h2>13. 任务系统与产物</h2>
          <ul>
            <li>所有任务进 Gateway 串行 GPU 队列：可看进度/阶段、取消、重试、SSE 实时更新。</li>
            <li>产物统一 data/artifacts/&lt;job_id&gt;/；任务历史在 data/config/state.db。</li>
          </ul>
          <h2>14. 本地优先与隐私</h2>
          <ul>
            <li>音频/模型/工程/日志全部在安装根 data；不写 C 盘用户目录；Gateway 只监听 127.0.0.1 + Bearer token。</li>
            <li>不联网也能做主要音频处理；只有首次安装组件/更新需要联网。</li>
          </ul>
          <h2>15. 硬件与快速上手</h2>
          <ul>
            <li>推荐 Windows + NVIDIA GPU（CUDA）；RVC/TTS/pymss 都走 GPU，显存不足时降 batch/分段处理。</li>
            <li>首次使用：安装组件 → 启动 Gateway → 启动内核并安装插件 → 重启内核 → 打开“AMS 音乐”进入 14 页签。</li>
            <li>最短工作流：数据集工作室准备 → 音源炼制 → 人声分离 → 歌曲换源 → 混音母带 → 评测器确认。</li>
          </ul>
          <h2>16. 已知限制</h2>
          <ul>
            <li>GitHub 大文件下载偶发中断（可重试；组件已安装会跳过）；启动器尚未做代码签名。</li>
            <li>TTS 训练听感调优封存；ASR 使用 Vosk 小模型（用于路由/校验，不是最终字幕）。</li>
            <li>内核任务栏程序图标属于 Kernel 可执行文件，插件只能改界面内 logo/标题，不能改 exe 图标。</li>
          </ul>
        </div>
      </section>
    </div>
  </main>
</template>

<style>
:root { color-scheme: dark; }
* { box-sizing: border-box; }
body { margin: 0; background: #14161a; color: #e6e8eb; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.app { max-width: 1120px; margin: 0 auto; padding: 28px 32px; }
header { display: flex; justify-content: space-between; align-items: flex-end; margin-bottom: 22px; gap: 12px; }
.header-actions { display: flex; align-items: center; gap: 12px; }
.guide-button { margin-top: 0; background: #1f2a3a; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 6px 12px; font-size: 12px; cursor: pointer; }
.guide-overlay { position: fixed; inset: 0; z-index: 100; background: rgba(5,8,12,.78); display: flex; align-items: center; justify-content: center; padding: 24px; }
.guide-panel { width: min(900px, 96vw); max-height: 88vh; overflow: auto; background: #1b1e24; border: 1px solid #33445c; border-radius: 12px; padding: 22px 26px; box-shadow: 0 24px 80px rgba(0,0,0,.5); }
.guide-head { display: flex; justify-content: space-between; gap: 16px; align-items: flex-start; border-bottom: 1px solid #2a2f38; padding-bottom: 12px; margin-bottom: 14px; }
.guide-head h1 { font-size: 22px; }
.guide-close { margin-top: 0; background: transparent; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 5px 12px; cursor: pointer; }
.guide-body h2 { margin: 18px 0 6px; color: #cfd6de; font-size: 14px; }
.guide-body p, .guide-body li { color: #b6c0cc; font-size: 13px; line-height: 1.65; }
.guide-body ul, .guide-body ol { margin: 4px 0 8px; padding-left: 20px; }
.guide-body li { display: list-item; list-style: inherit; border: 0; padding: 2px 0; justify-content: flex-start; }
.guide-body a { color: #8fb8ff; }
.brand-head { display: flex; align-items: center; gap: 14px; }
.brand-logo { width: 64px; height: 64px; border-radius: 50%; box-shadow: 0 6px 24px rgba(0,0,0,.45); }
h1 { margin: 0; font-size: 30px; font-weight: 650; letter-spacing: .5px; }
.sub { margin: 6px 0 0; color: #9aa3ad; font-size: 13px; }
.tag { background: #2a3a52; color: #9cc4ff; border-radius: 4px; padding: 1px 6px; font-size: 11px; }
.ver { color: #7d8790; font-size: 12px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
.card { background: #1b1e24; border: 1px solid #2a2f38; border-radius: 10px; padding: 18px 20px; }
.card.wide { grid-column: 1 / -1; }
h2 { margin: 0 0 12px; font-size: 15px; color: #cfd6de; font-weight: 600; }
ul { list-style: none; margin: 0; padding: 0; }
li { display: flex; flex-wrap: wrap; gap: 8px; justify-content: space-between; padding: 7px 0; border-bottom: 1px solid #232830; font-size: 13px; }
li:last-child { border-bottom: 0; }
li b { font-weight: 600; color: #dfe6ee; word-break: break-all; text-align: right; }
li small { flex-basis: 100%; color: #7f8994; word-break: break-all; }
.ok { color: #6fd08c; } .warn { color: #e0b25a; } .bad { color: #d97a7a; }
.err { color: #e07777; font-size: 12px; }
.note { color: #8a949e; font-size: 12px; margin-top: 10px; }
input { background: #14161a; color: #dfe6ee; border: 1px solid #2a2f38; border-radius: 4px; padding: 4px 6px; font-size: 12px; width: 60%; }
.wide-input { width: 100%; margin-bottom: 6px; }
.job { margin-top: 8px; font-size: 13px; color: #c4ccd5; }
.job .err { display: block; color: #e07777; font-size: 12px; margin-top: 4px; }
.progress { margin: 10px 0 4px; }
.progress-head { display: flex; justify-content: space-between; gap: 12px; color: #c4ccd5; font-size: 12px; margin-bottom: 5px; }
.progress-head b { color: #9fb6d4; font-variant-numeric: tabular-nums; }
.progress-track { height: 8px; background: #14161a; border: 1px solid #2a2f38; border-radius: 999px; overflow: hidden; }
.progress-fill { height: 100%; background: linear-gradient(90deg, #3f6fbd, #5b8def); transition: width .35s ease; }
.progress small { color: #7f8994; font-size: 11px; }
.row { display: flex; gap: 8px; flex-wrap: wrap; }
button { margin-top: 12px; background: #263244; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 7px 12px; }
button:disabled { opacity: .45; }
button.mini { margin: 0 0 0 6px; padding: 3px 8px; font-size: 12px; }
</style>