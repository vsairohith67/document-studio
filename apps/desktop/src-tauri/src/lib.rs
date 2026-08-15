use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemStatus {
    product: &'static str,
    offline_by_default: bool,
    phase: &'static str,
}

#[tauri::command]
fn system_status() -> SystemStatus {
    SystemStatus { product: "Document Studio", offline_by_default: true, phase: "foundation" }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![system_status])
        .run(tauri::generate_context!())
        .expect("error while running Document Studio");
}
