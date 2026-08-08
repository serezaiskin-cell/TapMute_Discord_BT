use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use lazy_static::lazy_static;

use crate::config::Keybind;

/// Обработчик отправки keybind для переключения мьюта в Discord.
/// Защищён от дребезга: минимальный интервал между отправками — 150 мс.
pub struct MuteHandler {
    enigo: Enigo,
    last_toggle: Option<Instant>,
    debounce_ms: u64,
    keybind_str: String,
}

impl MuteHandler {
    pub fn new() -> Self {
        let enigo = Enigo::new(&Settings::default()).unwrap_or_else(|e| {
            log::error!("[MuteHandler] Не удалось создать Enigo: {}", e);
            panic!("Enigo initialization failed");
        });
        Self {
            enigo,
            last_toggle: None,
            debounce_ms: 150,
            keybind_str: "F20".to_string(),
        }
    }

    /// Установить keybind, который будет отправляться при мьюте
    pub fn set_keybind(&mut self, keybind: &str) {
        self.keybind_str = keybind.to_string();
    }

    /// Отправляет keybind с защитой от дребезга (150 мс).
    /// Если с последней отправки прошло менее 150 мс — игнорирует вызов.
    pub fn do_mute(&mut self, keybind: &Keybind) {
        let now = Instant::now();
        if let Some(last) = self.last_toggle {
            if now.duration_since(last) < Duration::from_millis(self.debounce_ms) {
                log::debug!("[MuteHandler] Дребезг: пропускаем отправку");
                return;
            }
        }
        self.last_toggle = Some(now);

        log::info!("[MuteHandler] Отправка keybind: {:?}", keybind);

        let key = match keybind {
            Keybind::F13 => Key::F13,
            Keybind::F14 => Key::F14,
            Keybind::F15 => Key::F15,
            Keybind::F16 => Key::F16,
            Keybind::F17 => Key::F17,
            Keybind::F18 => Key::F18,
            Keybind::F19 => Key::F19,
            Keybind::F20 => Key::F20,
            Keybind::MediaPlayPause => Key::MediaPlayPause,
        };

        if let Err(e) = self.enigo.key(key, Direction::Click) {
            log::error!("[MuteHandler] Ошибка отправки клавиши: {}", e);
            return;
        }

        log::info!("[MuteHandler] Keybind {:?} отправлен успешно", keybind);
    }

    /// Тестовая отправка keybind (один раз, без дополнительных проверок)
    pub fn test_mute(&mut self, keybind: &Keybind) {
        log::info!("[MuteHandler] Тестовая отправка keybind: {:?}", keybind);
        self.do_mute(keybind);
    }
}

/// Глобальный экземпляр MuteHandler, доступный из любого потока
lazy_static! {
    pub static ref GLOBAL_MUTE_HANDLER: Arc<Mutex<MuteHandler>> =
        Arc::new(Mutex::new(MuteHandler::new()));
}
