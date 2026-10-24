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
  const ok = window.confirm("确定卸载 AMS Studio？\n\n只删除 AMS 的安装目录和 AMS 数据目录中的 kernel/components/runtime/logs；voices、artifacts 和现有 DSH 的 .dsh 不会被删除。");
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
      <div>
        <h1>AMS Studio</h1>
        <p class="sub">AI Music Studio · Launcher <span class="tag">M1</span></p>
      </div>
      <span class="ver">v{{ version }}</span>
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
        <button @click="requestUninstall">卸载 AMS Studio…</button>
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
          <button :disabled="pluginBusy" @click="installPlugins">安装音乐插件到内核</button>
        </div>
        <p v-if="pluginMsg" class="note">{{ pluginMsg }}</p>
        <p class="note">插件安装需要内核 profile 已初始化：先启动一次 AMS Kernel，停止后再点“安装音乐插件到内核”。</p>
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
  </main>
</template>

<style>
:root { color-scheme: dark; }
* { box-sizing: border-box; }
body { margin: 0; background: #14161a; color: #e6e8eb; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.app { max-width: 1120px; margin: 0 auto; padding: 28px 32px; }
header { display: flex; justify-content: space-between; align-items: flex-end; margin-bottom: 22px; }
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