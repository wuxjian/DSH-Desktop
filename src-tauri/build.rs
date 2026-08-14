fn main() {
    // 图标目录变化时强制重跑构建脚本,否则 Windows exe 资源会一直
    // 嵌入旧图标(见 https://github.com/tauri-apps/tauri 相关 issue)。
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
