(function () {
  "use strict";
  // Run ONLY in the dsh web frame (the direct child of the Tauri shell page):
  // skip the shell itself (self === top) and any deeper iframes the dsh web
  // app embeds (plugin browser tabs, HTML previews), so the title bar and the
  // layout overrides never leak into embedded third-party pages.
  if (window.self === window.top || window.parent !== window.top) return;

  // ── Communicate with the parent (Tauri webview) via postMessage ──
  function send(action) {
    window.parent.postMessage({ source: "dsh-dt", action: action }, "*");
  }

  // ── CSS ─────────────────────────────────────────────────
  var style = document.createElement("style");
  style.textContent = [
    // Title-bar strip: push BOTH the center column and the right details
    // column (plugin right sidebars) down 32px, so nothing overlaps the bar.
    // The left sidebar keeps its own top area (the bar starts right after it).
    '[class*="centerCol"]{padding-top:32px!important;box-sizing:border-box!important;}',
    '[class*="detailsCol"]{padding-top:32px!important;box-sizing:border-box!important;}',
    // Keep the right-column drag handle reachable below the bar.
    '[data-side="details"][class*="handle"]{top:32px!important;}',
    // dsh-better-sidebar plugin: its fixed right panel, its top-right toggle
    // cluster and its error rail all yield the top 32px to the title bar.
    // (The plugin mounts everything under div[data-dsh-better-sidebar].)
    'div[data-dsh-better-sidebar] [class*="toggleCluster"]{top:35px!important;}',
    'div[data-dsh-better-sidebar] [class*="panel"]:not([class*="panelHidden"]):not([class*="panelResize"]){top:32px!important;}',
    'div[data-dsh-better-sidebar] [class*="boundaryError"]{top:32px!important;}',
    "#dsh-dt-bar{",
    "  position:fixed;top:0;left:0;right:0;z-index:2147483647;",
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
  // right stays 0, so the bar's right end is always pinned to the top-right
  // corner of the viewport, on top of any plugin right sidebar.
  var trackedSb = null;

  function observeSidebar() {
    var sb = document.querySelector('[class*="sidebarCol"]');
    if (!sb) return false;
    trackedSb = sb;
    updateBarLeft();
    new ResizeObserver(updateBarLeft).observe(sb);
    return true;
  }

  function updateBarLeft() {
    // If React replaced the sidebar node (SPA re-mount), re-observe.
    if (trackedSb && !trackedSb.isConnected) {
      trackedSb = null;
      observeSidebar();
      return;
    }
    var sb = trackedSb || document.querySelector('[class*="sidebarCol"]');
    if (sb) {
      trackedSb = sb;
      bar.style.left = sb.getBoundingClientRect().width + "px";
    }
  }

  // ── Generic auto-yield ──────────────────────────────────
  // Future plugins that draw their own fixed right panels are pushed below
  // the 32px title bar automatically (panel heuristic: right edge flush with
  // the viewport, starts at the very top, tall — never tooltips/dropdowns).
  // Panels already shifted (top >= 32) or full-width overlays are skipped.
  var yieldPending = false;

  function yieldTopDockedPanels() {
    var vw = window.innerWidth;
    var vh = window.innerHeight;
    var candidates = document.querySelectorAll("body > *, body > * > *");
    for (var i = 0; i < candidates.length; i++) {
      var el = candidates[i];
      if (el.id === "dsh-dt-bar") continue;
      if (el.dataset && el.dataset.dshDtYielded) continue;
      if (getComputedStyle(el).position !== "fixed") continue;
      var r = el.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) continue;
      if (r.top <= 2 && r.right >= vw - 1 && r.height >= vh * 0.5 && r.width < vw * 0.8) {
        el.style.top = "32px";
        // Panels sized by height:100vh (no bottom anchor) keep fitting.
        if (getComputedStyle(el).bottom === "auto" && r.height >= vh - 4) {
          el.style.height = "calc(100vh - 32px)";
        }
        el.dataset.dshDtYielded = "1";
      }
    }
  }

  function scheduleYieldScan() {
    if (yieldPending) return;
    yieldPending = true;
    requestAnimationFrame(function () {
      yieldPending = false;
      yieldTopDockedPanels();
    });
  }

  // ── Inject ─────────────────────────────────────────────
  function mount() {
    (document.head || document.documentElement).appendChild(style);
    if (!document.getElementById("dsh-dt-bar")) {
      document.body.appendChild(bar);
    }
    observeSidebar();
    yieldTopDockedPanels();
    // Persistent observer: re-observe if the sidebar node is (re)created
    // later, and auto-yield newly inserted fixed panels.
    new MutationObserver(function (mutations) {
      for (var i = 0; i < mutations.length; i++) {
        if (mutations[i].addedNodes && mutations[i].addedNodes.length) {
          scheduleYieldScan();
          break;
        }
      }
      if (trackedSb && trackedSb.isConnected) return;
      observeSidebar();
    }).observe(document.body || document.documentElement, {
      childList: true,
      subtree: true,
    });
    window.addEventListener("resize", updateBarLeft);
  }

  if (document.body) {
    mount();
  } else {
    document.addEventListener("DOMContentLoaded", mount);
  }
})();
