use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use time::{Date, Month, OffsetDateTime, Time};

const TRIAL_SECONDS: u64 = 600;
const UNLOCK_CODE: &str = match option_env!("UNLOCK_CODE") {
    Some(code) => code,
    None => "dev-unlock-change-me",
};

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn hmac_hex(secret: &str, data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(data.as_bytes());
    hasher.finalize()[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn datetime_from_yyyymmddhh(s: &str) -> Result<OffsetDateTime, String> {
    if s.len() != 10 { return Err("bad length".into()); }
    let y = s[0..4].parse::<i32>().map_err(|_| "bad year")?;
    let m = s[4..6].parse::<u8>().map_err(|_| "bad month")?;
    let d = s[6..8].parse::<u8>().map_err(|_| "bad day")?;
    let h = s[8..10].parse::<u8>().map_err(|_| "bad hour")?;
    let month = Month::try_from(m).map_err(|_| "bad month")?;
    let date = Date::from_calendar_date(y, month, d).map_err(|_| "bad date")?;
    let time = Time::from_hms(h, 0, 0).map_err(|_| "bad time")?;
    Ok(date.with_time(time).assume_utc())
}

#[derive(Clone, Serialize, Deserialize)]
struct LicenseState {
    total_used_seconds: u64,
    last_save_at: u64,
    unlocked: bool,
    show_welcome: bool,
}

impl Default for LicenseState {
    fn default() -> Self {
        let t = now();
        Self { total_used_seconds: 0, last_save_at: t, unlocked: false, show_welcome: true }
    }
}

fn state_path(app: &tauri::AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir").join("license.json")
}

fn load_state(app: &tauri::AppHandle) -> LicenseState {
    let path = state_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(app: &tauri::AppHandle, state: &LicenseState) {
    let path = state_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string(state).unwrap());
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrialStatus {
    remaining_seconds: u64,
    unlocked: bool,
    show_welcome: bool,
}

#[tauri::command]
fn get_trial_status(app: tauri::AppHandle) -> TrialStatus {
    let mut state = load_state(&app);
    let t = now();
    let elapsed = t.saturating_sub(state.last_save_at);
    state.total_used_seconds = state.total_used_seconds.saturating_add(elapsed);
    state.last_save_at = t;
    save_state(&app, &state);
    let remaining = TRIAL_SECONDS.saturating_sub(state.total_used_seconds);
    TrialStatus { remaining_seconds: remaining, unlocked: state.unlocked, show_welcome: state.show_welcome }
}

#[tauri::command]
fn unlock_app(app: tauri::AppHandle, code: String) -> Result<String, String> {
    let parts: Vec<&str> = code.split('-').collect();
    if parts.len() != 4 || parts[0] != "TIMER" {
        return Err("Invalid code format. Use TIMER-YYMMDD-HH-XXXX.".into());
    }
    let date = parts[1];
    let hour = parts[2];
    let provided = parts[3];
    if date.len() != 6 || hour.len() != 2 || provided.len() != 16 {
        return Err("Invalid code format".into());
    }
    let data = format!("{}{}", date, hour);
    let expected = hmac_hex(UNLOCK_CODE, &data);
    if provided != expected {
        return Err("Invalid unlock code".into());
    }
    let full = format!("20{}{}", date, hour);
    let code_time = datetime_from_yyyymmddhh(&full)?;
    let now = OffsetDateTime::now_utc();
    let diff = now - code_time;
    if diff > time::Duration::hours(48) {
        return Err("Code expired".into());
    }
    if diff < time::Duration::hours(-1) {
        return Err("Code not yet valid".into());
    }
    let mut state = load_state(&app);
    state.unlocked = true;
    state.show_welcome = false;
    save_state(&app, &state);
    Ok("ok".into())
}

#[tauri::command]
fn dismiss_welcome(app: tauri::AppHandle) {
    let mut state = load_state(&app);
    state.show_welcome = false;
    save_state(&app, &state);
}

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
            get_trial_status,
            unlock_app,
            dismiss_welcome,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
