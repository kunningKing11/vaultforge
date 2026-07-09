use std::sync::Mutex;
use tauri::Manager;

mod activity;
mod assets;
mod commands;
mod derivation;
mod dto;
mod providers;
mod state;
mod storage;
#[cfg(test)]
mod tests;
mod tx;
mod validation;

use state::AppState;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let icon =
                tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))?.to_owned();
            if let Some(window) = app.get_webview_window("main") {
                window.set_icon(icon)?;
            }

            let storage_path = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve app data directory: {error}"))?
                .join("wallet.json");
            app.manage(Mutex::new(AppState::from_storage(storage_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::wallet::get_wallet,
            commands::market::refresh_prices,
            commands::wallet::create_wallet,
            commands::wallet::import_wallet,
            commands::wallet::unlock_wallet,
            commands::wallet::lock_wallet,
            commands::wallet::clear_wallet,
            commands::tx::sign_transaction,
            commands::tx::send_transaction,
            commands::tx::swap_tokens,
            commands::tx::check_transaction_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
