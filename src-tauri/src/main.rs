mod commands;

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tauri::{
    Emitter, Manager,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

#[derive(Default)]
struct PopoverFocus {
    is_open: bool,
    generation: u64,
    opened_at: Option<Instant>,
    gained_focus: bool,
}

impl PopoverFocus {
    fn opened(&mut self) {
        self.is_open = true;
        self.generation = self.generation.wrapping_add(1);
        self.opened_at = Some(Instant::now());
        self.gained_focus = false;
    }

    fn closed(&mut self) {
        self.is_open = false;
        self.generation = self.generation.wrapping_add(1);
        self.opened_at = None;
        self.gained_focus = false;
    }

    fn focus_changed(&mut self, focused: bool) -> bool {
        if !self.is_open {
            return false;
        }
        if focused {
            self.generation = self.generation.wrapping_add(1);
            self.gained_focus = true;
            return false;
        }
        self.gained_focus
            && self
                .opened_at
                .is_some_and(|opened_at| opened_at.elapsed() >= Duration::from_millis(350))
    }

    fn can_hide_after_blur(&self, generation: u64) -> bool {
        self.is_open && self.generation == generation
    }
}

type SharedPopoverFocus = Arc<Mutex<PopoverFocus>>;

#[derive(Default)]
struct TrayClickGate {
    last_down: Option<Instant>,
}

impl TrayClickGate {
    fn should_toggle(&mut self, phase: MouseButtonState) -> bool {
        match phase {
            MouseButtonState::Down => {
                self.last_down = Some(Instant::now());
                true
            }
            MouseButtonState::Up => !self
                .last_down
                .take()
                .is_some_and(|pressed_at| pressed_at.elapsed() < Duration::from_secs(2)),
        }
    }
}

pub(crate) fn mark_popover_closed(app: &tauri::AppHandle) {
    if let Some(focus) = app.try_state::<SharedPopoverFocus>() {
        if let Ok(mut state) = focus.lock() {
            state.closed();
        }
    }
}

#[cfg(target_os = "macos")]
fn configure_fullscreen_popover(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};

    // AppKit requires a separate full-screen behavior in addition to joining desktop Spaces.
    let native_window = window.ns_window()? as usize;
    window.run_on_main_thread(move || unsafe {
        let native_window = &*(native_window as *const NSWindow);
        let behavior = native_window.collectionBehavior()
            & !(NSWindowCollectionBehavior::FullScreenPrimary
                | NSWindowCollectionBehavior::FullScreenNone
                | NSWindowCollectionBehavior::Primary
                | NSWindowCollectionBehavior::Auxiliary);
        native_window.setCollectionBehavior(
            behavior
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::CanJoinAllApplications,
        );
    })
}

fn show_popover(
    app: &tauri::AppHandle,
    click_rect: Option<tauri::Rect>,
    focus: &SharedPopoverFocus,
) {
    if let Some(window) = app.get_webview_window("main") {
        if focus.lock().map(|state| state.is_open).unwrap_or(false) {
            let _ = window.set_focus();
            return;
        }

        if let Ok(mut state) = focus.lock() {
            state.opened();
        }
        if let Some(rect) = click_rect {
            let window_scale = window.scale_factor().unwrap_or(1.0);
            let physical_pos = rect.position.to_physical::<f64>(window_scale);
            let physical_size = rect.size.to_physical::<f64>(window_scale);
            let click_center_x = physical_pos.x + physical_size.width / 2.0;
            let click_center_y = physical_pos.y + physical_size.height / 2.0;
            let monitor = window
                .available_monitors()
                .ok()
                .and_then(|monitors| {
                    monitors.into_iter().find(|monitor| {
                        let origin = monitor.position();
                        let size = monitor.size();
                        click_center_x >= f64::from(origin.x)
                            && click_center_x < f64::from(origin.x) + f64::from(size.width)
                            && click_center_y >= f64::from(origin.y)
                            && click_center_y < f64::from(origin.y) + f64::from(size.height)
                    })
                })
                .or_else(|| window.current_monitor().ok().flatten());
            let scale_factor = monitor
                .as_ref()
                .map(|monitor| monitor.scale_factor())
                .unwrap_or(window_scale);
            let logical_pos = rect.position.to_logical::<f64>(scale_factor);
            let logical_size = rect.size.to_logical::<f64>(scale_factor);
            let mut popover_w: f64 = 420.0;
            let mut popover_h: f64 = 660.0;
            let tray_center_x = logical_pos.x + (logical_size.width / 2.0);
            let mut target_x = tray_center_x - (popover_w / 2.0);
            let mut target_y = logical_pos.y + logical_size.height + 4.0;
            if let Some(monitor) = monitor {
                let screen_pos = monitor.position().to_logical::<f64>(scale_factor);
                let screen_size = monitor.size().to_logical::<f64>(scale_factor);
                let right = screen_pos.x + screen_size.width - 12.0;
                let bottom = screen_pos.y + screen_size.height - 12.0;
                popover_w = popover_w.min((screen_size.width - 24.0).max(1.0));
                let min_x = screen_pos.x + 12.0;
                let max_x = (right - popover_w).max(min_x);
                target_x = (tray_center_x - popover_w / 2.0).clamp(min_x, max_x);
                target_y = target_y.min(bottom - 240.0);
                popover_h = popover_h.min((bottom - target_y).max(240.0));
            }
            let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                width: (popover_w * scale_factor).round().max(1.0) as u32,
                height: (popover_h * scale_factor).round().max(1.0) as u32,
            }));
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                x: (target_x * scale_factor).round() as i32,
                y: (target_y * scale_factor).round() as i32,
            }));
        }
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_title(Some("ThreadBack"));
        }
        if window.show().is_err() {
            if let Ok(mut state) = focus.lock() {
                state.closed();
            }
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_title(None::<String>);
            }
            return;
        }
        let _ = window.set_focus();
        let _ = window.emit("popover-opened", ());
    }
}

fn toggle_popover(
    app: &tauri::AppHandle,
    click_rect: Option<tauri::Rect>,
    focus: &SharedPopoverFocus,
) {
    if focus.lock().map(|state| state.is_open).unwrap_or(false) {
        if let Some(window) = app.get_webview_window("main") {
            if let Ok(mut state) = focus.lock() {
                state.closed();
            }
            let _ = window.hide();
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_title(None::<String>);
            }
        }
    } else {
        show_popover(app, click_rect, focus);
    }
}

fn setup_tray_and_window(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    commands::initialize(app)?;
    let focus = Arc::new(Mutex::new(PopoverFocus::default()));
    app.manage(Arc::clone(&focus));

    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "macos")]
        configure_fullscreen_popover(&window)?;

        let app_handle = app.handle().clone();
        let focus_for_window = Arc::clone(&focus);
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                let pending_hide = focus_for_window.lock().ok().and_then(|mut state| {
                    state.focus_changed(*focused).then_some(state.generation)
                });
                if let Some(generation) = pending_hide {
                    let app_handle = app_handle.clone();
                    let focus_for_window = Arc::clone(&focus_for_window);
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(180));
                        let should_hide = focus_for_window
                            .lock()
                            .map(|mut state| {
                                if state.can_hide_after_blur(generation) {
                                    state.closed();
                                    true
                                } else {
                                    false
                                }
                            })
                            .unwrap_or(false);
                        if should_hide {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.hide();
                            }
                            if let Some(tray) = app_handle.tray_by_id("main") {
                                let _ = tray.set_title(None::<String>);
                            }
                        }
                    });
                }
            }
        });
    }

    let show_item = MenuItemBuilder::with_id("show", "Open ThreadBack").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit ThreadBack").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let focus_for_menu = Arc::clone(&focus);
    let focus_for_click = Arc::clone(&focus);
    let click_gate = Mutex::new(TrayClickGate::default());
    let _tray = TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray-evidence-list.png"))
        .icon_as_template(true)
        .tooltip("ThreadBack")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => toggle_popover(app, None, &focus_for_menu),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                rect,
                ..
            } = event
            {
                if button == MouseButton::Left {
                    let should_toggle = click_gate
                        .lock()
                        .map(|mut gate| gate.should_toggle(button_state))
                        .unwrap_or(button_state == MouseButtonState::Up);
                    if should_toggle {
                        toggle_popover(tray.app_handle(), Some(rect), &focus_for_click);
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(focus) = app.try_state::<SharedPopoverFocus>() {
                show_popover(app, None, &focus);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .setup(setup_tray_and_window)
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::popover_close,
            commands::popover_set_status,
            commands::app_quit,
            commands::projects_create,
            commands::projects_relink,
            commands::projects_list,
            commands::projects_default_exclusions,
            commands::projects_get_policy,
            commands::projects_update_policy,
            commands::recovery_reports,
            commands::threads_list,
            commands::threads_start,
            commands::threads_pause,
            commands::threads_pause_all,
            commands::threads_resume,
            commands::threads_end,
            commands::threads_archive,
            commands::threads_delete,
            commands::checkpoints_save_place,
            commands::checkpoints_list,
            commands::checkpoints_get,
            commands::checkpoints_get_resume,
            commands::checkpoints_open_artifact,
            commands::checkpoints_approve,
            commands::checkpoints_corrections,
            commands::checkpoints_preview,
            commands::checkpoints_generate,
            commands::checkpoints_discard,
            commands::checkpoints_cancel_generation,
            commands::exports_json,
            commands::exports_markdown,
            commands::privacy_test_redaction,
            commands::privacy_expire_events,
            commands::configuration_status,
            commands::configuration_store,
            commands::configuration_remove_key,
            commands::configuration_test,
            commands::events_list,
            commands::events_add_note,
            commands::events_capture_git,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ThreadBack");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popover_ignores_opening_blur_and_hides_after_real_focus_loss() {
        let mut focus = PopoverFocus::default();
        focus.opened();

        assert!(!focus.focus_changed(false));
        assert!(!focus.focus_changed(true));
        assert!(!focus.focus_changed(false));

        focus.opened_at = Some(Instant::now() - Duration::from_secs(1));
        assert!(focus.focus_changed(false));
    }

    #[test]
    fn reopening_resets_the_previous_focus_cycle() {
        let mut focus = PopoverFocus::default();
        focus.opened();
        focus.focus_changed(true);
        focus.opened();
        focus.opened_at = Some(Instant::now() - Duration::from_secs(1));

        assert!(!focus.focus_changed(false));
    }

    #[test]
    fn menu_bar_clicks_toggle_even_when_focus_changes() {
        let mut focus = PopoverFocus::default();
        assert!(!focus.is_open);
        focus.opened();
        assert!(focus.is_open);
        focus.focus_changed(false);
        assert!(focus.is_open);
        focus.closed();
        assert!(!focus.is_open);
    }

    #[test]
    fn a_tray_click_cancels_a_pending_blur_hide() {
        let mut focus = PopoverFocus::default();
        focus.opened();
        focus.focus_changed(true);
        focus.opened_at = Some(Instant::now() - Duration::from_secs(1));
        assert!(focus.focus_changed(false));
        let pending_generation = focus.generation;

        focus.closed();
        assert!(!focus.can_hide_after_blur(pending_generation));

        focus.opened();
        assert!(!focus.can_hide_after_blur(pending_generation));
    }

    #[test]
    fn tray_press_toggles_once_even_when_down_and_up_both_arrive() {
        let mut gate = TrayClickGate::default();
        assert!(gate.should_toggle(MouseButtonState::Down));
        assert!(!gate.should_toggle(MouseButtonState::Up));
        assert!(gate.should_toggle(MouseButtonState::Up));
    }
}
