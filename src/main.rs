#![windows_subsystem = "windows"] // Prevent command prompt from appearing

mod capture;
mod preprocessing;
mod ocr;
mod clipboard;
mod notification;

use eframe::egui;
use global_hotkey::{
    hotkey::{Code, Modifiers, HotKey},
    GlobalHotKeyManager,
};
use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

// Win32 imports for click-through toggling
#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::c_void;
    pub type HWND = *mut c_void;
    pub type LONG = i32;
    pub const GWL_EXSTYLE: i32 = -20;
    pub const WS_EX_TRANSPARENT: u32 = 0x00000020;
    pub const WS_EX_LAYERED: u32 = 0x00080000;
    pub const WS_EX_NOACTIVATE: u32 = 0x08000000;

    extern "system" {
        pub fn GetWindowLongW(hwnd: HWND, n_index: i32) -> LONG;
        pub fn SetWindowLongW(hwnd: HWND, n_index: i32, dw_new_long: LONG) -> LONG;
        pub fn SetForegroundWindow(hwnd: HWND) -> i32;
    }

    pub unsafe fn set_click_through(hwnd: HWND, enabled: bool) {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let new_style = if enabled {
            style | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE
        } else {
            (style & !(WS_EX_TRANSPARENT | WS_EX_NOACTIVATE)) | WS_EX_LAYERED
        };
        SetWindowLongW(hwnd, GWL_EXSTYLE, new_style as i32);
        if !enabled {
            SetForegroundWindow(hwnd);
        }
    }
}

pub struct ScreenOcrApp {
    tray_icon: Option<TrayIcon>,
    _hotkey_manager: GlobalHotKeyManager,
    overlay_active: Arc<AtomicBool>,
    hwnd: Option<*mut std::ffi::c_void>,
    
    // Selection state
    start_pos: Option<egui::Pos2>,
    current_pos: Option<egui::Pos2>,
    
    // Position offset spanning all monitors
    window_offset_x: i32,
    window_offset_y: i32,
}

// SAFETY: we only access hwnd from the main thread
unsafe impl Send for ScreenOcrApp {}
unsafe impl Sync for ScreenOcrApp {}

impl ScreenOcrApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let hotkey_manager = GlobalHotKeyManager::new().unwrap();
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX);
        hotkey_manager.register(hotkey).unwrap();

        let overlay_active = Arc::new(AtomicBool::new(false));
        let overlay_active_clone = overlay_active.clone();

        // Background thread: handles hotkey + tray exit events
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            loop {
                // Handle tray exit
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    if event.id() == &tray_icon::menu::MenuId::new("exit") {
                        std::process::exit(0);
                    }
                }

                // Handle hotkey - only toggle on if overlay is off
                while let Ok(event) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
                    if event.state == global_hotkey::HotKeyState::Released {
                        // Only show if not already active
                        if !overlay_active_clone.load(Ordering::Relaxed) {
                            overlay_active_clone.store(true, Ordering::Relaxed);
                            ctx.request_repaint();
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        });

        Self {
            tray_icon: None,
            _hotkey_manager: hotkey_manager,
            overlay_active,
            hwnd: None,
            start_pos: None,
            current_pos: None,
            window_offset_x: 0,
            window_offset_y: 0,
        }
    }
}

impl eframe::App for ScreenOcrApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Always fully transparent background
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Initialize tray icon on first frame
        if self.tray_icon.is_none() {
            let tray_menu = Menu::new();
            let quit_i = MenuItem::with_id("exit", "Exit", true, None);
            tray_menu.append_items(&[&quit_i]).unwrap();

            let icon_bytes = include_bytes!("../assets/icon.png");
            let (icon_rgba, icon_width, icon_height) = {
                let image = image::load_from_memory(icon_bytes)
                    .expect("Failed to load icon")
                    .into_rgba8();
                let (width, height) = image.dimensions();
                (image.into_raw(), width, height)
            };
            let icon = Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();

            self.tray_icon = Some(
                TrayIconBuilder::new()
                    .with_menu(Box::new(tray_menu))
                    .with_tooltip("Screen OCR")
                    .with_icon(icon)
                    .build()
                    .unwrap(),
            );

            // Get and store the HWND for click-through toggling
            #[cfg(target_os = "windows")]
            {
                use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                if let Ok(wh) = frame.window_handle() {
                    if let RawWindowHandle::Win32(h) = wh.window_handle().unwrap().as_raw() {
                        self.hwnd = Some(h.hwnd.get() as *mut _);
                    }
                }

                // Start fully click-through (invisible to input)
                if let Some(hwnd) = self.hwnd {
                    unsafe { win32::set_click_through(hwnd, true); }
                }
            }
        }

        let is_active = self.overlay_active.load(Ordering::Relaxed);

        // Transition: became active
        if is_active && self.start_pos.is_none() && self.current_pos.is_none() {
            // Resize to span all monitors if not already done
            if let Ok(monitors) = xcap::Monitor::all() {
                let mut min_x = i32::MAX;
                let mut min_y = i32::MAX;
                let mut max_x = i32::MIN;
                let mut max_y = i32::MIN;

                for m in &monitors {
                    min_x = min_x.min(m.x());
                    min_y = min_y.min(m.y());
                    max_x = max_x.max(m.x() + m.width() as i32);
                    max_y = max_y.max(m.y() + m.height() as i32);
                }

                if min_x < i32::MAX {
                    self.window_offset_x = min_x;
                    self.window_offset_y = min_y;
                    let scale = ctx.pixels_per_point();
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                        min_x as f32 / scale,
                        min_y as f32 / scale,
                    )));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        (max_x - min_x) as f32 / scale,
                        (max_y - min_y) as f32 / scale,
                    )));
                }
            }

            // Disable click-through so we can receive mouse input
            #[cfg(target_os = "windows")]
            if let Some(hwnd) = self.hwnd {
                unsafe { win32::set_click_through(hwnd, false); }
            }
        }

        if is_active {
            let screen_rect = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("overlay"),
            ));

            // Cancel on Escape or right-click
            if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_pressed()) {
                self.overlay_active.store(false, Ordering::Relaxed);
                self.start_pos = None;
                self.current_pos = None;
                #[cfg(target_os = "windows")]
                if let Some(hwnd) = self.hwnd {
                    unsafe { win32::set_click_through(hwnd, true); }
                }
                ctx.request_repaint();
                return;
            }

            // Draw dim overlay
            match (self.start_pos, self.current_pos) {
                (Some(p1), Some(p2)) => {
                    let selection_rect = egui::Rect::from_two_pos(p1, p2);
                    let dim = egui::Color32::from_black_alpha(160);
                    // Top
                    painter.rect_filled(
                        egui::Rect::from_min_max(screen_rect.min, egui::pos2(screen_rect.max.x, selection_rect.min.y)),
                        0.0, dim,
                    );
                    // Bottom
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, selection_rect.max.y), screen_rect.max),
                        0.0, dim,
                    );
                    // Left
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, selection_rect.min.y), egui::pos2(selection_rect.min.x, selection_rect.max.y)),
                        0.0, dim,
                    );
                    // Right
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::pos2(selection_rect.max.x, selection_rect.min.y), egui::pos2(screen_rect.max.x, selection_rect.max.y)),
                        0.0, dim,
                    );
                    painter.rect_stroke(
                        selection_rect,
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 180, 255)),
                        egui::StrokeKind::Outside,
                    );
                }
                _ => {
                    painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(160));
                }
            }

            // Handle mouse input
            let pointer = ctx.input(|i| i.pointer.clone());
            if pointer.primary_pressed() {
                self.start_pos = pointer.interact_pos();
            }
            if pointer.primary_down() {
                self.current_pos = pointer.interact_pos();
            }
            if pointer.primary_released() && self.start_pos.is_some() && self.current_pos.is_some() {
                let p1 = self.start_pos.unwrap();
                let p2 = self.current_pos.unwrap();
                let selection_rect = egui::Rect::from_two_pos(p1, p2);

                self.overlay_active.store(false, Ordering::Relaxed);
                self.start_pos = None;
                self.current_pos = None;

                // Re-enable click-through
                #[cfg(target_os = "windows")]
                if let Some(hwnd) = self.hwnd {
                    unsafe { win32::set_click_through(hwnd, true); }
                }

                let scale_factor = ctx.pixels_per_point();
                let ox = self.window_offset_x;
                let oy = self.window_offset_y;
                std::thread::spawn(move || {
                    if let Ok(image) = capture::capture_region(selection_rect, scale_factor, ox, oy) {
                        let processed = preprocessing::preprocess(image);
                        if let Ok(text) = ocr::recognize(&processed, "eng+srp_latn", 6) {
                            if !text.trim().is_empty() {
                                let _ = clipboard::copy_to_clipboard(&text);
                                notification::notify_success();
                            }
                        }
                    }
                });
            }

            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result<()> {
    // Span all monitors
    let (total_x, total_y, total_w, total_h) = if let Ok(monitors) = xcap::Monitor::all() {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        for m in &monitors {
            min_x = min_x.min(m.x());
            min_y = min_y.min(m.y());
            max_x = max_x.max(m.x() + m.width() as i32);
            max_y = max_y.max(m.y() + m.height() as i32);
        }
        if min_x < i32::MAX {
            (min_x as f32, min_y as f32, (max_x - min_x) as f32, (max_y - min_y) as f32)
        } else {
            (0.0, 0.0, 1920.0, 1080.0)
        }
    } else {
        (0.0, 0.0, 1920.0, 1080.0)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_position(egui::pos2(total_x, total_y))
            .with_inner_size(egui::vec2(total_w, total_h))
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false),
        ..Default::default()
    };

    eframe::run_native(
        "Screen OCR",
        options,
        Box::new(|cc| Ok(Box::new(ScreenOcrApp::new(cc)))),
    )
}
