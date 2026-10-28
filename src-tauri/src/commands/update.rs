use tauri::utils::{config::BundleType, platform::bundle_type};

#[tauri::command]
pub(crate) fn updater_target() -> Option<(String, Option<&'static str>)> {
    let installer = match bundle_type() {
        Some(BundleType::Deb) => Some("deb"),
        Some(BundleType::Rpm) => Some("rpm"),
        Some(BundleType::AppImage) => Some("appimage"),
        Some(BundleType::Msi) => Some("msi"),
        Some(BundleType::Nsis) => Some("nsis"),
        Some(BundleType::App) => Some("app"),
        _ => None,
    };
    tauri_plugin_updater::target().map(|target| (target, installer))
}
