use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use crate::dsh::CREATE_NO_WINDOW;

/// What the launcher knows about the local Node.js / npm / dsh toolchain.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainStatus {
    pub node_found: bool,
    pub npm_found: bool,
    pub npm_cmd: Option<String>,
    pub dsh_found: bool,
    pub dsh_cmd: Option<String>,
    pub dsh_version: Option<String>,
}

/// Run `where <name>` and collect matching path lines.
fn where_lines(name: &str) -> Vec<String> {
    let out = match Command::new("where").arg(name).creation_flags(CREATE_NO_WINDOW).output() {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `where` on Windows lists both the extension-less shim and the `.cmd`
/// wrapper; prefer the `.cmd` one because it lives in the global npm bin
/// directory, next to `dsh.cmd`.
fn prefer_cmd(lines: Vec<String>) -> Option<String> {
    lines
        .iter()
        .find(|l| l.to_ascii_lowercase().ends_with(".cmd"))
        .cloned()
        .or_else(|| lines.first().cloned())
}

/// Read the installed dsh version from
/// `<global-bin>\node_modules\@deepseek-ai\dsh\package.json`.
fn installed_version_of(dsh_cmd: &Path) -> Option<String> {
    let pkg = dsh_cmd
        .parent()?
        .join("node_modules")
        .join("@deepseek-ai")
        .join("dsh")
        .join("package.json");
    let text = std::fs::read_to_string(pkg).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("version")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Detect node/npm/dsh on PATH, resolving concrete executable paths.
pub fn detect() -> ToolchainStatus {
    let node_found = !where_lines("node").is_empty();
    let npm_cmd = prefer_cmd(where_lines("npm"));
    let mut dsh_cmd = prefer_cmd(where_lines("dsh"));

    // Fallback: npm's global bin directory should contain dsh.cmd when the
    // package was installed with `npm install -g @deepseek-ai/dsh`.
    if dsh_cmd.is_none() {
        if let Some(npm) = &npm_cmd {
            if let Some(dir) = Path::new(npm).parent() {
                let candidate = dir.join("dsh.cmd");
                if candidate.exists() {
                    dsh_cmd = Some(candidate.to_string_lossy().into_owned());
                }
            }
        }
    }

    let dsh_version = dsh_cmd.as_deref().map(Path::new).and_then(installed_version_of);

    ToolchainStatus {
        node_found,
        npm_found: npm_cmd.is_some(),
        npm_cmd,
        dsh_found: dsh_cmd.is_some(),
        dsh_cmd,
        dsh_version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_cmd_chooses_the_cmd_shim() {
        let lines = vec![
            r"C:\Program Files\nodejs\npm".to_string(),
            r"C:\Users\me\AppData\Roaming\npm\npm.cmd".to_string(),
        ];
        assert_eq!(
            prefer_cmd(lines).as_deref(),
            Some(r"C:\Users\me\AppData\Roaming\npm\npm.cmd")
        );
    }

    #[test]
    fn prefer_cmd_falls_back_to_first_line() {
        let lines = vec![r"C:\tools\npm.exe".to_string()];
        assert_eq!(prefer_cmd(lines).as_deref(), Some(r"C:\tools\npm.exe"));
    }

    #[test]
    fn prefer_cmd_empty_input() {
        assert_eq!(prefer_cmd(Vec::new()), None);
    }
}
