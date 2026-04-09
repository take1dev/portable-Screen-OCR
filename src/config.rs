use serde::{Deserialize, Serialize};
use global_hotkey::hotkey::{Code, Modifiers};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConfig {
    pub modifier_ctrl: bool,
    pub modifier_shift: bool,
    pub modifier_alt: bool,
    pub modifier_meta: bool,
    pub key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            modifier_ctrl: true,
            modifier_shift: true,
            modifier_alt: false,
            modifier_meta: false,
            key: "KeyX".to_string(), // Default string representation of Code::KeyX
        }
    }
}

impl AppConfig {
    fn get_config_path() -> PathBuf {
        let mut path = std::env::current_exe().unwrap_or_default();
        path.pop();
        path.push("screen_ocr_config.toml");
        path
    }

    pub fn load() -> Self {
        let path = Self::get_config_path();
        if let Ok(contents) = fs::read_to_string(&path) {
            toml::from_str(&contents).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::get_config_path();
        if let Ok(contents) = toml::to_string(self) {
            let _ = fs::write(path, contents);
        }
    }

    pub fn get_modifiers(&self) -> Option<Modifiers> {
        let mut mods = Modifiers::empty();
        if self.modifier_ctrl { mods.insert(Modifiers::CONTROL); }
        if self.modifier_shift { mods.insert(Modifiers::SHIFT); }
        if self.modifier_alt { mods.insert(Modifiers::ALT); }
        if self.modifier_meta { mods.insert(Modifiers::META); }
        
        if mods.is_empty() { None } else { Some(mods) }
    }

    pub fn get_code(&self) -> Code {
        // Simple mapping, can be expanded. Using standard match for string to Code.
        match self.key.as_str() {
            "KeyA" => Code::KeyA, "KeyB" => Code::KeyB, "KeyC" => Code::KeyC,
            "KeyD" => Code::KeyD, "KeyE" => Code::KeyE, "KeyF" => Code::KeyF,
            "KeyG" => Code::KeyG, "KeyH" => Code::KeyH, "KeyI" => Code::KeyI,
            "KeyJ" => Code::KeyJ, "KeyK" => Code::KeyK, "KeyL" => Code::KeyL,
            "KeyM" => Code::KeyM, "KeyN" => Code::KeyN, "KeyO" => Code::KeyO,
            "KeyP" => Code::KeyP, "KeyQ" => Code::KeyQ, "KeyR" => Code::KeyR,
            "KeyS" => Code::KeyS, "KeyT" => Code::KeyT, "KeyU" => Code::KeyU,
            "KeyV" => Code::KeyV, "KeyW" => Code::KeyW, "KeyX" => Code::KeyX,
            "KeyY" => Code::KeyY, "KeyZ" => Code::KeyZ,
            "Digit0" => Code::Digit0, "Digit1" => Code::Digit1, "Digit2" => Code::Digit2,
            "Digit3" => Code::Digit3, "Digit4" => Code::Digit4, "Digit5" => Code::Digit5,
            "Digit6" => Code::Digit6, "Digit7" => Code::Digit7, "Digit8" => Code::Digit8,
            "Digit9" => Code::Digit9,
            "F1" => Code::F1, "F2" => Code::F2, "F3" => Code::F3, "F4" => Code::F4,
            "F5" => Code::F5, "F6" => Code::F6, "F7" => Code::F7, "F8" => Code::F8,
            "F9" => Code::F9, "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
            _ => Code::KeyX,
        }
    }
}
