// 一键打包脚本:执行 tauri build,并把 NSIS 安装包复制到项目根的 release/ 目录。
import { spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));
const { productName, version } = conf;
const arch = process.env.npm_config_arch ?? "x64";
const installer = join(
  root,
  "src-tauri",
  "target",
  "release",
  "bundle",
  "nsis",
  `${productName}_${version}_${arch}-setup.exe`
);

function run(cmd, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd: root,
      stdio: "inherit",
      shell: process.platform === "win32",
    });
    child.on("error", reject);
    child.on("exit", (code) =>
      code === 0 ? resolve() : reject(new Error(`${cmd} 退出码 ${code}`))
    );
  });
}

async function main() {
  console.log(`[package] 构建 ${productName} v${version} (${arch}) …`);
  await run("npm", ["run", "tauri", "build"]);

  if (!existsSync(installer)) {
    console.error(`[package] 未找到安装包: ${installer}`);
    process.exit(1);
  }

  const outDir = join(root, "release");
  mkdirSync(outDir, { recursive: true });
  const out = join(outDir, `${productName}_${version}_${arch}-setup.exe`);
  copyFileSync(installer, out);
  console.log(`[package] 完成: ${out}`);
}

main().catch((error) => {
  console.error("[package] 失败:", error.message);
  process.exit(1);
});
