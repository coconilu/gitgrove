// 外部工具：探测已安装应用、进程状态和持久化默认打开方式。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::{git, store};

#[cfg(windows)]
mod windows_console;

#[cfg(windows)]
fn console_arguments(id: &str) -> Option<&'static str> {
    match id {
        "powershell-core" | "powershell" => Some("-NoExit"),
        "cmd" => Some("/K"),
        _ => None,
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchPreferences {
    pub editor: String,
    pub terminal: String,
}

impl Default for LaunchPreferences {
    fn default() -> Self {
        Self {
            editor: "auto".into(),
            terminal: "auto".into(),
        }
    }
}

struct AppSpec {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    executable: &'static str,
    processes: &'static [&'static str],
    locations: Vec<PathBuf>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchApp {
    id: String,
    name: String,
    kind: String,
    path: Option<String>,
    // None 表示进程查询失败，不能把失败当作未运行。
    running: Option<bool>,
}

fn spec(
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    executable: &'static str,
    processes: &'static [&'static str],
) -> AppSpec {
    AppSpec {
        id,
        name,
        kind,
        executable,
        processes,
        locations: Vec::new(),
    }
}

fn catalog() -> Vec<AppSpec> {
    #[cfg(windows)]
    {
        let mut apps = vec![
            spec("vscode", "VS Code", "editor", "Code.exe", &["code.exe"]),
            spec("cursor", "Cursor", "editor", "Cursor.exe", &["cursor.exe"]),
            spec("zcode", "ZCode", "editor", "ZCode.exe", &["zcode.exe"]),
            spec(
                "windows-terminal",
                "Windows Terminal",
                "terminal",
                "wt.exe",
                &["windowsterminal.exe"],
            ),
            spec(
                "powershell-core",
                "PowerShell 7",
                "terminal",
                "pwsh.exe",
                &["pwsh.exe"],
            ),
            spec(
                "powershell",
                "Windows PowerShell",
                "terminal",
                "powershell.exe",
                &["powershell.exe"],
            ),
            spec("cmd", "命令提示符", "terminal", "cmd.exe", &["cmd.exe"]),
            spec(
                "git-bash",
                "Git Bash",
                "terminal",
                "git-bash.exe",
                &["mintty.exe", "git-bash.exe"],
            ),
        ];
        let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        for app in &mut apps {
            match app.id {
                "vscode" | "cursor" | "zcode" => {
                    let folder = match app.id {
                        "vscode" => "Microsoft VS Code",
                        "cursor" => "cursor",
                        _ => "ZCode",
                    };
                    if let Some(base) = &local {
                        app.locations
                            .push(base.join("Programs").join(folder).join(app.executable));
                    }
                    for base in &roots {
                        app.locations.push(base.join(folder).join(app.executable));
                    }
                }
                "windows-terminal" => {
                    if let Some(base) = &local {
                        app.locations
                            .push(base.join("Microsoft/WindowsApps/wt.exe"));
                    }
                }
                "powershell-core" => {
                    for base in &roots {
                        app.locations.push(base.join("PowerShell/7/pwsh.exe"));
                    }
                }
                "powershell" | "cmd" => {
                    if let Some(base) = std::env::var_os("SystemRoot") {
                        app.locations
                            .push(
                                PathBuf::from(base)
                                    .join("System32")
                                    .join(if app.id == "cmd" {
                                        "cmd.exe"
                                    } else {
                                        "WindowsPowerShell/v1.0/powershell.exe"
                                    }),
                            );
                    }
                }
                "git-bash" => {
                    for base in &roots {
                        app.locations.push(base.join("Git/git-bash.exe"));
                    }
                    if let Some(base) = &local {
                        app.locations.push(base.join("Programs/Git/git-bash.exe"));
                    }
                }
                _ => {}
            }
        }
        apps
    }
    #[cfg(target_os = "macos")]
    {
        let mut apps = vec![
            spec(
                "vscode",
                "VS Code",
                "editor",
                "code",
                &["code", "visual studio code"],
            ),
            spec("cursor", "Cursor", "editor", "cursor", &["cursor"]),
            spec("zcode", "ZCode", "editor", "zcode", &["zcode"]),
            spec("terminal", "Terminal", "terminal", "", &["terminal"]),
            spec("iterm", "iTerm2", "terminal", "", &["iterm2"]),
        ];
        for (app, bundle) in apps.iter_mut().zip([
            "Visual Studio Code.app",
            "Cursor.app",
            "ZCode.app",
            "Terminal.app",
            "iTerm.app",
        ]) {
            for base in [
                PathBuf::from("/Applications"),
                git::home_dir().join("Applications"),
                PathBuf::from("/System/Applications/Utilities"),
            ] {
                app.locations.push(base.join(bundle));
            }
        }
        apps
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        vec![
            spec("vscode", "VS Code", "editor", "code", &["code"]),
            spec("cursor", "Cursor", "editor", "cursor", &["cursor"]),
            spec("zcode", "ZCode", "editor", "zcode", &["zcode"]),
            spec(
                "gnome-terminal",
                "GNOME Terminal",
                "terminal",
                "gnome-terminal",
                &["gnome-terminal", "gnome-terminal-server"],
            ),
            spec("konsole", "Konsole", "terminal", "konsole", &["konsole"]),
            spec("xterm", "XTerm", "terminal", "xterm", &["xterm"]),
        ]
    }
}

fn available_path(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|s| s == "app") {
        return path.is_dir();
    }
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path
            .metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    }
    #[cfg(not(unix))]
    true
}

fn resolve_in(app: &AppSpec, dirs: &[PathBuf]) -> Option<PathBuf> {
    // Windows 的 code/cursor 通常在 PATH 中只有 .cmd；直接定位 GUI .exe，避免 shell 回退。
    for dir in dirs.iter().filter(|p| p.is_absolute()) {
        let direct = dir.join(app.executable);
        if !app.executable.is_empty() && available_path(&direct) {
            return Some(direct);
        }
        #[cfg(windows)]
        if matches!(app.id, "vscode" | "cursor" | "zcode" | "git-bash") {
            if let Some(parent) = dir.parent() {
                let candidate = parent.join(app.executable);
                if available_path(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    app.locations.iter().find(|p| available_path(p)).cloned()
}

fn resolve(app: &AppSpec) -> Option<PathBuf> {
    let dirs: Vec<_> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    resolve_in(app, &dirs)
}

fn process_names() -> Option<Vec<String>> {
    #[cfg(windows)]
    let output = {
        let program = PathBuf::from(std::env::var_os("SystemRoot")?).join("System32/tasklist.exe");
        git::new_cmd(program.to_str()?)
            .args(["/FO", "CSV", "/NH"])
            .output()
            .ok()?
    };
    #[cfg(not(windows))]
    let output = git::new_cmd("ps")
        .args(["-A", "-o", "comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(
        text.lines()
            .filter_map(|line| {
                #[cfg(windows)]
                let name = line.strip_prefix('"')?.split('"').next()?;
                #[cfg(not(windows))]
                let name = line.trim().rsplit('/').next()?;
                Some(name.to_lowercase())
            })
            .collect(),
    )
}

fn process_name_matches(actual: &str, expected: &str, linux_comm: bool) -> bool {
    // Linux /proc comm 最多 15 字节；当前应用清单中的进程名均为 ASCII。
    // macOS / Windows 保持完整名称匹配，不能把相同前缀的其他进程当作目标。
    actual == expected || (linux_comm && expected.len() > 15 && expected.get(..15) == Some(actual))
}

fn discover() -> Vec<LaunchApp> {
    let processes = process_names();
    catalog()
        .iter()
        .map(|app| LaunchApp {
            id: app.id.into(),
            name: app.name.into(),
            kind: app.kind.into(),
            path: resolve(app).map(|p| p.to_string_lossy().into_owned()),
            running: processes.as_ref().map(|names| {
                app.processes.iter().any(|name| {
                    names
                        .iter()
                        .any(|n| process_name_matches(n, name, cfg!(target_os = "linux")))
                })
            }),
        })
        .collect()
}

#[tauri::command]
pub async fn discover_launch_apps() -> Result<Vec<LaunchApp>, String> {
    tauri::async_runtime::spawn_blocking(discover)
        .await
        .map_err(|e| e.to_string())
}

fn read_preferences(path: &Path) -> Result<LaunchPreferences, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("读取打开方式配置失败: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LaunchPreferences::default()),
        Err(e) => Err(format!("读取打开方式配置失败: {e}")),
    }
}

#[tauri::command]
pub fn get_launch_preferences() -> Result<LaunchPreferences, String> {
    read_preferences(&store::app_data_dir().join("launch-preferences.json"))
}

fn selected_app(kind: &str, id: &str) -> Result<(AppSpec, PathBuf), String> {
    for app in catalog().into_iter().filter(|a| a.kind == kind) {
        if id == "auto" || id == app.id {
            if let Some(path) = resolve(&app) {
                return Ok((app, path));
            }
            if id != "auto" {
                return Err(format!(
                    "未检测到 {}，请在“配置”中选择已安装的应用。",
                    app.name
                ));
            }
        }
    }
    Err(format!(
        "没有可用的{}，请安装应用后在“配置”中重新检测。",
        if kind == "editor" {
            "编辑器"
        } else {
            "终端"
        }
    ))
}

fn validate_preferences(prefs: &LaunchPreferences) -> Result<(), String> {
    for (kind, id) in [("editor", &prefs.editor), ("terminal", &prefs.terminal)] {
        if id != "auto" {
            selected_app(kind, id)?;
        }
    }
    Ok(())
}

static SAVE_LOCK: Mutex<()> = Mutex::new(());

fn write_preferences(path: &Path, prefs: &LaunchPreferences) -> Result<(), String> {
    let _guard = SAVE_LOCK.lock().map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    let text = serde_json::to_vec_pretty(prefs).map_err(|e| e.to_string())?;
    std::fs::write(&temp, text).map_err(|e| format!("保存打开方式失败: {e}"))?;
    std::fs::rename(&temp, path).map_err(|e| format!("保存打开方式失败: {e}"))
}

#[tauri::command]
pub fn save_launch_preferences(
    preferences: LaunchPreferences,
) -> Result<LaunchPreferences, String> {
    validate_preferences(&preferences)?;
    write_preferences(
        &store::app_data_dir().join("launch-preferences.json"),
        &preferences,
    )?;
    Ok(preferences)
}

fn launch_command(app: &AppSpec, executable: &Path, target: &Path) -> Command {
    #[cfg(target_os = "macos")]
    if executable.extension().is_some_and(|s| s == "app") {
        let mut command = git::new_cmd("/usr/bin/open");
        command.arg("-a").arg(executable).arg(target);
        return command;
    }
    let mut command = git::new_cmd(&executable.to_string_lossy());
    if app.kind == "editor" {
        command.arg(target);
    } else {
        // 路径作为工作目录或独立参数传递，绝不拼进 shell 命令。
        command.current_dir(target);
        match app.id {
            "windows-terminal" => {
                command.args(["-w", "new", "new-tab", "-d"]).arg(".");
            }
            "git-bash" => {
                command.arg("--cd=.");
            }
            "gnome-terminal" => {
                command.arg("--working-directory").arg(target);
            }
            "konsole" => {
                command.arg("--workdir").arg(target);
            }
            _ => {}
        }
    }
    command
}

fn launch(path: String, kind: &str) -> Result<(), String> {
    let prefs = get_launch_preferences()?;
    let id = if kind == "editor" {
        &prefs.editor
    } else {
        &prefs.terminal
    };
    launch_selected(path, kind, id)
}

fn launch_selected(path: String, kind: &str, id: &str) -> Result<(), String> {
    let target = PathBuf::from(path);
    if !target.is_absolute() || !target.exists() || (kind == "terminal" && !target.is_dir()) {
        return Err("目标路径不存在，或不是有效的完整目录路径。".into());
    }
    let (app, executable) = selected_app(kind, id)?;
    #[cfg(windows)]
    if let Some(arguments) = console_arguments(app.id) {
        windows_console::spawn(&executable, arguments, &target)
            .map_err(|e| format!("无法启动 {}: {e}", app.name))?;
        return Ok(());
    }
    let mut command = launch_command(&app, &executable, &target);
    // Electron 编辑器不应继承父进程的 Node 模式。
    command.env_remove("ELECTRON_RUN_AS_NODE");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 {}: {e}。请检查配置中的应用路径。", app.name))?;
    // 后台回收句柄；不阻塞界面等待编辑器或交互终端退出。
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[tauri::command]
pub async fn open_in_editor(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launch(path, "editor"))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn open_in_terminal(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launch(path, "terminal"))
        .await
        .map_err(|e| e.to_string())?
}

// ZCode 是独立入口，不走“默认编辑器”偏好设置
#[tauri::command]
pub async fn open_in_zcode(path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || launch_selected(path, "editor", "zcode"))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("gitgrove-launch-{}-{stamp}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn preferences_round_trip_replace_and_invalid_json() {
        let scratch = Scratch::new();
        let path = scratch.0.join("preferences.json");
        assert_eq!(
            read_preferences(&path).unwrap(),
            LaunchPreferences::default()
        );
        let prefs = LaunchPreferences {
            editor: "cursor".into(),
            terminal: "cmd".into(),
        };
        write_preferences(&path, &prefs).unwrap();
        assert_eq!(read_preferences(&path).unwrap(), prefs);
        write_preferences(&path, &LaunchPreferences::default()).unwrap();
        assert_eq!(
            read_preferences(&path).unwrap(),
            LaunchPreferences::default()
        );
        std::fs::write(&path, "invalid").unwrap();
        assert!(read_preferences(&path).is_err());
    }

    #[test]
    fn rejects_unknown_choices_and_missing_targets() {
        assert!(selected_app("editor", "notepad").is_err());
        assert!(selected_app("terminal", "vscode").is_err());
        assert!(launch("relative/path".into(), "editor").is_err());
        let scratch = Scratch::new();
        let file = scratch.0.join("file.txt");
        std::fs::write(&file, "test").unwrap();
        assert!(launch(file.to_string_lossy().into(), "terminal").is_err());
    }

    #[test]
    fn process_matching_accounts_for_linux_comm_truncation_only() {
        assert!(process_name_matches(
            "gnome-terminal-",
            "gnome-terminal-server",
            true
        ));
        assert!(process_name_matches(
            "gnome-terminal-server",
            "gnome-terminal-server",
            true
        ));
        assert!(!process_name_matches(
            "gnome-terminal-",
            "gnome-terminal-server",
            false
        ));
        assert!(!process_name_matches(
            "gnome-terminal",
            "gnome-terminal-server",
            true
        ));
        assert!(process_name_matches(
            "windowsterminal.exe",
            "windowsterminal.exe",
            false
        ));
        assert!(!process_name_matches(
            "windowsterminal",
            "windowsterminal.exe",
            false
        ));
    }

    #[cfg(windows)]
    #[test]
    fn finds_gui_executable_from_cli_bin_and_common_install_without_path() {
        let scratch = Scratch::new();
        let root = scratch.0.join("Microsoft VS Code");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("code.cmd"), "").unwrap();
        let mut app = spec("vscode", "VS Code", "editor", "Code.exe", &[]);
        assert!(resolve_in(&app, &[bin.clone()]).is_none());
        let exe = root.join("Code.exe");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(resolve_in(&app, &[bin]), Some(exe.clone()));
        app.locations.push(exe.clone());
        assert_eq!(resolve_in(&app, &[]), Some(exe));
    }

    #[cfg(windows)]
    #[test]
    fn launch_paths_are_data_even_with_shell_metacharacters() {
        let target = Path::new(r"C:\项目 & test\O'Brien %PATH%; $value");
        for app in catalog() {
            let command = launch_command(&app, Path::new(app.executable), target);
            let args: Vec<_> = command
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            if app.kind == "editor" {
                assert_eq!(args, vec![target.to_string_lossy()]);
            } else {
                assert_eq!(command.get_current_dir(), Some(target));
                assert!(!args
                    .iter()
                    .any(|arg| arg.contains("%PATH%") || arg == "/c" || arg == "-Command"));
            }
        }
    }

    #[test]
    #[ignore = "read-only snapshot of locally installed applications"]
    fn local_discovery_snapshot() {
        println!("{}", serde_json::to_string_pretty(&discover()).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn real_powershell_keeps_special_character_working_directory() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, TerminateProcess, WaitForSingleObject,
        };
        let (_, executable) = selected_app("terminal", "powershell").unwrap();
        let scratch = Scratch::new();
        let target = scratch.0.join("项目 & O'Brien %PATH%; $value");
        std::fs::create_dir_all(&target).unwrap();
        // 即使 cargo test 的 stdio 是管道，新终端也必须使用自己的控制台。
        // 使用固定的相对文件名回传测试结果，绝不把 target 拼入 PowerShell 代码。
        let process = windows_console::spawn_hidden(&executable,
            "-NoExit -NoProfile -NonInteractive -Command \"$r = @{cwd=(Get-Location).Path; input=[Console]::IsInputRedirected; output=[Console]::IsOutputRedirected; error=[Console]::IsErrorRedirected}; [IO.File]::WriteAllText((Join-Path (Get-Location).Path 'result.json'), ($r | ConvertTo-Json)); exit\"",
            &target).unwrap();
        // SAFETY: process 是当前测试创建且仍持有的进程句柄；只允许终止该测试进程。
        let status = unsafe { WaitForSingleObject(process.as_raw_handle(), 15_000) };
        if status != WAIT_OBJECT_0 {
            unsafe {
                TerminateProcess(process.as_raw_handle(), 1);
            }
            panic!("PowerShell 原生测试未在 15 秒内完成: {status}");
        }
        let mut exit_code = 0;
        assert_ne!(
            unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit_code) },
            0
        );
        assert_eq!(exit_code, 0);
        let result: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(target.join("result.json")).unwrap())
                .unwrap();
        for stream in ["input", "output", "error"] {
            assert_eq!(result[stream], false, "{stream} 不应继承父进程重定向");
        }
        // Windows TEMP 可能使用 RUNNER~1 等 8.3 短路径，而 PowerShell 会返回长路径。
        // 比较真实目录身份，避免把同一目录的两种拼写误判为启动位置错误。
        assert_eq!(
            std::fs::canonicalize(result["cwd"].as_str().unwrap()).unwrap(),
            std::fs::canonicalize(&target).unwrap()
        );
    }
}
