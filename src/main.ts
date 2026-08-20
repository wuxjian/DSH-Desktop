import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface ToolchainStatus {
  nodeFound: boolean;
  npmFound: boolean;
  npmCmd: string | null;
  dshFound: boolean;
  dshCmd: string | null;
  dshVersion: string | null;
}

type WebStatus = "notInstalled" | "stopped" | "starting" | "running" | "failed";

interface UpdateInfo {
  currentVersion: string | null;
  latestVersion: string | null;
  updateAvailable: boolean;
  lastCheckDate: string | null;
  dismissedVersion: string | null;
  lastError: string | null;
}

interface StatusPayload {
  toolchain: ToolchainStatus;
  webStatus: WebStatus;
  failedReason: string | null;
  webPort: number;
  theme: "dark" | "light" | "system";
  update: UpdateInfo;
  /** 服务是否可由桌面端重启/停止(本实例拉起,或端口由本机 node 进程承载) */
  managed: boolean;
  /** managed 为 false 时,占用 web 端口的进程映像名(如 WSL 中继 wslrelay.exe) */
  externalProcess: string | null;
}

interface ProcLogEvent {
  source: string;
  line: string;
}

interface WebStatusEvent {
  status: WebStatus;
}

interface ThemeEvent {
  preference: "dark" | "light" | "system";
}

interface UpgradeDoneEvent {
  success: boolean;
}

function $<T extends HTMLElement>(selector: string): T {
  const el = document.querySelector<T>(selector);
  if (!el) throw new Error(`missing element: ${selector}`);
  return el;
}

const overlay = $("#overlay");
const overlayTitle = $("#overlay-title");
const overlayDesc = $("#overlay-desc");
const overlaySpinner = $("#overlay-spinner");
const logPre = $("#log-pre");
const frame = $("#app-frame") as HTMLIFrameElement;
const statusDot = $("#status-dot");
const statusText = $("#status-text");
const versionText = $("#version-text");
const btnCheck = $("#btn-check") as HTMLButtonElement;
const btnUpgrade = $("#btn-upgrade");
const btnRestart = $("#btn-restart");
const btnStop = $("#btn-stop");
const btnBrowser = $("#btn-browser") as HTMLButtonElement;
const btnRetry = $("#btn-retry");
const btnInstallDsh = $("#btn-install-dsh");
const btnOpenNodejs = $("#btn-open-nodejs");
const toast = $("#toast");
const toastVersion = $("#toast-version");
const toastCurrent = $("#toast-current");
const toastUpgrade = $("#toast-upgrade");
const toastLater = $("#toast-later");
const appWindow = getCurrentWindow();

let status: StatusPayload | null = null;
let themePref: "dark" | "light" | "system" = "system";
let frameMounted = false;
let busy = false;
/** 最近一次看到的运行中服务是否由桌面端不可管理的外部环境(如 WSL)承载 */
let externalLastSeen = false;
const logLines: string[] = [];

const STATUS_LABEL: Record<WebStatus, string> = {
  notInstalled: "未安装 dsh",
  stopped: "DeepSeek Harness 未启动",
  starting: "DeepSeek Harness 启动中…",
  running: "DeepSeek Harness 运行中",
  failed: "DeepSeek Harness 异常",
};

function appendLog(source: string, line: string) {
  logLines.push(`[${source}] ${line}`);
  if (logLines.length > 300) logLines.splice(0, logLines.length - 300);
  logPre.textContent = logLines.join("\n");
  logPre.scrollTop = logPre.scrollHeight;
}

function clearLogs() {
  logLines.length = 0;
  logPre.textContent = "";
}

function applyTheme(preference: "dark" | "light" | "system") {
  themePref = preference;
  const dark =
    preference === "dark" ||
    (preference === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

interface OverlayOptions {
  title: string;
  desc?: string;
  spinner?: boolean;
  retry?: boolean;
  installDsh?: boolean;
  openNodejs?: boolean;
}

function showOverlay(opts: OverlayOptions) {
  overlay.classList.remove("hidden");
  overlayTitle.textContent = opts.title;
  overlayDesc.textContent = opts.desc ?? "";
  overlaySpinner.classList.toggle("hidden", !opts.spinner);
  btnRetry.classList.toggle("hidden", !opts.retry);
  btnInstallDsh.classList.toggle("hidden", !opts.installDsh);
  btnOpenNodejs.classList.toggle("hidden", !opts.openNodejs);
}

function hideOverlay() {
  overlay.classList.add("hidden");
}

function mountFrame() {
  if (!status) return;
  const url = `http://127.0.0.1:${status.webPort}/`;
  if (frameMounted && frame.src === url) {
    hideOverlay();
    return;
  }
  frame.src = url;
  frameMounted = true;
}

function unmountFrame() {
  frame.src = "about:blank";
  frameMounted = false;
}

function renderStatusBar() {
  if (!status) return;
  const st = status.webStatus;
  statusDot.className = `dot dot-${st}`;
  statusText.textContent = STATUS_LABEL[st];
  const parts: string[] = [];
  if (status.update.currentVersion) parts.push(`dsh v${status.update.currentVersion}`);
  if (status.update.latestVersion) parts.push(`最新 v${status.update.latestVersion}`);
  if (status.update.updateAvailable) parts.push("有新版本");
  versionText.textContent = parts.length ? parts.join(" · ") : "版本未知";
  btnUpgrade.classList.toggle("hidden", !status.update.updateAvailable);
  btnRestart.classList.toggle("hidden", st !== "running");
  btnStop.classList.toggle("hidden", st !== "running");
  externalLastSeen = st === "running" && !status.managed;
  btnBrowser.disabled = st !== "running";
}

function showToast(info: UpdateInfo) {
  toastVersion.textContent = info.latestVersion ? `v${info.latestVersion}` : "";
  toastCurrent.textContent = info.currentVersion ? `v${info.currentVersion}` : "未知";
  toast.classList.remove("hidden");
}

function hideToast() {
  toast.classList.add("hidden");
}

function handleUpdate(info: UpdateInfo) {
  if (status) status.update = info;
  renderStatusBar();
  const shouldToast =
    info.updateAvailable && !!info.latestVersion && info.latestVersion !== info.dismissedVersion;
  if (shouldToast) showToast(info);
  else hideToast();
}

async function refreshStatus() {
  status = await invoke<StatusPayload>("get_status");
  applyTheme(status.theme);
  renderStatusBar();
  return status;
}

async function startDsh() {
  if (busy) return;
  busy = true;
  try {
    showOverlay({
      title: "正在启动 Deepseek Harness…",
      desc: "首次启动可能需要下载依赖,请稍候",
      spinner: true,
    });
    await invoke("start_dsh");
    await refreshStatus();
  } catch (e) {
    showOverlay({ title: "启动失败", desc: String(e), retry: true });
  } finally {
    busy = false;
  }
}

async function ensureRunning() {
  const s = await refreshStatus();
  if (!s.toolchain.nodeFound || !s.toolchain.npmFound) {
    showOverlay({
      title: "需要安装 Node.js",
      desc: "未检测到 Node.js / npm。dsh 通过 npm 安装,请先安装 Node.js(包含 npm),然后重新打开 DeepSeek Harness。",
      openNodejs: true,
    });
    return;
  }
  if (!s.toolchain.dshFound) {
    showOverlay({
      title: "未检测到 dsh",
      desc: "请先安装 dsh:npm install -g @deepseek-ai/dsh",
      installDsh: true,
    });
    return;
  }
  if (s.webStatus === "running") {
    mountFrame();
    return;
  }
  if (s.webStatus === "starting") {
    showOverlay({ title: "DeepSeek Harness 启动中…", spinner: true });
    return;
  }
  await startDsh();
}

async function runUpgrade(kind: "install" | "upgrade") {
  if (busy) return;
  busy = true;
  try {
    clearLogs();
    showOverlay({
      title: kind === "install" ? "正在安装 dsh…" : "正在升级 dsh…",
      desc: "npm install -g @deepseek-ai/dsh",
      spinner: true,
    });
    const info = await invoke<UpdateInfo>("upgrade_dsh");
    handleUpdate(info);
    hideToast();
    const s = await refreshStatus();
    if (s.webStatus === "running" && s.managed) {
      showOverlay({ title: "升级完成,正在重启 DeepSeek Harness…", spinner: true });
      unmountFrame();
      await invoke("restart_dsh");
      await refreshStatus();
    } else if (s.webStatus === "running") {
      showOverlay({
        title: "升级完成",
        desc: "当前 DeepSeek Harness 服务由外部启动,重启该服务后新版本生效。",
        retry: true,
      });
    } else {
      await ensureRunning();
    }
  } catch (e) {
    showOverlay({ title: "升级失败", desc: String(e), retry: true });
  } finally {
    busy = false;
  }
}

async function restartDsh() {
  if (!status?.managed) {
    const holder = status?.externalProcess
      ? `端口由 ${status.externalProcess} 提供`
      : "端口由外部进程提供";
    showOverlay({
      title: "无法从此处重启",
      desc: `当前 DeepSeek Harness 不是桌面端可管理的本机 node 进程(${holder},可能运行在 WSL 中)。请在原环境中重启该服务,应用每 5 秒自动检测,恢复后会自动重新挂载。`,
    });
    return;
  }
  if (busy) return;
  busy = true;
  try {
    clearLogs();
    showOverlay({ title: "正在重启 DeepSeek Harness…", spinner: true });
    unmountFrame();
    await invoke("restart_dsh");
    await refreshStatus();
  } catch (e) {
    showOverlay({ title: "重启失败", desc: String(e), retry: true });
  } finally {
    busy = false;
  }
}

async function stopDsh() {
  if (!status?.managed) {
    const holder = status?.externalProcess
      ? `端口由 ${status.externalProcess} 提供`
      : "端口由外部进程提供";
    showOverlay({
      title: "无法从此处停止",
      desc: `当前 DeepSeek Harness 不是桌面端可管理的本机 node 进程(${holder},可能运行在 WSL 中)。请在原环境(如 WSL)中停止该服务;应用会自动检测,服务停止后状态会更新。`,
    });
    return;
  }
  if (busy) return;
  busy = true;
  try {
    showOverlay({ title: "正在停止 DeepSeek Harness…", spinner: true });
    await invoke("stop_dsh");
    await refreshStatus();
  } catch (e) {
    showOverlay({ title: "停止失败", desc: String(e), retry: true });
  } finally {
    busy = false;
  }
}

async function onWebStatusChanged(st: WebStatus) {
  if (status) status.webStatus = st;
  const wasExternal = externalLastSeen;
  renderStatusBar();
  switch (st) {
    case "running":
      mountFrame();
      break;
    case "starting":
      if (overlay.classList.contains("hidden")) {
        showOverlay({ title: "DeepSeek Harness 启动中…", spinner: true });
      }
      break;
    case "failed": {
      const s = await refreshStatus();
      showOverlay({
        title: "DeepSeek Harness 启动失败",
        desc: s.failedReason ?? "请查看下方日志",
        retry: true,
      });
      break;
    }
    case "stopped":
      unmountFrame();
      showOverlay({
        title: "DeepSeek Harness 已停止",
        desc: wasExternal
          ? "该服务原由外部启动(如 WSL)。请在原环境中重新启动,应用会自动挂载;或点击重试,由桌面端在本机启动 DeepSeek Harness。"
          : "点击重试重新启动 DeepSeek Harness",
        retry: true,
      });
      break;
    case "notInstalled":
      break;
  }
}

function checkNow() {
  btnCheck.disabled = true;
  btnCheck.textContent = "检查中…";
  invoke<UpdateInfo>("check_update", { force: true })
    .then(handleUpdate)
    .catch((e) => {
      if (status) status.update.lastError = String(e);
      versionText.textContent = `检查失败:${String(e)}`;
    })
    .finally(() => {
      btnCheck.disabled = false;
      btnCheck.textContent = "检查更新";
      renderStatusBar();
    });
}

async function boot() {
  await listen<WebStatusEvent>("web-status-changed", (e) => onWebStatusChanged(e.payload.status));
  await listen<ThemeEvent>("theme-changed", (e) => applyTheme(e.payload.preference));
  await listen<ProcLogEvent>("proc-log", (e) => appendLog(e.payload.source, e.payload.line));
  await listen<UpdateInfo>("update-status", (e) => handleUpdate(e.payload));
  await listen<UpgradeDoneEvent>("upgrade-done", () => undefined);

  window
    .matchMedia("(prefers-color-scheme: dark)")
    .addEventListener("change", () => applyTheme(themePref));

  // Handle window-control messages from the injected iframe bar
  window.addEventListener("message", (e) => {
    if (e.data?.source !== "dsh-dt") return;
    switch (e.data.action) {
      case "drag":
        void appWindow.startDragging();
        break;
      case "minimize":
        void appWindow.minimize();
        break;
      case "toggleMaximize":
        void (async () => {
          try {
            if (await appWindow.isMaximized()) await appWindow.unmaximize();
            else await appWindow.maximize();
          } catch (err) {
            console.error("toggle maximize failed:", err);
          }
        })();
        break;
      case "close":
        void appWindow.close();
        break;
    }
  });

  btnRetry.addEventListener("click", () => void ensureRunning());
  btnInstallDsh.addEventListener("click", () => void runUpgrade("install"));
  btnOpenNodejs.addEventListener("click", () => void invoke("open_nodejs").catch(() => undefined));
  btnCheck.addEventListener("click", checkNow);
  btnUpgrade.addEventListener("click", () => void runUpgrade("upgrade"));
  btnRestart.addEventListener("click", () => void restartDsh());
  btnStop.addEventListener("click", () => void stopDsh());
  btnBrowser.addEventListener("click", () => void invoke("open_in_browser").catch(() => undefined));
  toastUpgrade.addEventListener("click", () => void runUpgrade("upgrade"));
  toastLater.addEventListener("click", () => {
    const latest = status?.update.latestVersion;
    if (!latest) return;
    void invoke("dismiss_update", { version: latest })
      .then(hideToast)
      .catch(() => undefined);
  });
  frame.addEventListener("load", () => {
    if (frameMounted && status?.webStatus === "running") hideOverlay();
  });

  try {
    await refreshStatus();
    await ensureRunning();
    void invoke<UpdateInfo>("check_update", { force: false })
      .then(handleUpdate)
      .catch(() => undefined);
  } catch (e) {
    showOverlay({ title: "初始化失败", desc: String(e), retry: true });
  }
}

window.addEventListener("DOMContentLoaded", () => void boot());
