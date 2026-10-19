<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

type EnvItem = { label: string; value: string; ok: boolean };

const version = ref("0.1.1 (M0.2+)");
const loading = ref(false);
const error = ref("");
const checks = ref<EnvItem[]>([
  { label: "环境检测", value: "正在读取…", ok: false },
]);
const status = ref([
  { name: "内核（DSH fork）", state: "已构建", detail: "M0.1 构建通过；启动集成待做" },
  { name: "Gateway", state: "已实现", detail: "M0.3 23 测试通过；启动集成待做" },
  { name: "音源库", state: "已校验", detail: "默认音源：曼波（M0.4）" },
  { name: "更新中心", state: "工具就绪", detail: "签名工具 M0.8；UI 待接" },
]);

async function refresh() {
  loading.value = true;
  error.value = "";
  try {
    checks.value = await invoke<EnvItem[]>("get_environment");
    const v = await invoke<string>("app_version");
    version.value = v + " (M0.2+)";
  } catch (e) {
    error.value = String(e);
    checks.value = [
      { label: "运行环境", value: "浏览器预览模式（无法读取本机信息）", ok: false },
    ];
  } finally {
    loading.value = false;
  }
}

onMounted(refresh);
</script>

<template>
  <main class="app">
    <header>
      <div>
        <h1>AMS Studio</h1>
        <p class="sub">AI Music Studio · Launcher <span class="tag">M0.2+</span></p>
      </div>
      <span class="ver">v{{ version }}</span>
    </header>

    <section class="grid">
      <div class="card">
        <h2>环境检测</h2>
        <p v-if="error" class="err">读取失败：{{ error }}</p>
        <ul>
          <li v-for="c in checks" :key="c.label">
            <span>{{ c.label }}</span>
            <b :class="c.ok ? 'ok' : 'warn'">{{ c.value }}</b>
          </li>
        </ul>
        <button :disabled="loading" @click="refresh">{{ loading ? "检测中…" : "重新检测" }}</button>
      </div>
      <div class="card">
        <h2>组件状态</h2>
        <ul>
          <li v-for="s in status" :key="s.name">
            <span>{{ s.name }}</span>
            <b :class="s.state.includes('已') ? 'ok' : 'warn'">{{ s.state }}</b>
            <small>{{ s.detail }}</small>
          </li>
        </ul>
      </div>
      <div class="card wide">
        <h2>M0 已完成</h2>
        <ol>
          <li>工具链 / 7 个私有仓库</li>
          <li>DSH fork 构建 + Web 200</li>
          <li>启动器 exe + NSIS + MSI</li>
          <li>Gateway 23 测试 / 默认曼波音源</li>
          <li>MSVC → pth 3/3 / 插件闭环 12/12</li>
          <li>RVC 换声 / pymss CPU 分离冒烟</li>
        </ol>
        <p class="note">下一步：M0.9 CI/签名/备份 → 启动器接 Gateway/内核 → M1 功能页面。</p>
      </div>
    </section>
  </main>
</template>

<style>
:root { color-scheme: dark; }
* { box-sizing: border-box; }
body { margin: 0; background: #14161a; color: #e6e8eb; font-family: "Segoe UI", "Microsoft YaHei", sans-serif; }
.app { max-width: 1080px; margin: 0 auto; padding: 28px 32px; }
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
li small { flex-basis: 100%; color: #7f8994; }
.ok { color: #6fd08c; } .warn { color: #e0b25a; }
.err { color: #e07777; font-size: 12px; }
button { margin-top: 12px; background: #263244; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 7px 12px; }
button:disabled { opacity: .55; }
ol { margin: 0; padding-left: 20px; color: #c4ccd5; font-size: 13px; line-height: 1.9; }
.note { color: #7f8994; font-size: 12px; margin-top: 10px; }
</style>
