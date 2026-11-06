//! 窗口控制 IPC 命令。

/// 把主窗口移动到指定物理像素坐标（多显示器场景使用）。
#[tauri::command]
pub fn set_window_position(window: tauri::Window, x: i32, y: i32) -> Result<(), String> {
    window.set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}
