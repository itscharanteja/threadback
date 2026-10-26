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
    dialog_open: bool,
    generation: u64,
    last_tray_click: Option<Instant>,
    #[cfg(target_os = "macos")]
    previous_presentation: Option<usize>,
}

impl PopoverFocus {
    fn opened(&mut self) {
        self.is_open = true;
        self.generation = self.generation.wrapping_add(1);
    }

    fn closed(&mut self) {
        self.is_open = false;
        self.dialog_open = false;
        self.generation = self.generation.wrapping_add(1);
    }

    fn tray_clicked(&mut self) {
        self.last_tray_click = Some(Instant::now());
        self.generation = self.generation.wrapping_add(1);
    }

    fn outside_click_generation(&self) -> Option<u64> {
        (self.is_open
            && !self.dialog_open
            && !self
                .last_tray_click
                .is_some_and(|clicked| clicked.elapsed() < Duration::from_millis(350)))
        .then_some(self.generation)
    }

    fn can_hide_after_outside_click(&self, generation: u64) -> bool {
        self.is_open && !self.dialog_open && self.generation == generation
    }

    fn begin_dialog(&mut self) {
        if self.is_open {
            self.dialog_open = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    fn end_dialog(&mut self) -> bool {
        if !self.is_open || !self.dialog_open {
            return false;
        }
        self.dialog_open = false;
        self.generation = self.generation.wrapping_add(1);
        true
    }
}

type SharedPopoverFocus = Arc<Mutex<PopoverFocus>>;

pub(crate) fn close_popover(app: &tauri::AppHandle) {
    if let Some(focus) = app.try_state::<SharedPopoverFocus>() {
        if let Ok(mut state) = focus.lock() {
            state.closed();
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        #[cfg(target_os = "macos")]
        restore_menu_bar(&window, app);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(None::<String>);
    }
}

#[cfg(target_os = "macos")]
fn pin_menu_bar(window: &tauri::WebviewWindow, app: &tauri::AppHandle) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationPresentationOptions};

    let app = app.clone();
    let apply = move || {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        let native_app = NSApplication::sharedApplication(main_thread);
        let Some(focus) = app.try_state::<SharedPopoverFocus>() else {
            return;
        };
        let Ok(mut state) = focus.lock() else {
            return;
        };
        if !state.is_open {
            return;
        }
        if state.previous_presentation.is_none() {
            state.previous_presentation = Some(native_app.presentationOptions().0);
        }
        drop(state);
        // A status-item click can change the active app before this runs, so
        // currentSystemPresentationOptions is not a reliable fullscreen gate.
        // Apply our presentation after the popover is shown and focused.
        if !native_app.isActive() {
            #[allow(deprecated)]
            native_app.activateIgnoringOtherApps(true);
        }
        native_app.setPresentationOptions(NSApplicationPresentationOptions::AutoHideDock);
    };
    if MainThreadMarker::new().is_some() {
        apply();
    } else {
        let _ = window.run_on_main_thread(apply);
    }
}

#[cfg(target_os = "macos")]
fn restore_menu_bar(window: &tauri::WebviewWindow, app: &tauri::AppHandle) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationPresentationOptions};

    let app = app.clone();
    let apply = move || {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        let previous = app.try_state::<SharedPopoverFocus>().and_then(|focus| {
            focus.lock().ok().and_then(|mut state| {
                (!state.is_open)
                    .then(|| state.previous_presentation.take())
                    .flatten()
            })
        });
        if let Some(previous) = previous {
            NSApplication::sharedApplication(main_thread)
                .setPresentationOptions(NSApplicationPresentationOptions(previous));
        }
    };
    if MainThreadMarker::new().is_some() {
        apply();
    } else {
        let _ = window.run_on_main_thread(apply);
    }
}

pub(crate) fn begin_popover_dialog(app: &tauri::AppHandle) {
    if let Some(focus) = app.try_state::<SharedPopoverFocus>() {
        if let Ok(mut state) = focus.lock() {
            state.begin_dialog();
        }
    }
}

pub(crate) fn end_popover_dialog(app: &tauri::AppHandle) {
    let should_refocus = app
        .try_state::<SharedPopoverFocus>()
        .and_then(|focus| focus.lock().ok().map(|mut state| state.end_dialog()))
        .unwrap_or(false);
    if should_refocus {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
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

#[cfg(target_os = "macos")]
fn install_outside_click_monitor(app: &tauri::AppHandle, focus: SharedPopoverFocus) {
    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask};

    let app = app.clone();
    let handler = RcBlock::new(move |_event: std::ptr::NonNull<NSEvent>| {
        let generation = focus
            .lock()
            .ok()
            .and_then(|state| state.outside_click_generation());
        let Some(generation) = generation else {
            return;
        };
        let app = app.clone();
        let focus = Arc::clone(&focus);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(180));
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.run_on_main_thread(move || {
                    if focus
                        .lock()
                        .map(|state| state.can_hide_after_outside_click(generation))
                        .unwrap_or(false)
                    {
                        close_popover(&app);
                    }
                });
            }
        });
    });

    let mask = NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp | NSEventMask::OtherMouseUp;
    if let Some(monitor) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &handler) {
        // Installed once for the app lifetime; AppKit removes it at process exit.
        std::mem::forget(monitor);
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq)]
struct PopoverFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
fn popover_frame(cursor_x: f64, screen: PopoverFrame, visible: PopoverFrame) -> PopoverFrame {
    const MARGIN: f64 = 12.0;
    const MENU_BAR_CLEARANCE: f64 = 28.0;
    let left = visible.x.max(screen.x) + MARGIN;
    let right = (visible.x + visible.width).min(screen.x + screen.width) - MARGIN;
    let bottom = visible.y.max(screen.y) + MARGIN;
    let top = (visible.y + visible.height).min(screen.y + screen.height - MENU_BAR_CLEARANCE);
    let width = 420.0_f64.min((right - left).max(1.0));
    let height = 660.0_f64.min((top - bottom).max(1.0));

    PopoverFrame {
        x: (cursor_x - width / 2.0).clamp(left, (right - width).max(left)),
        y: top - height,
        width,
        height,
    }
}

#[cfg(target_os = "macos")]
fn position_popover_on_cursor_display(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};

    let native_window = window.ns_window()? as usize;
    let place = move || {
        let Some(main_thread) = MainThreadMarker::new() else {
            return;
        };
        // Mouse location and NSScreen frames are both global AppKit points.
        // The tray event rectangle is physical pixels and can use the wrong
        // display scale when a second monitor has a different backing scale.
        let cursor = NSEvent::mouseLocation();
        let screens = NSScreen::screens(main_thread);
        let Some(screen) = screens.iter().find(|screen| {
            let frame = screen.frame();
            cursor.x >= frame.origin.x
                && cursor.x < frame.origin.x + frame.size.width
                && cursor.y >= frame.origin.y
                && cursor.y < frame.origin.y + frame.size.height
        }) else {
            return;
        };
        let screen_frame = screen.frame();
        let visible_frame = screen.visibleFrame();
        let target = popover_frame(
            cursor.x,
            PopoverFrame {
                x: screen_frame.origin.x,
                y: screen_frame.origin.y,
                width: screen_frame.size.width,
                height: screen_frame.size.height,
            },
            PopoverFrame {
                x: visible_frame.origin.x,
                y: visible_frame.origin.y,
                width: visible_frame.size.width,
                height: visible_frame.size.height,
            },
        );

        let native_window = unsafe { &*(native_window as *const NSWindow) };
        let mut frame = native_window.frame();
        frame.origin.x = target.x;
        frame.origin.y = target.y;
        frame.size.width = target.width;
        frame.size.height = target.height;
        native_window.setFrame_display(frame, true);
    };

    if MainThreadMarker::new().is_some() {
        place();
        Ok(())
    } else {
        window.run_on_main_thread(place)
    }
}

fn show_popover(
    app: &tauri::AppHandle,
    _click_rect: Option<tauri::Rect>,
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
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_title(Some("ThreadBack"));
        }
        #[cfg(target_os = "macos")]
        let _ = position_popover_on_cursor_display(&window);
        #[cfg(not(target_os = "macos"))]
        if let Some(rect) = _click_rect {
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
        if window.show().is_err() {
            if let Ok(mut state) = focus.lock() {
                state.closed();
            }
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_title(None::<String>);
            }
            #[cfg(target_os = "macos")]
            restore_menu_bar(&window, app);
            return;
        }
        let _ = window.set_focus();
        #[cfg(target_os = "macos")]
        pin_menu_bar(&window, app);
        let _ = window.emit("popover-opened", ());
    }
}

fn toggle_popover(
    app: &tauri::AppHandle,
    click_rect: Option<tauri::Rect>,
    focus: &SharedPopoverFocus,
) {
    if let Ok(mut state) = focus.lock() {
        state.tray_clicked();
    }
    if focus.lock().map(|state| state.is_open).unwrap_or(false) {
        close_popover(app);
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
    }

    #[cfg(target_os = "macos")]
    install_outside_click_monitor(app.handle(), Arc::clone(&focus));

    let show_item = MenuItemBuilder::with_id("show", "Open ThreadBack").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit ThreadBack").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .separator()
        .item(&quit_item)
        .build()?;

    let focus_for_menu = Arc::clone(&focus);
    let focus_for_click = Arc::clone(&focus);
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
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    toggle_popover(tray.app_handle(), Some(rect), &focus_for_click);
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
            commands::popover_begin_dialog,
            commands::popover_end_dialog,
            commands::app_quit,
            commands::projects_create,
            commands::projects_git_status,
            commands::projects_initialize_git,
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
    fn outside_click_dismisses_an_open_popover() {
        let mut focus = PopoverFocus::default();
        assert!(focus.outside_click_generation().is_none());
        focus.opened();
        let generation = focus.outside_click_generation().unwrap();
        assert!(focus.can_hide_after_outside_click(generation));
        focus.closed();
        assert!(!focus.can_hide_after_outside_click(generation));
    }

    #[test]
    fn reopening_restores_outside_click_dismissal() {
        let mut focus = PopoverFocus::default();
        focus.opened();
        let old_generation = focus.outside_click_generation().unwrap();
        focus.closed();
        focus.opened();
        assert!(!focus.can_hide_after_outside_click(old_generation));
        assert!(focus.outside_click_generation().is_some());
    }

    #[test]
    fn menu_bar_clicks_toggle_the_popover() {
        let mut focus = PopoverFocus::default();
        assert!(!focus.is_open);
        focus.opened();
        focus.tray_clicked();
        assert!(focus.is_open);
        assert!(focus.outside_click_generation().is_none());
        focus.closed();
        assert!(!focus.is_open);
    }

    #[test]
    fn tray_click_cancels_a_pending_outside_dismissal() {
        let mut focus = PopoverFocus::default();
        focus.opened();
        let pending = focus.outside_click_generation().unwrap();
        focus.tray_clicked();
        assert!(!focus.can_hide_after_outside_click(pending));
    }

    #[test]
    fn folder_dialog_keeps_popover_open_during_outside_clicks() {
        let mut focus = PopoverFocus::default();
        focus.opened();
        let pending = focus.outside_click_generation().unwrap();
        focus.begin_dialog();
        assert!(focus.outside_click_generation().is_none());
        assert!(!focus.can_hide_after_outside_click(pending));
        assert!(focus.end_dialog());
        assert!(focus.is_open);
        assert!(focus.outside_click_generation().is_some());
        assert!(!focus.end_dialog());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn popover_stays_on_an_external_display_to_the_right() {
        let external = PopoverFrame {
            x: 1440.0,
            y: -100.0,
            width: 1920.0,
            height: 1080.0,
        };
        let frame = popover_frame(3300.0, external, external);
        assert_eq!(frame.width, 420.0);
        assert_eq!(frame.height, 660.0);
        assert!(frame.x >= external.x + 12.0);
        assert!(frame.x + frame.width <= external.x + external.width - 12.0);
        assert!(frame.y >= external.y + 12.0);
        assert!(frame.y + frame.height <= external.y + external.height - 28.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn popover_stays_on_an_external_display_to_the_left() {
        let external = PopoverFrame {
            x: -1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let frame = popover_frame(-1880.0, external, external);
        assert_eq!(frame.x, -1908.0);
        assert!(frame.x + frame.width < 0.0);
        assert!(frame.y >= 12.0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn popover_shrinks_to_fit_a_short_display() {
        let short = PopoverFrame {
            x: 1440.0,
            y: 0.0,
            width: 600.0,
            height: 400.0,
        };
        let frame = popover_frame(2000.0, short, short);
        assert_eq!(frame.height, 360.0);
        assert_eq!(frame.y, 12.0);
        assert!(frame.x + frame.width <= 2028.0);
    }
}
