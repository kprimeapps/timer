use serde::Serialize;
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, Serialize)]
struct MonitorInfo {
    name: String,
    width: f64,
    height: f64,
    x: f64,
    y: f64,
    primary: bool,
}

#[tauri::command]
fn get_displays(app: tauri::AppHandle) -> Vec<MonitorInfo> {
    let monitors = app.available_monitors().unwrap_or_default();
    let primary = app.primary_monitor().ok().flatten();
    monitors
        .iter()
        .map(|m| {
            let pos = m.position();
            let size = m.size();
            MonitorInfo {
                name: m.name().map_or("Unknown".to_string(), |v| v.clone()),
                width: size.width as f64,
                height: size.height as f64,
                x: pos.x as f64,
                y: pos.y as f64,
                primary: primary.as_ref().map(|p| p.name() == m.name()).unwrap_or(false),
            }
        })
        .collect()
}

#[tauri::command]
async fn open_cast(app: tauri::AppHandle, monitor_name: String) -> Result<(), String> {
    // Close existing cast window if any
    if let Some(w) = app.get_webview_window("cast") {
        w.close().map_err(|e| e.to_string())?;
    }

    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    let monitor = monitors
        .iter()
        .find(|m| m.name().as_deref() == Some(&monitor_name))
        .ok_or_else(|| "Monitor not found".to_string())?;

    let pos = monitor.position();
    let size = monitor.size();

    let cast_window = WebviewWindowBuilder::new(&app, "cast", WebviewUrl::App("receiver.html".into()))
        .position(pos.x as f64, pos.y as f64)
        .inner_size(size.width as f64, size.height as f64)
        .fullscreen(true)
        .decorations(false)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;

    let app_clone = app.clone();
    cast_window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event {
            let _ = app_clone.emit("cast-closed", ());
        }
    });

    Ok(())
}

#[tauri::command]
async fn close_cast(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("cast") {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn send_cast_state(app: tauri::AppHandle, state: serde_json::Value) -> Result<(), String> {
    if let Some(cast) = app.get_webview_window("cast") {
        cast.emit("cast-state", state).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_displays,
            open_cast,
            close_cast,
            send_cast_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
