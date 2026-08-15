# DSH-Desktop

DSH 的 Windows 桌面启动器(Tauri 2 + Vanilla TS)。打开应用即自动启动 `dsh web` 并把 DSH Web 界面内嵌在无边框窗口里,同时负责 dsh 的版本检查与一键升级。

## 功能

- **一键启动**:启动时自动执行 `dsh web`,服务就绪后在窗口内嵌显示 DSH Web(`http://127.0.0.1:3080`);若服务已在运行(包括外部启动的),直接挂载,绝不重复启动。
- **无边框窗口**:自绘标题栏与内容同底色、无分隔线,融为一体;支持拖拽、最小化、最大化/还原、关闭;应用图标为 SVG 鲸鱼(深色主题自动反白)。
- **主题联动**:实时监听 `%USERPROFILE%\.dsh\settings.yaml` 的 `ui-theme.preference`(`dark` / `light` / `system`,system 跟随系统深浅色),标题栏与内嵌页面同步变色。
- **版本检查与升级**:每天第一次启动检查新版本,之后每 6 小时后台再查;发现新版本时左上角弹出「升级dsh新版本 vX.Y.Z」提示,可一键执行 `npm install -g @deepseek-ai/dsh` 并自动重启服务;「稍后」当日不再提示。
- **环境引导**:未检测到 Node.js / npm 时提示安装并提供 nodejs.org 入口;未安装 dsh 时提供一键安装按钮。
- **干净退出**:关闭窗口即退出应用,并停止由本应用启动的 dsh web(外部启动的服务不受影响);单实例运行,重复启动只聚焦已有窗口。
<img src="[图片链接](https://private-user-images.githubusercontent.com/18617209/636460037-097e8240-f944-4490-84cf-d8470e069b50.png?jwt=eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJpc3MiOiJnaXRodWIuY29tIiwiYXVkIjoicmF3LmdpdGh1YnVzZXJjb250ZW50LmNvbSIsImtleSI6ImtleTUiLCJleHAiOjE3ODY3NjMyNjYsIm5iZiI6MTc4Njc2Mjk2NiwicGF0aCI6Ii8xODYxNzIwOS82MzY0NjAwMzctMDk3ZTgyNDAtZjk0NC00NDkwLTg0Y2YtZDg0NzBlMDY5YjUwLnBuZz9YLUFtei1BbGdvcml0aG09QVdTNC1ITUFDLVNIQTI1NiZYLUFtei1DcmVkZW50aWFsPUFLSUFWQ09EWUxTQTUzUFFLNFpBJTJGMjAyNjA4MTUlMkZ1cy1lYXN0LTElMkZzMyUyRmF3czRfcmVxdWVzdCZYLUFtei1EYXRlPTIwMjYwODE1VDAzMDI0NlomWC1BbXotRXhwaXJlcz0zMDAmWC1BbXotU2lnbmF0dXJlPWQ2Nzg5ZTJhMWNkYjFmYjZmM2RkMzkwYmQ3MmUyYzJiZjUxMzgwNjljY2IwMThhZTE1MWYyYjNkYjg2OWY2ODYmWC1BbXotU2lnbmVkSGVhZGVycz1ob3N0JnJlc3BvbnNlLWNvbnRlbnQtdHlwZT1pbWFnZSUyRnBuZyJ9.qDrC75B41g-immu8fQx-jDqhvcDs_Gq6RlUYxENaqdc)" width="300" height="200">
## 环境要求

- Windows 10/11(自带 WebView2 Runtime)
- Node.js ≥ 18(含 npm;用于前端构建与 dsh 运行)
- Rust stable(用于编译 Tauri)
- dsh:`npm install -g @deepseek-ai/dsh`

## 快速开始

```bash
npm install        # 安装前端依赖
npm run tauri dev  # 开发模式(热更新,需要 Rust 工具链)
```

## 打包

```bash
npm run package    # 一键打包:构建 + 复制安装包到 ./release/
```

等价的手动步骤:

```bash
npm run tauri build
# 产物: src-tauri/target/release/bundle/nsis/DSH-Desktop_<版本>_x64-setup.exe
```

## 脚本一览

| 脚本 | 作用 |
|------|------|
| `npm run tauri dev` | 开发模式(启动 Vite + 编译运行桌面应用) |
| `npm run package` | 一键打包(`scripts/package.mjs`:构建并收集安装包到 `release/`) |
| `npm run tauri build` | 构建 NSIS 安装包与 exe |
| `npm run icon` | 从 `src-tauri/icons/source.svg` 重新生成全套图标(.ico/.png) |
| `npm run build` | 仅构建前端(`tsc` + `vite build`) |
| `cargo test`(在 `src-tauri/`) | Rust 单元测试(主题解析、版本比较、路径解析) |

## 目录结构

```
├─ src/                    # 前端(原生 TS,无框架)
│  ├─ main.ts              # 状态机、事件监听、升级/重启流程
│  ├─ styles.css           # 主题变量与界面样式
│  └─ assets/logo.svg      # 应用内图标(与根目录 icon.svg 同源)
├─ src-tauri/
│  ├─ src/
│  │  ├─ dsh.rs            # dsh web 进程管理、健康探测、状态机
│  │  ├─ theme.rs          # settings.yaml 主题解析与文件监听
│  │  ├─ update.rs         # 版本检查、每日闸门、npm 升级
│  │  ├─ env.rs            # node/npm/dsh 工具链探测
│  │  ├─ commands.rs       # Tauri 命令层
│  │  └─ state.rs          # 共享状态
│  ├─ tauri.conf.json      # 窗口/无边框/CSP/打包配置
│  ├─ capabilities/        # 权限声明(窗口操作、opener)
│  └─ icons/               # 图标集(含 source.svg 源图)
├─ scripts/package.mjs     # 一键打包脚本
└─ icon.svg                # 应用图标源文件(替换后运行 npm run icon)
```

## 配置说明

| 环境变量 | 说明 |
|----------|------|
| `DSH_DESKTOP_PORT` | 覆盖 dsh web 端口(默认 3080,会以 `dsh web --port <N>` 传递) |
| `DSH_HOME` | 覆盖 DSH 配置目录(默认 `%USERPROFILE%\.dsh`) |

其他行为说明:

- 主题读取 `settings.yaml` 的 `ui-theme.preference`;该文件不存在或字段非法时默认 `system`(跟随系统)。兼容文件名 `setting.yaml`。
- 版本检查使用 npm registry(`registry.npmjs.org/@deepseek-ai/dsh/latest`);当日检查状态与「稍后」记录持久化在 `%APPDATA%\com.dsh.desktop\state.json`。
- 端口被非 dsh 服务占用时,状态条会报「端口被其他程序占用」并展示进程日志。

## 常见问题

- **最小化/关闭/拖拽无效**:Tauri 2 的窗口动作权限需在 `src-tauri/capabilities/default.json` 显式声明(已配置完整)。
- **关窗后 dsh web 仍在运行**:只有本应用启动的 dsh web 才会随退出而停止;先于应用启动的服务(如命令行手动 `dsh web`)不会被误杀。
- **修改图标**:替换根目录 `icon.svg` → `npm run icon` → 重新打包;应用内标题栏图标同步替换 `src/assets/logo.svg`。
- **打包后任务栏/桌面仍显示旧图标**:图标文件变更后必须重跑构建脚本(已通过 `build.rs` 的 `rerun-if-changed` 保证);Windows 自身的图标缓存也会造成残留,重装后运行 `ie4uinit.exe -show` 或重启资源管理器;若应用被固定到任务栏,取消固定后再固定一次。
