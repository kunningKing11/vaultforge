use std::sync::Mutex;
use tauri::Manager;

mod activity;
mod address;
mod assets;
mod cctp;
mod cctp_attestation;
mod cctp_execution;
mod cctp_mint;
mod cctp_native;
mod commands;
mod derivation;
mod dto;
mod jupiter;
mod operations;
mod providers;
mod registry;
mod state;
mod storage;
mod swap_routes;
mod swap_solana;
#[cfg(test)]
mod tests;
mod tx;
mod uniswap;
mod uniswap_config;
mod validation;
mod webview;

use providers::http::ProviderClients;
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build());

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
        .plugin(tauri_plugin_opener::init())
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
            commands::wallet::update_wallet_settings,
            commands::wallet::create_wallet,
            commands::wallet::import_wallet,
            commands::wallet::unlock_wallet,
            commands::wallet::lock_wallet,
            commands::wallet::clear_wallet,
            commands::tx::sign_transaction,
            commands::tx::send_transaction,
            commands::tx::cancel_signed_transaction,
            commands::tx::swap_tokens,
            commands::swap::get_uniswap_state,
            commands::swap::quote_uniswap_swap,
            commands::swap::create_uniswap_swap,
            commands::swap::prepare_uniswap_swap,
            commands::swap::submit_uniswap_swap,
            commands::swap::sync_uniswap_swap,
            commands::swap::cancel_uniswap_swap,
            commands::tx::check_transaction_status,
            commands::cctp::poll_cctp_attestation,
            commands::cctp::refresh_cctp_attestation,
            commands::cctp::get_cctp_state,
            commands::cctp::create_cctp_transfer,
            commands::cctp::prepare_cctp_step,
            commands::cctp::submit_cctp_step,
            commands::cctp::cancel_cctp_transfer,
            commands::cctp::sync_cctp_transfer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VaultForge Wallet");
}
