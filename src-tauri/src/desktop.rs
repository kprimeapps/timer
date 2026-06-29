use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use futures_util::stream::StreamExt;
use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};
use time::{Date, Duration, Month, OffsetDateTime, Time};
use tokio::sync::broadcast;

const TRIAL_SECONDS: u64 = 600;
const UNLOCK_CODE: &str = match option_env!("UNLOCK_CODE") {
    Some(code) => code,
    None => "dev-unlock-change-me",
};
const WS_PORT: u16 = 4337;
static ACTUAL_WS_PORT: AtomicU16 = AtomicU16::new(WS_PORT);

#[derive(Clone)]
struct AppState {
    tx: broadcast::Sender<String>,
    app: tauri::AppHandle,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn hmac_hex(secret: &str, data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(data.as_bytes());
    hasher.finalize()[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn parse_hex_byte(h: &str) -> Result<u8, String> {
    u8::from_str_radix(h, 16).map_err(|_| "bad hex".into())
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
    if parts.len() != 3 || parts[0].len() != 2 || parts[1].len() != 2 || parts[2].len() != 4 {
        return Err("Use format MM-DD-XXXX".into());
    }
    let mm_hex = parts[0];
    let dd_hex = parts[1];
    let provided = parts[2];
    let mm_dec = parse_hex_byte(mm_hex)?;
    let dd_dec = parse_hex_byte(dd_hex)?;
    if mm_dec < 1 || mm_dec > 12 || dd_dec < 1 || dd_dec > 31 {
        return Err("Invalid date".into());
    }
    let now = OffsetDateTime::now_utc();
    let month = Month::try_from(mm_dec).map_err(|_| "bad month")?;
    let years = [now.year(), now.year() - 1];
    for &year in &years {
        let data = format!("{:04}{:02}{:02}", year, mm_dec, dd_dec);
        let expected = &hmac_hex(UNLOCK_CODE, &data)[..4];
        if expected == provided {
            let code_date = Date::from_calendar_date(year, month, dd_dec)
                .map_err(|_| "bad date")?;
            let code_time = code_date.with_time(Time::from_hms(0, 0, 0).unwrap()).assume_utc();
            let diff = now - code_time;
            if diff >= Duration::hours(-1) && diff <= Duration::days(7) {
                let mut state = load_state(&app);
                state.unlocked = true;
                state.show_welcome = false;
                save_state(&app, &state);
                return Ok("ok".into());
            }
            return Err("Code expired".into());
        }
    }
    Err("Invalid code".into())
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

#[tauri::command]
fn broadcast_state(app: tauri::AppHandle, state: String) {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&state) {
        let _ = app.emit("broadcast-state", &val);
    }
}

#[tauri::command]
fn get_remote_url() -> String {
    let ip = local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = ACTUAL_WS_PORT.load(Ordering::Relaxed);
    format!("http://{}:{}", ip, port)
}

fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:53").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let app = state.app.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                let _ = app.emit("mobile-command", text.clone());
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => {},
        _ = (&mut recv_task) => {},
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../../mobile.html"))
}

fn find_available_port(start: u16) -> u16 {
    for port in start..start + 100 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = rt.enter();

    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let (tx, _) = broadcast::channel(32);

            let state = AppState { tx: tx.clone(), app: handle.clone() };
            let port = find_available_port(WS_PORT);
            ACTUAL_WS_PORT.store(port, Ordering::Relaxed);
            let app_state = state.clone();

            tokio::spawn(async move {
                let router = Router::new()
                    .route("/", get(index_handler))
                    .route("/ws", get(ws_handler))
                    .with_state(app_state);

                let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
                    .await
                    .expect("Failed to bind WebSocket server");
                axum::serve(listener, router).await.unwrap();
            });

            let tx_clone = tx.clone();
            handle.listen("broadcast-state", move |event| {
                let _ = tx_clone.send(event.payload().to_owned());
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_displays,
            open_cast,
            close_cast,
            send_cast_state,
            get_trial_status,
            unlock_app,
            dismiss_welcome,
            broadcast_state,
            get_remote_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
