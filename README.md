# DSH-Desktop

DSH 的 Windows 桌面启动器(Tauri 2 + Vanilla TS)。

## 功能

- **一键启动**:打开应用即自动执行 `dsh web`,服务就绪后在窗口内嵌显示 DSH Web(`http://127.0.0.1:3080`);若服务已在运行(包括外部启动的),直接挂载,绝不重复启动。
- **无边框窗口**:自绘标题栏与内容同底色融为一体,可拖拽、最小化、关闭;主题实时跟随 `%USERPROFILE%\.dsh\settings.yaml` 的 `ui-theme.preference`(`dark` / `light` / `system`,system 跟随系统深浅色)。
- **版本检查与升级**:每天第一次启动检查新版本,之后每 6 小时后台再查;发现新版本时左上角弹出「升级dsh新版本」提示,可一键执行 `npm install -g @deepseek-ai/dsh` 并自动重启服务。
- **环境提示**:未检测到 Node.js / npm 时提示安装并提供 nodejs.org 入口;未安装 dsh 时提供一键安装按钮。
- **干净退出**:关闭窗口即退出应用,并停止由本应用启动的 dsh web(外部启动的服务不受影响);单实例运行,重复启动只聚焦已有窗口。

## 开发

```bash
npm install        # 前端依赖
npm run tauri dev  # 开发模式(需要 Rust 工具链)
npm run tauri build  # 打包 NSIS 安装程序
```

## 说明

- dsh 通过 `npm install -g @deepseek-ai/dsh` 安装,启动器用 `where dsh` / `where npm` 解析可执行文件路径。
- 端口默认 3080,可用环境变量 `DSH_DESKTOP_PORT` 覆盖。
- DSH 配置目录默认 `%USERPROFILE%\.dsh`(`%DSH_HOME%` 可覆盖);主题读取 `settings.yaml`(兼容 `setting.yaml`)。
- 版本检查使用 npm registry(`registry.npmjs.org/@deepseek-ai/dsh/latest`),当日检查状态与"稍后"记录持久化在应用数据目录 `state.json`。
