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
    // Push only the center column down 32px, not the sidebar
    '[class*="centerCol"]{padding-top:32px!important;box-sizing:border-box!important;}',
    "#dsh-dt-bar{",
    "  position:fixed;top:0;left:0;right:0;z-index:99999;",
    "  display:flex;align-items:center;justify-content:flex-end;height:32px;",
    "  -webkit-user-select:none;user-select:none;",
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

  // ── Build the bar (buttons only; entire bar area is draggable) ──
  var bar = document.createElement("div");
  bar.id = "dsh-dt-bar";
  bar.innerHTML =
    '<button class="dsh-dt-btn dsh-dt-min" title="最小化">\u2212</button>' +
    '<button class="dsh-dt-btn dsh-dt-max" title="最大化">\u25A1</button>' +
    '<button class="dsh-dt-btn dsh-dt-close" title="关闭">\u2715</button>';

  // Button click handlers
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

  // Drag + double-click on the entire bar (except buttons)
  // e.detail: 1 = single click → drag, 2 = double click → toggle maximize
  bar.addEventListener("mousedown", function (e) {
    if (e.button !== 0) return;
    if (e.target.closest(".dsh-dt-btn")) return; // clicked on a button, skip
    e.preventDefault();
    if (e.detail === 2) {
      send("toggleMaximize");
    } else {
      send("drag");
    }
  });

  // ── Position bar: left = sidebar width (tracked live) ──
  function updateBarLeft() {
    var sb = document.querySelector('[class*="sidebarCol"]');
    if (sb) bar.style.left = sb.getBoundingClientRect().width + "px";
  }

  function observeSidebar() {
    var sb = document.querySelector('[class*="sidebarCol"]');
    if (!sb) return false;
    updateBarLeft();
    new ResizeObserver(updateBarLeft).observe(sb);
    return true;
  }

  // ── Inject ─────────────────────────────────────────────
  function mount() {
    (document.head || document.documentElement).appendChild(style);
    if (!document.getElementById("dsh-dt-bar")) {
      document.body.appendChild(bar);
    }
    if (!observeSidebar()) {
      var domObs = new MutationObserver(function () {
        if (observeSidebar()) domObs.disconnect();
      });
      domObs.observe(document.body || document.documentElement, {
        childList: true,
        subtree: true,
      });
    }
    window.addEventListener("resize", updateBarLeft);
  }

  if (document.body) {
    mount();
  } else {
    document.addEventListener("DOMContentLoaded", mount);
  }
})();
