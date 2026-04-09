#![windows_subsystem = "windows"] // Prevent command prompt from appearing

mod capture;
mod preprocessing;
mod ocr;
mod clipboard;
mod notification;

use eframe::egui;
use global_hotkey::{
    hotkey::{Code, Modifiers, HotKey},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use std::thread;

pub struct ScreenOcrApp {
    tray_icon: Option<TrayIcon>,
    hotkey_manager: GlobalHotKeyManager,
    overlay_visible: bool,
    exit: bool,
    
    // Selection state
    start_pos: Option<egui::Pos2>,
    current_pos: Option<egui::Pos2>,
    
    // Position of the overlay spanning all monitors
    window_offset_x: i32,
    window_offset_y: i32,
}

impl ScreenOcrApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let hotkey_manager = GlobalHotKeyManager::new().unwrap();
        
        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyX);
        hotkey_manager.register(hotkey).unwrap();

        // Background thread to constantly wake up eframe.
        // Because the window is hidden, eframe goes to sleep and stops listening to hotkey/tray events.
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            loop {
                // Also check for Exit just in case winit event loop is hard-blocked
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    if event.id() == &tray_icon::menu::MenuId::new("exit") {
                        std::process::exit(0);
                    }
                }
                ctx.request_repaint();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        Self {
            tray_icon: None,
            hotkey_manager,
            overlay_visible: false,
            exit: false,
            start_pos: None,
            current_pos: None,
            window_offset_x: 0,
            window_offset_y: 0,
        }
    }
}

impl eframe::App for ScreenOcrApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Essential to make the main window transparent!
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
                let rgba = image.into_raw();
                (rgba, width, height)
            };
            let icon = Icon::from_rgba(icon_rgba, icon_width, icon_height).unwrap();

            let tray_icon = TrayIconBuilder::new()
                .with_menu(Box::new(tray_menu))
                .with_tooltip("Screen OCR")
                .with_icon(icon)
                .build()
                .unwrap();

            self.tray_icon = Some(tray_icon);
        }

        // Handle Tray Events
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            println!("Tray event: {:?}", event);
        }



        // Handle Hotkey Events
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == global_hotkey::HotKeyState::Released {
                self.overlay_visible = !self.overlay_visible;
                
                if self.overlay_visible {
                    if let Ok(monitors) = xcap::Monitor::all() {
                        let mut min_x = i32::MAX;
                        let mut min_y = i32::MAX;
                        let mut max_x = i32::MIN;
                        let mut max_y = i32::MIN;

                        for m in monitors {
                            min_x = min_x.min(m.x());
                            min_y = min_y.min(m.y());
                            max_x = max_x.max(m.x() + m.width() as i32);
                            max_y = max_y.max(m.y() + m.height() as i32);
                        }

                        if min_x < i32::MAX {
                            self.window_offset_x = min_x;
                            self.window_offset_y = min_y;

                            let scale = ctx.pixels_per_point();
                            let logical_x = min_x as f32 / scale;
                            let logical_y = min_y as f32 / scale;
                            let logical_w = (max_x - min_x) as f32 / scale;
                            let logical_h = (max_y - min_y) as f32 / scale;

                            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(logical_x, logical_y)));
                            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(logical_w, logical_h)));
                        }
                    }
                }

                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.overlay_visible));
                self.start_pos = None;
                self.current_pos = None;
            }
        }

        // Allow user to cancel overlay with Escape or Right Click
        if self.overlay_visible {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_pressed()) {
                self.overlay_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.start_pos = None;
                self.current_pos = None;
            }
        }



        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        if self.overlay_visible {
            let screen_rect = ctx.screen_rect();
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("overlay")));

            match (self.start_pos, self.current_pos) {
                (Some(p1), Some(p2)) => {
                    let selection_rect = egui::Rect::from_two_pos(p1, p2);
                    let color = egui::Color32::from_black_alpha(128); // Semi-transparent black

                    // Top
                    painter.rect_filled(
                        egui::Rect::from_min_max(screen_rect.min, egui::Pos2::new(screen_rect.max.x, selection_rect.min.y)),
                        0.0,
                        color,
                    );
                    // Bottom
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::Pos2::new(screen_rect.min.x, selection_rect.max.y), screen_rect.max),
                        0.0,
                        color,
                    );
                    // Left
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::Pos2::new(screen_rect.min.x, selection_rect.min.y), egui::Pos2::new(selection_rect.min.x, selection_rect.max.y)),
                        0.0,
                        color,
                    );
                    // Right
                    painter.rect_filled(
                        egui::Rect::from_min_max(egui::Pos2::new(selection_rect.max.x, selection_rect.min.y), egui::Pos2::new(screen_rect.max.x, selection_rect.max.y)),
                        0.0,
                        color,
                    );
                    
                    // Draw a thin red border around selection
                    painter.rect_stroke(
                        selection_rect,
                        0.0,
                        egui::Stroke::new(1.0, egui::Color32::RED),
                        egui::StrokeKind::Inside,
                    );
                }
                _ => {
                    // Full screen dark mode before selection starts
                    painter.rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(128));
                }
            }

            let pointer = &ctx.input(|i| i.pointer.clone());
            
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

                self.overlay_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                
                let scale_factor = ctx.pixels_per_point();
                let ox = self.window_offset_x;
                let oy = self.window_offset_y;
                thread::spawn(move || {
                    if let Ok(image) = capture::capture_region(selection_rect, scale_factor, ox, oy) {
                        let processed = preprocessing::preprocess(image);
                        if let Ok(text) = ocr::recognize(&processed, "eng+srp_latn", 6) {
                            if !text.is_empty() {
                                let _ = clipboard::copy_to_clipboard(&text);
                                notification::notify_success();
                            }
                        }
                    }
                });

                self.start_pos = None;
                self.current_pos = None;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_visible(false)
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
