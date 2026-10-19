<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

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
const kernel = ref<KernelInfo | null>(null);
const kernelMsg = ref("");
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
    components.value = await invoke<ComponentItem[]>("component_status");
    if (gateway.value?.healthy) await scanVoices();
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

async function refreshAll() {
  await Promise.all([refreshEnv(), refreshGateway(), refreshKernel()]);
}

onMounted(() => {
  refreshAll();
  timer = window.setInterval(() => { refreshGateway(); refreshKernel(); }, 5000);
});
onUnmounted(() => {
  if (timer !== undefined) window.clearInterval(timer);
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
        </div>
        <p class="note">默认音源从 AMS_DEFAULT_VOICE_DIR（默认 F:\ai歌曲\0 曼波）复制到 AMS_HOME/voices/曼波/，不进 Git/安装包。</p>
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
.row { display: flex; gap: 8px; flex-wrap: wrap; }
button { margin-top: 12px; background: #263244; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 7px 12px; }
button:disabled { opacity: .45; }
</style>