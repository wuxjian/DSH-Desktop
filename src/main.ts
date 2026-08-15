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
  owned: boolean;
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
const overlayLogo = $(".overlay-logo");
const logPre = $("#log-pre");
const frame = $("#app-frame") as HTMLIFrameElement;
const statusDot = $("#status-dot");
const statusText = $("#status-text");
const versionText = $("#version-text");
const btnCheck = $("#btn-check") as HTMLButtonElement;
const btnUpgrade = $("#btn-upgrade");
const btnRestart = $("#btn-restart");
const btnBrowser = $("#btn-browser") as HTMLButtonElement;
const btnRetry = $("#btn-retry");
const btnInstallDsh = $("#btn-install-dsh");
const btnOpenNodejs = $("#btn-open-nodejs");
const btnMin = $("#btn-min") as HTMLButtonElement;
const btnMax = $("#btn-max") as HTMLButtonElement;
const btnClose = $("#btn-close") as HTMLButtonElement;
const appWindow = getCurrentWindow();
const toast = $("#toast");
const toastVersion = $("#toast-version");
const toastCurrent = $("#toast-current");
const toastUpgrade = $("#toast-upgrade");
const toastLater = $("#toast-later");

let status: StatusPayload | null = null;
let themePref: "dark" | "light" | "system" = "system";
let frameMounted = false;
let busy = false;
const logLines: string[] = [];

const STATUS_LABEL: Record<WebStatus, string> = {
  notInstalled: "未安装 dsh",
  stopped: "dsh web 未启动",
  starting: "dsh web 启动中…",
  running: "dsh web 运行中",
  failed: "dsh web 异常",
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
  overlayLogo.classList.toggle("hidden", !!opts.spinner);
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
  btnRestart.classList.toggle("hidden", !(st === "running" && status.owned));
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
      title: "正在启动 dsh web…",
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
    showOverlay({ title: "dsh web 启动中…", spinner: true });
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
    if (s.webStatus === "running" && s.owned) {
      showOverlay({ title: "升级完成,正在重启 dsh web…", spinner: true });
      unmountFrame();
      await invoke("restart_dsh");
      await refreshStatus();
    } else if (s.webStatus === "running") {
      showOverlay({
        title: "升级完成",
        desc: "当前 dsh web 由外部启动,重启该服务后新版本生效。",
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
  if (busy) return;
  busy = true;
  try {
    clearLogs();
    showOverlay({ title: "正在重启 dsh web…", spinner: true });
    unmountFrame();
    await invoke("restart_dsh");
    await refreshStatus();
  } catch (e) {
    showOverlay({ title: "重启失败", desc: String(e), retry: true });
  } finally {
    busy = false;
  }
}

async function onWebStatusChanged(st: WebStatus) {
  if (status) status.webStatus = st;
  renderStatusBar();
  switch (st) {
    case "running":
      mountFrame();
      break;
    case "starting":
      if (overlay.classList.contains("hidden")) {
        showOverlay({ title: "dsh web 启动中…", spinner: true });
      }
      break;
    case "failed": {
      const s = await refreshStatus();
      showOverlay({
        title: "dsh web 启动失败",
        desc: s.failedReason ?? "请查看下方日志",
        retry: true,
      });
      break;
    }
    case "stopped":
      unmountFrame();
      showOverlay({
        title: "dsh web 已停止",
        desc: "点击重试重新启动 dsh web",
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

/** Keep the maximize/restore glyph in sync with the window state. */
async function updateMaxButton() {
  try {
    const maximized = await appWindow.isMaximized();
    btnMax.innerHTML = maximized ? "&#x2750;" : "&#x25A1;";
    btnMax.title = maximized ? "还原" : "最大化";
  } catch (e) {
    console.error("isMaximized failed:", e);
  }
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

  btnMin.addEventListener("click", () => void appWindow.minimize());
  btnMax.addEventListener("click", () => {
    void (async () => {
      try {
        if (await appWindow.isMaximized()) await appWindow.unmaximize();
        else await appWindow.maximize();
      } catch (e) {
        console.error("toggle maximize failed:", e);
      }
      await updateMaxButton();
    })();
  });
  btnClose.addEventListener("click", () => void appWindow.close());
  void appWindow.onResized(() => void updateMaxButton());
  void updateMaxButton();
  btnRetry.addEventListener("click", () => void ensureRunning());
  btnInstallDsh.addEventListener("click", () => void runUpgrade("install"));
  btnOpenNodejs.addEventListener("click", () => void invoke("open_nodejs").catch(() => undefined));
  btnCheck.addEventListener("click", checkNow);
  btnUpgrade.addEventListener("click", () => void runUpgrade("upgrade"));
  btnRestart.addEventListener("click", () => void restartDsh());
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
