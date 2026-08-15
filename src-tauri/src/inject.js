(function () {
  "use strict";
  if (window.self === window.top) return; // only run inside the dsh web iframe

  // ── Communicate with the parent (Tauri webview) via postMessage ──
  function send(action) {
    window.parent.postMessage({ source: "dsh-dt", action: action }, "*");
  }

  // ── CSS ─────────────────────────────────────────────────
  var style = document.createElement("style");
  style.textContent = [
    "#dsh-dt-bar{",
    "  position:fixed;top:0;left:0;right:0;z-index:99999;",
    "  display:flex;align-items:center;height:32px;",
    "  -webkit-user-select:none;user-select:none;",
    "}",
    ".dsh-dt-drag{",
    "  flex:1;height:100%;cursor:default;",
    "}",
    ".dsh-dt-btn{",
    "  width:40px;height:32px;border:none;border-radius:0;",
    "  background:transparent;cursor:pointer;",
    "  display:flex;align-items:center;justify-content:center;",
    "  color:var(--dsw-alias-label-secondary,#9aa1ad);",
    "  font-size:13px;line-height:1;",
    "  transition:background .12s ease,color .12s ease;",
    "}",
    ".dsh-dt-btn:hover{",
    "  background:var(--dsw-alias-interactive-bg-hover,rgba(128,128,128,.14));",
    "  color:var(--dsw-alias-label-primary,#e8eaee);",
    "}",
    ".dsh-dt-close:hover{background:#e81123!important;color:#fff!important;}",
  ].join("\n");

  // ── Build the bar ──────────────────────────────────────
  var bar = document.createElement("div");
  bar.id = "dsh-dt-bar";
  bar.innerHTML =
    '<div class="dsh-dt-drag"></div>' +
    '<button class="dsh-dt-btn dsh-dt-min" title="最小化">\u2212</button>' +
    '<button class="dsh-dt-btn dsh-dt-max" title="最大化">\u25A1</button>' +
    '<button class="dsh-dt-btn dsh-dt-close" title="关闭">\u2715</button>';

  // Button handlers
  bar.querySelector(".dsh-dt-min").addEventListener("click", function (e) {
    e.stopPropagation();
    send("minimize");
  });
  bar.querySelector(".dsh-dt-max").addEventListener("click", function (e) {
    e.stopPropagation();
    send("toggleMaximize");
  });
  bar.querySelector(".dsh-dt-close").addEventListener("click", function (e) {
    e.stopPropagation();
    send("close");
  });

  // Drag: only on the drag handle area
  var drag = bar.querySelector(".dsh-dt-drag");
  drag.addEventListener("mousedown", function (e) {
    if (e.button !== 0) return;
    e.preventDefault();
    send("drag");
  });
  drag.addEventListener("dblclick", function () {
    send("toggleMaximize");
  });

  // ── Inject ─────────────────────────────────────────────
  function mount() {
    (document.head || document.documentElement).appendChild(style);
    if (!document.getElementById("dsh-dt-bar")) {
      document.body.appendChild(bar);
    }
  }

  if (document.body) {
    mount();
  } else {
    document.addEventListener("DOMContentLoaded", mount);
  }
})();
