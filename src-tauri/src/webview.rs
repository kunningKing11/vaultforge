use tauri::{Runtime, WebviewWindow};

pub fn disable_zoom<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.set_zoom(1.0)?;

    // On Windows, `zoomHotkeysEnabled: false` is applied directly to WebView2's
    // zoom-control and pinch-zoom settings by Wry.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    disable_native_gesture_zoom(window)?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn disable_native_gesture_zoom<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.with_webview(|native_webview| {
        use gdk::{EventMask, EventType, ModifierType};
        use gtk::prelude::{WidgetExt, WidgetExtManual};
        use webkit2gtk::{WebViewExt, glib::Propagation};

        let webview = native_webview.inner();
        webview.set_zoom_level(1.0);
        webview.add_events(
            EventMask::TOUCHPAD_GESTURE_MASK
                | EventMask::SCROLL_MASK
                | EventMask::SMOOTH_SCROLL_MASK,
        );
        webview.connect_event(|_, event| {
            let is_pinch = event.event_type() == EventType::TouchpadPinch;
            let is_control_scroll = event.event_type() == EventType::Scroll
                && event
                    .state()
                    .is_some_and(|state| state.contains(ModifierType::CONTROL_MASK));

            if is_pinch || is_control_scroll {
                Propagation::Stop
            } else {
                Propagation::Proceed
            }
        });
    })
}

#[cfg(target_os = "macos")]
fn disable_native_gesture_zoom<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.with_webview(|native_webview| {
        // SAFETY: Tauri documents `inner()` as a valid WKWebView pointer on macOS,
        // and the reference does not outlive this `with_webview` callback.
        unsafe {
            let webview: &objc2_web_kit::WKWebView = &*native_webview.inner().cast();
            webview.setMagnification(1.0);
            webview.setAllowsMagnification(false);
        }
    })
}
