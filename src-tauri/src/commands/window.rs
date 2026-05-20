#[tauri::command]
pub fn set_window_position(window: tauri::Window, x: i32, y: i32) -> Result<(), String> {
    window.set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())
}
