use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Доступные keybind для отправки в Discord
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Keybind {
    #[serde(rename = "F13")]
    F13,
    #[serde(rename = "F14")]
    F14,
    #[serde(rename = "F15")]
    F15,
    #[serde(rename = "F16")]
    F16,
    #[serde(rename = "F17")]
    F17,
    #[serde(rename = "F18")]
    F18,
    #[serde(rename = "F19")]
    F19,
    #[serde(rename = "F20")]
    F20,
    #[serde(rename = "MediaPlayPause")]
    MediaPlayPause,
}

impl Keybind {
    /// Все доступные keybind
    pub fn all() -> &'static [Keybind] {
        &[
            Keybind::F13,
            Keybind::F14,
            Keybind::F15,
            Keybind::F16,
            Keybind::F17,
            Keybind::F18,
            Keybind::F19,
            Keybind::F20,
            Keybind::MediaPlayPause,
        ]
    }

    /// Строковое представление keybind
    pub fn as_str(&self) -> &'static str {
        match self {
            Keybind::F13 => "F13",
            Keybind::F14 => "F14",
            Keybind::F15 => "F15",
            Keybind::F16 => "F16",
            Keybind::F17 => "F17",
            Keybind::F18 => "F18",
            Keybind::F19 => "F19",
            Keybind::F20 => "F20",
            Keybind::MediaPlayPause => "MediaPlayPause",
        }
    }
}

impl Default for Keybind {
    fn default() -> Self {
        Keybind::F20
    }
}

/// Главная конфигурация приложения. Сохраняется в tapmute.toml рядом с exe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub keybind: Keybind,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_double_tap_ms")]
    pub double_tap_ms: u64,
    #[serde(default)]
    pub start_with_os: bool,
    #[serde(default = "default_compact_mode")]
    pub compact_mode: bool,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_enabled() -> bool {
    true
}

fn default_double_tap_ms() -> u64 {
    400
}

fn default_compact_mode() -> bool {
    true
}

fn default_language() -> String {
    "ru".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keybind: Keybind::default(),
            enabled: default_enabled(),
            double_tap_ms: default_double_tap_ms(),
            start_with_os: false,
            compact_mode: default_compact_mode(),
            language: default_language(),
        }
    }
}

impl Config {
    /// Загружает конфиг из tapmute.toml рядом с exe. Если файла нет — создаёт default.
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        log::error!("[Config] Ошибка парсинга TOML: {}. Используем значения по умолчанию.", e);
                        Config::default()
                    }
                },
                Err(e) => {
                    log::error!("[Config] Ошибка чтения файла: {}. Используем значения по умолчанию.", e);
                    Config::default()
                }
            }
        } else {
            let default = Config::default();
            if let Err(e) = default.save() {
                log::error!("[Config] Ошибка создания default config: {}", e);
            } else {
                log::info!("[Config] Создан default config: {:?}", path);
            }
            default
        }
    }

    /// Сохраняет конфиг в tapmute.toml
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        log::info!("[Config] Сохранено в {:?}", path);
        Ok(())
    }

    /// Путь к файлу конфигурации: рядом с exe
    pub fn path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("tapmute.toml")))
            .unwrap_or_else(|| PathBuf::from("tapmute.toml"))
    }
}
