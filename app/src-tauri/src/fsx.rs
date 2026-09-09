// 文件系统：懒加载目录、文件预览、git status 角标、回收站删除、系统打开

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::git;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
}

#[tauri::command]
pub fn read_dir(path: String) -> Result<Vec<DirEntry>, String> {
    let rd = std::fs::read_dir(&path).map_err(|e| format!("读取目录失败 {path}: {e}"))?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(DirEntry { name, is_dir });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePreview {
    pub text: String,
    pub truncated: bool,
    pub is_binary: bool,
}

const PREVIEW_LIMIT: usize = 256 * 1024;

#[tauri::command]
pub fn read_file_preview(path: String) -> Result<FilePreview, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(&path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut buf = Vec::new();
    f.by_ref()
        .take((PREVIEW_LIMIT + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    let truncated = buf.len() > PREVIEW_LIMIT;
    buf.truncate(PREVIEW_LIMIT);
    let is_binary = buf.iter().take(8192).any(|&b| b == 0);
    Ok(FilePreview {
        text: if is_binary { String::new() } else { String::from_utf8_lossy(&buf).to_string() },
        truncated,
        is_binary,
    })
}

#[tauri::command]
pub fn checkout_status(path: String) -> Result<git::StatusMap, String> {
    git::status_porcelain(Path::new(&path))
}

// Markdown 预览里的相对路径图片走 asset protocol 加载，按目录收紧作用域
#[tauri::command]
pub fn allow_asset_scope(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri::Manager;
    app.asset_protocol_scope()
        .allow_directory(Path::new(&path), true)
        .map_err(|e| format!("允许资源目录失败 {path}: {e}"))
}

#[tauri::command]
pub fn trash_path(path: String) -> Result<(), String> {
    trash::delete(&path).map_err(|e| format!("移入回收站失败: {e}"))
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    // 路径不存在时 explorer 会静默落到默认位置（文档），提前报错
    if !p.exists() {
        return Err(format!("路径不存在，无法定位: {path}"));
    }
    // cfg! 是运行时宏，两个分支在所有平台都要可编译；explorer_arguments 只在非 macOS 存在，
    // 所以用 #[cfg] 属性分别编译两个 let 绑定
    #[cfg(target_os = "macos")]
    let mut c = {
        let mut c = git::new_cmd("open");
        if p.is_dir() {
            c.arg(&path);
        } else {
            c.arg("-R").arg(&path);
        }
        c
    };
    #[cfg(not(target_os = "macos"))]
    let mut c = {
        let mut c = git::new_cmd("explorer");
        c.args(explorer_arguments(&path, p.is_dir()));
        c
    };
    // explorer 经常返回非零退出码，忽略之
    c.spawn().map_err(|e| format!("无法打开文件管理器: {e}"))?;
    Ok(())
}

// explorer.exe 只认反斜杠路径；git 输出（worktree list 等）是正斜杠，
// 直接传入会被静默忽略并打开默认位置
#[cfg(not(target_os = "macos"))]
fn explorer_arguments(path: &str, is_dir: bool) -> Vec<String> {
    let native = path.replace('/', "\\");
    if is_dir {
        vec![native]
    } else {
        vec![format!("/select,{native}")]
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::explorer_arguments;

    #[test]
    fn explorer_args_use_native_separators() {
        // git worktree list --porcelain 输出正斜杠路径，explorer 需要反斜杠
        let mixed = "C:/Users/admin/gh-projects/demo";
        assert_eq!(
            explorer_arguments(mixed, true),
            vec![r"C:\Users\admin\gh-projects\demo"]
        );
        assert_eq!(
            explorer_arguments(mixed, false),
            vec![r"/select,C:\Users\admin\gh-projects\demo"]
        );
        // 已是反斜杠的路径保持不变
        assert_eq!(
            explorer_arguments(r"C:\proj\demo", true),
            vec![r"C:\proj\demo"]
        );
    }
}
