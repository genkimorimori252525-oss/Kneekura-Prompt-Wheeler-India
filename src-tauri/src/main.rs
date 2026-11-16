#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

fn reveal(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn hide(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let show_item = MenuItem::with_id(app, "show", "ウィンドウを表示", true, None::<&str>)?;
            let hide_item = MenuItem::with_id(app, "hide", "ウィンドウを隠す", true, None::<&str>)?;
            let exit_item = MenuItem::with_id(app, "exit", "終了", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &hide_item, &exit_item])?;

            // Rust owns the tray icon, so hiding the webview does not lose the
            // ability to reopen it. Explicit "終了" really exits the process.
            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("bundle icon is required").clone())
                .tooltip("Kneekura Prompt Wheeler")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => reveal(app),
                    "hide" => hide(app),
                    "exit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                reveal(app);
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("Kneekura Prompt Wheeler failed to start");
}
