import "./style.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface QuotaWindow {
  label: string;
  used_percent: number;
  remaining_percent: number;
  used_text: string | null;
  resets_at_ms: number | null;
}

interface ResetCredit {
  index: number;
  expires_at_ms: number | null;
}

interface ProviderQuota {
  id: string;
  name: string;
  ok: boolean;
  error: string | null;
  plan: string | null;
  extra: string | null;
  windows: QuotaWindow[];
  credits: ResetCredit[];
  fetched_at_ms: number;
}

interface AppConfig {
  glm_key: string;
  glm_base: string;
  kimi_key: string;
  kimi_web_token: string;
  deepseek_key: string;
  codex_name: string;
  luchikey_base: string;
  luchikey_key: string;
  refresh_minutes: number;
}

interface ConfigView {
  config: AppConfig;
  glm_key_detected: boolean;
  codex_auth_found: boolean;
}

interface DeviceAuthStart {
  user_code: string;
  verification_url: string;
  interval: number;
}

interface KimiLoginStatus {
  logged_in: boolean;
  expires_at_ms: number | null;
}

const PROVIDER_COLORS: Record<string, string> = {
  glm: "#6f96e8",
  kimi: "#9182e8",
  codex: "#4fb893",
  deepseek: "#6f8fe0",
  luchikey: "#d9a441",
};

/** 品牌 logo（Simple Icons，24x24 viewBox） */
const PROVIDER_LOGOS: Record<string, string> = {
  glm: "M12.606 1.806l-1.677 2.388c-0.258 0.374-0.697 0.606-1.161 0.606h-9.162V1.794C0.594 1.806 12.606 1.806 12.606 1.806zM24 1.806L9.6 22.206 0 22.206 14.4 1.806zM11.394 22.206l1.69-2.4c0.258-0.374 0.697-0.606 1.161-0.606h9.149v3.006H11.394z",
  kimi: "M21.765.351C22.998.351 24 1.353 24 2.586S22.998 4.82 21.765 4.82h-1.974c-.15 0-.26-.12-.26-.26V2.586A2.237 2.237 0 0 1 21.765.35M9.41 13.388l8.447-8.377c.16-.16.07-.471-.14-.471h-4.55s-.1.02-.14.06l-9.099 9.029c-.14.14-.35.02-.35-.21V4.81c0-.15-.1-.27-.221-.27H.22c-.12 0-.22.12-.22.27v18.57c0 .15.1.27.22.27h3.137c.12 0 .22-.12.22-.27v-3.79c0-.08.03-.16.08-.21l2.826-2.796c.07-.07.16-.08.241-.03l7.546 5.551a8.9 8.9 0 0 0 4.018 1.493c.12.01.23-.11.23-.27V19.76c0-.14-.08-.25-.19-.26a5.8 5.8 0 0 1-2.355-.942l-6.533-4.73c-.14-.09-.15-.32-.03-.441",
  codex: "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z",
  deepseek: "M23.748 4.651c-.254-.124-.364.113-.512.233-.051.04-.094.09-.137.137-.372.397-.806.657-1.373.626-.829-.046-1.537.214-2.163.848-.133-.782-.575-1.248-1.247-1.548-.352-.155-.708-.311-.955-.65-.172-.24-.219-.509-.305-.774-.055-.16-.11-.323-.293-.35-.2-.031-.278.136-.356.276-.313.572-.434 1.202-.422 1.84.027 1.436.633 2.58 1.838 3.393.137.094.172.187.129.323-.082.28-.18.553-.266.833-.055.179-.137.218-.328.14a5.5 5.5 0 0 1-1.737-1.179c-.857-.828-1.631-1.743-2.597-2.46a12 12 0 0 0-.689-.47c-.985-.957.13-1.743.387-1.836.27-.098.094-.433-.778-.428-.872.003-1.67.295-2.687.685a3 3 0 0 1-.465.136 9.6 9.6 0 0 0-2.883-.101c-1.885.21-3.39 1.1-4.497 2.622C.082 8.776-.231 10.854.152 13.02c.403 2.284 1.568 4.175 3.36 5.653 1.857 1.533 3.997 2.284 6.438 2.14 1.482-.085 3.132-.284 4.994-1.86.47.234.962.328 1.78.398.629.058 1.235-.031 1.705-.129.735-.155.684-.836.418-.961-2.155-1.004-1.682-.595-2.112-.926 1.095-1.295 2.768-3.598 3.284-6.733.05-.346.115-.834.108-1.114-.004-.171.035-.238.23-.257a4.2 4.2 0 0 0 1.545-.475c1.397-.763 1.96-2.016 2.093-3.517.02-.23-.004-.467-.247-.588M11.58 18.168c-2.088-1.642-3.101-2.183-3.52-2.16-.39.024-.32.472-.234.763.09.288.207.487.371.74.114.167.192.416-.113.603-.673.416-1.842-.14-1.897-.168-1.361-.801-2.5-1.86-3.301-3.306-.775-1.393-1.225-2.888-1.299-4.482-.02-.385.094-.522.477-.592a4.7 4.7 0 0 1 1.53-.038c2.131.311 3.946 1.264 5.467 2.774.868.86 1.525 1.887 2.202 2.89.72 1.066 1.494 2.082 2.48 2.915.348.291.626.513.892.677-.802.09-2.14.109-3.055-.615zm1.001-6.44a.306.306 0 0 1 .415-.287.3.3 0 0 1 .113.074.3.3 0 0 1 .086.214c0 .17-.136.307-.308.307a.303.303 0 0 1-.306-.307m3.11 1.596c-.2.081-.4.151-.591.16a1.25 1.25 0 0 1-.798-.254c-.274-.23-.47-.358-.551-.758a1.7 1.7 0 0 1 .015-.588c.07-.327-.007-.537-.238-.727-.188-.156-.426-.199-.689-.199a.6.6 0 0 1-.254-.078.253.253 0 0 1-.114-.358 1 1 0 0 1 .192-.21c.356-.202.767-.136 1.146.016.352.144.618.408 1.001.782.392.451.462.576.685.915.176.264.336.536.446.848.066.194-.02.353-.25.45",
};

function logoSvg(id: string, color: string): string {
  const d = PROVIDER_LOGOS[id];
  if (!d) return `<span class="dot" style="background:${color}"></span>`;
  return `<span class="logo" style="color:${color}"><svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="${d}"/></svg></span>`;
}

const PLAN_LABELS: Record<string, string> = {
  lite: "Lite",
  pro: "Pro",
  max: "Max",
  plus: "Plus",
  proplus: "Pro+",
};

let quotas: ProviderQuota[] = [];
let cfgView: ConfigView | null = null;
let kimiLogin: KimiLoginStatus | null = null;
let view: "main" | "settings" = "main";
let refreshing = false;
let refreshTimer: number | undefined;
let clockTimer: number | undefined;
let kimiPollTimer: number | undefined;

const ESC_MAP: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
  "'": "&#39;",
};

function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => ESC_MAP[c] ?? c);
}

function fmtDuration(ms: number): string {
  if (ms <= 0) return "即将重置";
  const m = Math.floor(ms / 60000);
  if (m < 1) return "<1分钟";
  if (m < 60) return `${m}分钟`;
  const h = Math.floor(m / 60);
  const d = Math.floor(h / 24);
  const hh = h % 24;
  if (d > 0) return `${d}天${hh}小时`;
  return `${h}小时${m % 60}分`;
}

/** 展示用倒计时：追加「后重置」后缀 */
function fmtReset(ms: number): string {
  const d = fmtDuration(ms);
  return d === "即将重置" ? d : `${d}后重置`;
}

function barColor(remaining: number): string {
  if (remaining >= 60) return "#5fb877";
  if (remaining >= 25) return "#c9a86a";
  return "#c97a7a";
}

function fmtMinute(ts: number): string {
  const d = new Date(ts);
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function creditsHtml(p: ProviderQuota): string {
  if (!p.credits?.length) return "";
  return p.credits
    .map(
      (c) =>
        `<div class="credit-line">重置卡${c.index} 到期时间：${
          c.expires_at_ms ? fmtMinute(c.expires_at_ms) : "永不过期"
        }</div>`
    )
    .join("");
}

function planBadge(p: ProviderQuota): string {
  if (!p.plan) return "";
  const label = PLAN_LABELS[p.plan.toLowerCase()] ?? p.plan.toUpperCase();
  return `<span class="plan">${esc(label)}</span>`;
}

function windowHtml(w: QuotaWindow): string {
  const remain = Math.round(w.remaining_percent);
  const color = barColor(remain);
  const reset = w.resets_at_ms
    ? `<span class="reset" data-reset="${w.resets_at_ms}">${fmtReset(w.resets_at_ms - Date.now())}</span>`
    : `<span class="reset muted">重置时间未知</span>`;
  return `
    <div class="window">
      <div class="window-top">
        <span class="wlabel">${esc(w.label)}</span>
        <span class="remain" style="color:${color}">剩 ${remain}%</span>
      </div>
      <div class="bar"><div class="fill" style="width:${remain}%;background:${color}"></div></div>
      <div class="window-bottom">
        <span class="used">${w.used_text ? esc(w.used_text) : `已用 ${Math.round(w.used_percent)}%`}</span>
        ${reset}
      </div>
    </div>`;
}

function cardHtml(p: ProviderQuota): string {
  const color = PROVIDER_COLORS[p.id] ?? "#888";
  let body: string;
  if (p.ok) {
    if (p.windows.length > 0) {
      body = p.windows.map(windowHtml).join("");
      body += creditsHtml(p);
      if (p.extra) body += `<div class="extra">${esc(p.extra)}</div>`;
    } else if (p.extra) {
      // 余额类卡片（如 DeepSeek）：无窗口，第一行大字 + 其余小字
      const lines = p.extra.split("\n");
      body = `<div class="balance">${esc(lines[0])}</div>${lines
        .slice(1)
        .map((l) => `<div class="balance-sub">${esc(l)}</div>`)
        .join("")}`;
    } else {
      body = `<div class="error">未返回数据</div>`;
    }
  } else {
    body = `<div class="error">${esc(p.error ?? "查询失败")}</div>`;
  }
  return `
    <div class="card" data-card="${p.id}" style="border-left-color:${color}">
      <div class="card-head" data-tauri-drag-region>
        ${logoSvg(p.id, color)}
        <span class="pname">${esc(p.name)}</span>
        ${planBadge(p)}
      </div>
      <div class="card-body">${body}</div>
    </div>`;
}

function renderMain(): string {
  const latest = quotas.length ? Math.max(...quotas.map((q) => q.fetched_at_ms || 0)) : 0;
  const timeText = latest
    ? new Date(latest).toLocaleTimeString("zh-CN", { hour12: false })
    : "--:--:--";
  return `
    <div class="head" data-tauri-drag-region>
      <span class="title" data-tauri-drag-region>AI Quota</span>
      <span class="upd" data-tauri-drag-region>${timeText}</span>
      <span class="spacer" data-tauri-drag-region></span>
      <button id="btn-refresh" class="icon-btn" title="立即刷新 ${refreshing ? "（进行中）" : ""}">${refreshing ? "⟳" : "⟳"}</button>
      <button id="btn-settings" class="icon-btn" title="设置">⚙</button>
      <button id="btn-hide" class="icon-btn" title="隐藏到托盘">—</button>
    </div>
    <div class="cards" id="cards">
      ${quotas.length ? quotas.map(cardHtml).join('<div class="divider"></div>') : `<div class="loading">正在查询…</div>`}
    </div>`;
}

function kimiStateText(): string {
  if (!kimiLogin?.logged_in) return "未登录";
  const exp = kimiLogin.expires_at_ms;
  const t = exp ? new Date(exp).toLocaleString("zh-CN", { hour12: false }) : "";
  return `已登录（access 续期至 ${t}，自动续命）`;
}

async function startKimiLogin() {
  const flow = document.getElementById("kimi-login-flow");
  const btn = document.getElementById("btn-kimi-login") as HTMLButtonElement | null;
  if (!flow) return;
  try {
    if (btn) btn.disabled = true;
    const start = await invoke<DeviceAuthStart>("kimi_login_start");
    flow.innerHTML = `
      <div class="login-box">
        <div>1. 已复制用户码，浏览器将打开授权页</div>
        <div class="user-code">${esc(start.user_code)}</div>
        <div>2. 在页面中确认授权（码 10 分钟内有效）</div>
        <div class="login-wait">等待授权中…</div>
      </div>`;
    try {
      await navigator.clipboard.writeText(start.user_code);
    } catch {
      /* 剪贴板不可用就忽略 */
    }
    const win = window.open(start.verification_url, "_blank");
    if (!win) {
      flow.innerHTML += `<div class="hint">弹窗被拦截：<a href="${esc(start.verification_url)}" target="_blank">点此打开授权页</a></div>`;
    }
    if (kimiPollTimer) window.clearInterval(kimiPollTimer);
    kimiPollTimer = window.setInterval(async () => {
      try {
        const done = await invoke<KimiTokens | null>("kimi_login_poll");
        if (done) {
          window.clearInterval(kimiPollTimer);
          kimiPollTimer = undefined;
          kimiLogin = await invoke<KimiLoginStatus>("kimi_login_status");
          render();
          void refresh();
        }
      } catch (e) {
        window.clearInterval(kimiPollTimer);
        kimiPollTimer = undefined;
        const box = flow.querySelector(".login-wait");
        if (box) box.textContent = String(e);
        if (btn) btn.disabled = false;
      }
    }, (start.interval || 5) * 1000);
  } catch (e) {
    flow.innerHTML = `<div class="hint">发起登录失败：${esc(String(e))}</div>`;
    if (btn) btn.disabled = false;
  }
}

interface KimiTokens {
  access_token: string;
}

function renderSettings(): string {
  const c = cfgView?.config ?? { glm_key: "", glm_base: "", kimi_key: "", kimi_web_token: "", deepseek_key: "", codex_name: "", luchikey_base: "", luchikey_key: "", refresh_minutes: 10 };
  const detected = cfgView?.glm_key_detected;
  return `
    <div class="head" data-tauri-drag-region>
      <span class="title" data-tauri-drag-region>设置</span>
      <span class="spacer" data-tauri-drag-region></span>
      <button id="btn-back" class="icon-btn" title="返回">←</button>
    </div>
    <div class="form">
      <label>智谱 GLM API Key</label>
      ${detected ? `<div class="hint ok">已从 ZCode 配置自动读取</div>` : ""}
      <input id="in-glm" type="password" placeholder="${detected ? "（自动检测）" : "sk-… / Coding Plan Key"}" value="${esc(c.glm_key)}" spellcheck="false"/>
      <label>Kimi 账号（设备码登录，推荐）</label>
      <div class="login-row">
        <span id="kimi-login-state" class="hint">${kimiStateText()}</span>
        <button id="btn-kimi-login" class="mini-btn">${kimiLogin?.logged_in ? "重新登录" : "登录 Kimi"}</button>
        ${kimiLogin?.logged_in ? `<button id="btn-kimi-logout" class="mini-btn subtle">退出</button>` : ""}
      </div>
      <div id="kimi-login-flow"></div>
      <label>Kimi API Key（可选，5小时/周额度）</label>
      <input id="in-kimi" type="password" placeholder="Kimi Coding 密钥" value="${esc(c.kimi_key)}" spellcheck="false"/>
      <label>Kimi 网页 Token（可选，手动覆盖登录态）</label>
      <input id="in-kimi-web" type="password" placeholder="留空 = 使用上方登录的账号" value="${esc(c.kimi_web_token)}" spellcheck="false"/>
      <div class="hint">月度总量池只能用网页登录态查询：登录 kimi.com 后，F12 → 应用 → Cookie → 复制 kimi-auth 的完整值粘贴到这里</div>
      <label>DeepSeek API Key（余额）</label>
      <input id="in-deepseek" type="password" placeholder="sk-…" value="${esc(c.deepseek_key)}" spellcheck="false"/>
      <label>ChatGPT 显示名</label>
      <input id="in-codex-name" type="text" placeholder="如 ChatGPT Pro 20x（接口只返回 pro 档位）" value="${esc(c.codex_name)}" spellcheck="false"/>
      <label>Luchikey API Key（中转余额）</label>
      <input id="in-lk-key" type="password" placeholder="sk-…（sub2api 的 key）" value="${esc(c.luchikey_key)}" spellcheck="false"/>
      <label>自动刷新间隔</label>
      <select id="in-interval">
        ${[1, 5, 10, 15, 30, 60]
          .map((m) => `<option value="${m}" ${c.refresh_minutes === m ? "selected" : ""}>${m === 1 ? "1 分钟（调试）" : `${m} 分钟`}</option>`)
          .join("")}
      </select>
      <div class="hint">ChatGPT：检测到 ${cfgView?.codex_auth_found ? "✅ 已登录 Codex" : "❌ 未登录（需先 codex login）"}</div>
      <button id="btn-save" class="save">保存</button>
      <div class="hint" id="save-msg"></div>
    </div>`;
}

function render() {
  const app = document.getElementById("app")!;
  app.innerHTML = view === "settings" ? renderSettings() : renderMain();

  if (view === "main") {
    app.querySelector("#btn-refresh")?.addEventListener("click", () => void refresh());
    app.querySelector("#btn-settings")?.addEventListener("click", () => {
      view = "settings";
      render();
    });
    app.querySelector("#btn-hide")?.addEventListener("click", () => void getCurrentWindow().hide());
    updateTimes();
  } else {
    app.querySelector("#btn-back")?.addEventListener("click", () => {
      if (kimiPollTimer) {
        window.clearInterval(kimiPollTimer);
        kimiPollTimer = undefined;
      }
      view = "main";
      render();
    });
    app.querySelector("#btn-kimi-login")?.addEventListener("click", () => void startKimiLogin());
    app.querySelector("#btn-kimi-logout")?.addEventListener("click", async () => {
      await invoke("kimi_logout");
      kimiLogin = await invoke<KimiLoginStatus>("kimi_login_status");
      render();
    });
    app.querySelector("#btn-save")?.addEventListener("click", () => void saveSettings());
  }
}

function updateTimes() {
  document.querySelectorAll<HTMLElement>(".reset").forEach((el) => {
    const t = Number(el.dataset.reset);
    if (t) el.textContent = fmtReset(t - Date.now());
  });
}

async function refresh() {
  if (refreshing) return;
  refreshing = true;
  const btn = document.getElementById("btn-refresh");
  if (btn) btn.classList.add("spin");
  try {
    quotas = await invoke<ProviderQuota[]>("query_all");
  } catch (e) {
    quotas = [
      { id: "err", name: "查询失败", ok: false, error: String(e), plan: null, extra: null, windows: [], credits: [], fetched_at_ms: Date.now() },
    ];
  }
  refreshing = false;
  if (view === "main") {
    render();
    const nb = document.getElementById("btn-refresh");
    nb?.classList.remove("spin");
  }
}

async function saveSettings() {
  const glm = (document.getElementById("in-glm") as HTMLInputElement)?.value.trim() ?? "";
  const kimi = (document.getElementById("in-kimi") as HTMLInputElement)?.value.trim() ?? "";
  const kimiWeb = (document.getElementById("in-kimi-web") as HTMLInputElement)?.value.trim() ?? "";
  const deepseek = (document.getElementById("in-deepseek") as HTMLInputElement)?.value.trim() ?? "";
  const codexName = (document.getElementById("in-codex-name") as HTMLInputElement)?.value.trim() ?? "";
  const lkKey = (document.getElementById("in-lk-key") as HTMLInputElement)?.value.trim() ?? "";
  const minutes = Number((document.getElementById("in-interval") as HTMLSelectElement)?.value ?? 10);
  const cfg: AppConfig = {
    glm_key: glm || cfgView?.config.glm_key || "",
    glm_base: cfgView?.config.glm_base || "",
    kimi_key: kimi,
    kimi_web_token: kimiWeb,
    deepseek_key: deepseek || cfgView?.config.deepseek_key || "",
    codex_name: codexName,
    luchikey_base: cfgView?.config.luchikey_base || "",
    luchikey_key: lkKey || cfgView?.config.luchikey_key || "",
    refresh_minutes: minutes,
  };
  try {
    await invoke("set_config", { newCfg: cfg });
    cfgView = await invoke<ConfigView>("get_config");
    scheduleAutoRefresh();
    view = "main";
    render();
    await refresh();
  } catch (e) {
    const msg = document.getElementById("save-msg");
    if (msg) msg.textContent = `保存失败：${e}`;
  }
}

function scheduleAutoRefresh() {
  if (refreshTimer) window.clearInterval(refreshTimer);
  const minutes = cfgView?.config.refresh_minutes ?? 10;
  refreshTimer = window.setInterval(() => void refresh(), Math.max(1, minutes) * 60_000);
}

async function init() {
  try {
    cfgView = await invoke<ConfigView>("get_config");
  } catch {
    cfgView = null;
  }
  try {
    kimiLogin = await invoke<KimiLoginStatus>("kimi_login_status");
  } catch {
    kimiLogin = null;
  }
  render();
  scheduleAutoRefresh();
  clockTimer = window.setInterval(updateTimes, 30_000);

  await listen("refresh-requested", () => {
    void refresh();
  });

  await refresh();
}

void init();
