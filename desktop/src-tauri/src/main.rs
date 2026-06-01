#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod agent;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            agent::agent_status,
            agent::start_tunnel,
            agent::stop_tunnel,
            agent::start_agent,
            agent::open_web,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
