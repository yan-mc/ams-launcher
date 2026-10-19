<script setup lang="ts">
import { ref } from "vue";

const version = "0.1.0 (M0)";
const status = ref([
  { name: "内核（DSH fork）", state: "未安装", detail: "等待 M0.1 构建" },
  { name: "Gateway", state: "未检测", detail: "等待 M0.3" },
  { name: "音源库", state: "待扫描", detail: "默认音源：曼波" },
  { name: "更新中心", state: "占位", detail: "M0.8" },
]);
const checks = ref([
  { name: "Windows", value: "Windows 11 x64" },
  { name: "GPU", value: "RTX 4080 Laptop · 12GB" },
  { name: "Python", value: "3.12（系统）" },
  { name: "Git", value: "2.55" },
]);
</script>

<template>
  <main class="app">
    <header>
      <div>
        <h1>AMS Studio</h1>
        <p class="sub">AI Music Studio · Launcher <span class="tag">M0</span></p>
      </div>
      <span class="ver">v{{ version }}</span>
    </header>

    <section class="grid">
      <div class="card">
        <h2>环境检测</h2>
        <ul>
          <li v-for="c in checks" :key="c.name"><span>{{ c.name }}</span><b>{{ c.value }}</b></li>
        </ul>
        <button disabled>重新检测（M0.2）</button>
      </div>
      <div class="card">
        <h2>组件状态</h2>
        <ul>
          <li v-for="s in status" :key="s.name">
            <span>{{ s.name }}</span>
            <b :class="s.state === '正常' ? 'ok' : 'warn'">{{ s.state }}</b>
            <small>{{ s.detail }}</small>
          </li>
        </ul>
      </div>
      <div class="card wide">
        <h2>下一步（M0）</h2>
        <ol>
          <li>M0.1 DSH fork 构建</li>
          <li>M0.3 Gateway MVP（任务/SSE/音源扫描）</li>
          <li>M0.4 音色库 + 默认曼波</li>
          <li>M0.5 MSVC → pth 转换器 3/3</li>
          <li>M0.6 音乐插件最小闭环</li>
        </ol>
        <p class="note">M0 只做验证骨架，不含完整 UI；完整页面在 M1+。</p>
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
li b { font-weight: 600; color: #dfe6ee; }
li small { flex-basis: 100%; color: #7f8994; }
.ok { color: #6fd08c; } .warn { color: #e0b25a; }
button { margin-top: 12px; background: #263244; color: #9fb6d4; border: 1px solid #33445c; border-radius: 6px; padding: 7px 12px; }
button:disabled { opacity: .55; }
ol { margin: 0; padding-left: 20px; color: #c4ccd5; font-size: 13px; line-height: 1.9; }
.note { color: #7f8994; font-size: 12px; margin-top: 10px; }
</style>
