use std::sync::Mutex;
use tauri::Manager;

mod activity;
mod address;
mod assets;
mod commands;
mod derivation;
mod dto;
mod providers;
mod registry;
mod state;
mod storage;
#[cfg(test)]
mod tests;
mod tx;
mod validation;
mod webview;

use providers::http::ProviderClients;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app
                .get_webview_window("main")
                .expect("no main window")
                .set_focus();
        }));
    }

    builder
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let icon =
                tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))?.to_owned(); // TODO: upscale
            if let Some(window) = app.get_webview_window("main") {
                webview::disable_zoom(&window)?;
                window.set_icon(icon)?;
            }

            let storage_path = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve app data directory: {error}"))?
                .join("wallet.json");
            app.manage(ProviderClients::new()?);
            app.manage(Mutex::new(AppState::from_storage(storage_path)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::wallet::get_wallet,
            commands::wallet::generate_mnemonic_cmd,
            commands::market::refresh_portfolio,
            commands::market::set_fiat_currency,
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
