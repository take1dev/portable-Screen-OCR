#![windows_subsystem = "windows"] // Prevent command prompt from appearing

mod capture;
mod preprocessing;
mod ocr;
mod clipboard;
mod notification;
mod config;

use eframe::egui;
use global_hotkey::{
    hotkey::HotKey,
    GlobalHotKeyManager,
};
use tray_icon::{
    menu::{Menu, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Instant;
use image::RgbaImage;

// ─── Win32 is no longer needed ────────────────────────────────────────────────

// ─── App state ────────────────────────────────────────────────────────────────

pub struct ScreenOcrApp {
    // Infrastructure
    tray_icon: Option<TrayIcon>,
    _hotkey_manager: GlobalHotKeyManager,
    overlay_active: Arc<AtomicBool>,
    show_settings: Arc<AtomicBool>,
    was_settings_open: bool,
    was_active: bool,
    initialized: bool,
    config: config::AppConfig,

    // ── "Capture-first" state ──
    // When the hotkey fires we capture the entire virtual desktop into this
    // buffer, then display it as a frozen image inside the overlay window.
    // The user draws a selection rectangle on top of the frozen image.
    // On release we crop from the buffer — no second capture needed, no
    // timing race, and the overlay itself is never captured.
    frozen_screenshot: Option<RgbaImage>,
    frozen_texture: Option<egui::TextureHandle>,
    frozen_offset_x: i32,
    frozen_offset_y: i32,
    frozen_width: u32,
    frozen_height: u32,

    // Selection state
    start_pos: Option<egui::Pos2>,
    current_pos: Option<egui::Pos2>,

    // 30-second safety timeout
    overlay_activated_at: Option<Instant>,
}

// ScreenOcrApp works safely across threads
unsafe impl Send for ScreenOcrApp {}
unsafe impl Sync for ScreenOcrApp {}

impl ScreenOcrApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let config = config::AppConfig::load();
        let hotkey_manager = GlobalHotKeyManager::new().unwrap();
        let hotkey = HotKey::new(config.get_modifiers(), config.get_code());
        if let Err(e) = hotkey_manager.register(hotkey) {
            notification::notify_error(&format!("Failed to register hotkey. Might be mapped by another app! ({:?})", e));
        }

        let overlay_active = Arc::new(AtomicBool::new(false));
        let show_settings = Arc::new(AtomicBool::new(false));

        // Background thread: poll hotkey + tray menu events
        {
            let overlay = overlay_active.clone();
            let settings = show_settings.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || loop {
                if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
                    if event.id() == &tray_icon::menu::MenuId::new("exit") {
                        std::process::exit(0);
                    } else if event.id() == &tray_icon::menu::MenuId::new("change_shortcut") {
                        settings.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.request_repaint();
                    }
                }
                while let Ok(ev) = global_hotkey::GlobalHotKeyEvent::receiver().try_recv() {
                    if ev.state == global_hotkey::HotKeyState::Pressed
                        && !overlay.load(Ordering::Relaxed)
                    {
                        overlay.store(true, Ordering::Relaxed);
                        // Crucial: Winit suspends repaints for hidden windows!
                        // We MUST send a viewport command to wake up the main thread's event loop.
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.request_repaint();
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(16));
            });
        }

        Self {
            tray_icon: None,
            _hotkey_manager: hotkey_manager,
            overlay_active,
            show_settings,
            was_settings_open: false,
            was_active: false,
            initialized: false,
            config,
            frozen_screenshot: None,
            frozen_texture: None,
            frozen_offset_x: 0,
            frozen_offset_y: 0,
            frozen_width: 0,
            frozen_height: 0,
            start_pos: None,
            current_pos: None,
            overlay_activated_at: None,
        }
    }

    // ── Window helpers ────────────────────────────────────────────────────

    fn hide_overlay(&mut self, ctx: &egui::Context) {
        // Instead of making the window completely invisible (which causes Winit
        // to aggressively suspend the event loop on Windows), we teleport it
        // off-screen. This guarantees it can always wake up to custom events
        // and request_repaint calls from background threads.
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(100.0, 100.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(-20000.0, -20000.0)));
        self.frozen_texture = None;
    }

    fn show_overlay(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // ── Dismiss helper (used in cancel / timeout / after selection) ───────

    fn dismiss(&mut self, ctx: &egui::Context) {
        self.overlay_active.store(false, Ordering::Relaxed);
        self.start_pos = None;
        self.current_pos = None;
        self.was_active = false;
        self.overlay_activated_at = None;
        self.frozen_screenshot = None;
        self.hide_overlay(ctx);
    }

    // ── Capture the entire virtual desktop into `frozen_screenshot` ──────

    fn capture_desktop(&mut self) {
        if let Ok(monitors) = xcap::Monitor::all() {
            // Compute the virtual desktop bounding box
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

            let total_w = (max_x - min_x) as u32;
            let total_h = (max_y - min_y) as u32;

            // Allocate a canvas covering the entire virtual desktop
            let mut canvas = RgbaImage::new(total_w, total_h);

            for monitor in &monitors {
                if let Ok(shot) = monitor.capture_image() {
                    let dx = (monitor.x() - min_x) as u32;
                    let dy = (monitor.y() - min_y) as u32;

                    // Convert xcap image to image::RgbaImage
                    let src = RgbaImage::from_raw(
                        shot.width(), shot.height(), shot.into_raw(),
                    );
                    if let Some(src) = src {
                        image::imageops::overlay(&mut canvas, &src, dx as i64, dy as i64);
                    }
                }
            }

            self.frozen_offset_x = min_x;
            self.frozen_offset_y = min_y;
            self.frozen_width = total_w;
            self.frozen_height = total_h;
            self.frozen_screenshot = Some(canvas);
        }
    }
}

// ─── eframe::App ─────────────────────────────────────────────────────────────

impl eframe::App for ScreenOcrApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // ── First-frame init ──────────────────────────────────────────────
        if !self.initialized {
            self.initialized = true;

            let tray_menu = Menu::new();
            let change_i = MenuItem::with_id("change_shortcut", "Change Shortcut", true, None);
            let quit_i   = MenuItem::with_id("exit", "Exit", true, None);
            tray_menu.append_items(&[&change_i, &quit_i]).unwrap();

            let icon_bytes = include_bytes!("../assets/icon.png");
            let (icon_rgba, icon_w, icon_h) = {
                let img = image::load_from_memory(icon_bytes)
                    .expect("Failed to load icon")
                    .into_rgba8();
                let (w, h) = img.dimensions();
                (img.into_raw(), w, h)
            };
            let icon = Icon::from_rgba(icon_rgba, icon_w, icon_h).unwrap();

            self.tray_icon = Some(
                TrayIconBuilder::new()
                    .with_menu(Box::new(tray_menu))
                    .with_tooltip("Screen OCR")
                    .with_icon(icon)
                    .build()
                    .unwrap(),
            );

            // Hide immediately — window must not be visible at idle
            self.hide_overlay(ctx);
            return;
        }

        // ── Settings panel ────────────────────────────────────────────────
        let is_settings = self.show_settings.load(Ordering::Relaxed);
        if is_settings {
            if !self.was_settings_open {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(400.0, 250.0)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(100.0, 100.0)));
                self.show_overlay(ctx);
                self.was_settings_open = true;
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Global Hotkey Configuration");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.config.modifier_ctrl, "Ctrl");
                    ui.checkbox(&mut self.config.modifier_shift, "Shift");
                    ui.checkbox(&mut self.config.modifier_alt, "Alt");
                    ui.checkbox(&mut self.config.modifier_meta, "Win");
                });
                egui::ComboBox::from_label("Key")
                    .selected_text(&self.config.key)
                    .show_ui(ui, |ui| {
                        let keys = [
                            "KeyA","KeyB","KeyC","KeyD","KeyE","KeyF","KeyG","KeyH","KeyI","KeyJ",
                            "KeyK","KeyL","KeyM","KeyN","KeyO","KeyP","KeyQ","KeyR","KeyS","KeyT",
                            "KeyU","KeyV","KeyW","KeyX","KeyY","KeyZ",
                            "Digit0","Digit1","Digit2","Digit3","Digit4","Digit5","Digit6","Digit7","Digit8","Digit9",
                            "F1","F2","F3","F4","F5","F6","F7","F8","F9","F10","F11","F12",
                        ];
                        for k in keys {
                            ui.selectable_value(&mut self.config.key, k.to_string(), k);
                        }
                    });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.config.save();
                        let _ = self._hotkey_manager.unregister_all(&[]);
                        let hk = HotKey::new(self.config.get_modifiers(), self.config.get_code());
                        if let Err(e) = self._hotkey_manager.register(hk) {
                            notification::notify_error(&format!("Failed to register hotkey: {:?}", e));
                        }
                        self.show_settings.store(false, Ordering::Relaxed);
                    }
                    if ui.button("Cancel").clicked() {
                        self.config = config::AppConfig::load();
                        self.show_settings.store(false, Ordering::Relaxed);
                    }
                });
            });
            return;
        } else if self.was_settings_open {
            self.hide_overlay(ctx);
            self.was_settings_open = false;
        }

        // ── Overlay activation transition ─────────────────────────────────
        let is_active = self.overlay_active.load(Ordering::Relaxed);

        if is_active && !self.was_active {
            // 1. Capture the desktop FIRST (before showing any overlay)
            self.capture_desktop();
            self.was_active = true;
            self.overlay_activated_at = Some(Instant::now());

            // 2. Resize the eframe window to span the virtual desktop
            let scale = ctx.pixels_per_point();
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                self.frozen_offset_x as f32 / scale,
                self.frozen_offset_y as f32 / scale,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                self.frozen_width as f32 / scale,
                self.frozen_height as f32 / scale,
            )));

            // 3. Now show the window
            self.show_overlay(ctx);
        } else if !is_active && self.was_active {
            self.was_active = false;
            self.hide_overlay(ctx);
        }

        if !is_active {
            return;
        }

        // ── Auto-dismiss after 30 seconds ─────────────────────────────────
        if let Some(t) = self.overlay_activated_at {
            if t.elapsed().as_secs() >= 30 {
                self.dismiss(ctx);
                return;
            }
        }

        // ── Upload frozen screenshot as a texture (once per activation) ───
        if self.frozen_texture.is_none() {
            if let Some(ref img) = self.frozen_screenshot {
                let size = [img.width() as usize, img.height() as usize];
                let pixels = img.as_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, pixels);
                self.frozen_texture = Some(ctx.load_texture(
                    "frozen_desktop",
                    color_image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }

        // ── Draw ──────────────────────────────────────────────────────────
        let screen_rect = ctx.screen_rect();

        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("overlay"),
        ));

        // Paint the frozen screenshot as the background so the user sees a
        // "frozen" desktop rather than a transparent/black surface.
        if let Some(ref tex) = self.frozen_texture {
            painter.image(
                tex.id(),
                screen_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }

        // Cancel on Escape / right-click
        if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_pressed()) {
            self.dismiss(ctx);
            return;
        }

        // Dim + selection rectangle
        let dim = egui::Color32::from_black_alpha(120);
        match (self.start_pos, self.current_pos) {
            (Some(p1), Some(p2)) => {
                let sel = egui::Rect::from_two_pos(p1, p2);
                // Top
                painter.rect_filled(
                    egui::Rect::from_min_max(screen_rect.min, egui::pos2(screen_rect.max.x, sel.min.y)),
                    0.0, dim,
                );
                // Bottom
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, sel.max.y), screen_rect.max),
                    0.0, dim,
                );
                // Left
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(screen_rect.min.x, sel.min.y), egui::pos2(sel.min.x, sel.max.y)),
                    0.0, dim,
                );
                // Right
                painter.rect_filled(
                    egui::Rect::from_min_max(egui::pos2(sel.max.x, sel.min.y), egui::pos2(screen_rect.max.x, sel.max.y)),
                    0.0, dim,
                );
                painter.rect_stroke(
                    sel, 0.0,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 180, 255)),
                    egui::StrokeKind::Outside,
                );
            }
            _ => {
                painter.rect_filled(screen_rect, 0.0, dim);
            }
        }

        // ── Mouse handling ────────────────────────────────────────────────
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

            // The selection is in logical (egui) coordinates.
            // Convert to physical pixels inside the frozen_screenshot buffer.
            let scale = ctx.pixels_per_point();
            let sel = egui::Rect::from_two_pos(p1, p2);
            let px = (sel.min.x * scale) as u32;
            let py = (sel.min.y * scale) as u32;
            let pw = ((sel.width() * scale) as u32).max(1);
            let ph = ((sel.height() * scale) as u32).max(1);

            // Take ownership of the frozen screenshot for the OCR thread
            let screenshot = self.frozen_screenshot.take();

            // Dismiss the overlay immediately
            self.dismiss(ctx);

            // OCR in a background thread
            std::thread::spawn(move || {
                if let Some(full) = screenshot {
                    // Clamp to image bounds
                    let img_w = full.width();
                    let img_h = full.height();
                    let cx = px.min(img_w.saturating_sub(1));
                    let cy = py.min(img_h.saturating_sub(1));
                    let cw = pw.min(img_w - cx);
                    let ch = ph.min(img_h - cy);

                    if cw == 0 || ch == 0 { return; }

                    let cropped = image::imageops::crop_imm(&full, cx, cy, cw, ch).to_image();
                    let dynamic = image::DynamicImage::ImageRgba8(cropped);
                    let processed = preprocessing::preprocess(dynamic);

                    match ocr::recognize(&processed, "eng+srp_latn", 6) {
                        Ok(text) if !text.trim().is_empty() => {
                            let _ = clipboard::copy_to_clipboard(&text);
                            notification::notify_success();
                        }
                        Ok(_) => {} // empty text, nothing to copy
                        Err(e) => {
                            eprintln!("OCR error: {e}");
                        }
                    }
                }
            });
        }
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn ensure_single_instance() -> Result<(), &'static str> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn CreateMutexW(
            lpMutexAttributes: *mut std::ffi::c_void,
            bInitialOwner: i32,
            lpName: *const u16,
        ) -> *mut std::ffi::c_void;
        fn GetLastError() -> u32;
    }

    let mut name: Vec<u16> = OsStr::new("Global\\PortableScreenOcrSingleInstanceMutex")
        .encode_wide()
        .collect();
    name.push(0);

    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        if handle.is_null() {
            return Err("Failed to create single instance mutex");
        }
        if GetLastError() == 183 { // ERROR_ALREADY_EXISTS
            return Err("Another instance is already running.");
        }
    }
    Ok(())
}

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    if ensure_single_instance().is_err() {
        // Silently exit if already running
        std::process::exit(0);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(100.0, 100.0))
            .with_position(egui::pos2(-20000.0, -20000.0)) // Start immediately off-screen!
            .with_decorations(false)
            .with_transparent(false) // No DWM transparency needed, image is opaque!
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
