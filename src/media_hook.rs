//! Глобальный перехват media-клавиши Play/Pause на Windows.
//!
//! Использует WH_KEYBOARD_LL (low-level keyboard hook) для перехвата
//! VK_MEDIA_PLAY_PAUSE (0xB3).
//!
//! Логика:
//! - Одиночное нажатие: запоминаем время, пропускаем событие дальше (ОС ставит музыку на паузу)
//! - Двойное нажатие (в пределах double_tap_ms): отправляем мьют, suppress'им событие

use std::sync::Mutex;
use std::time::{Duration, Instant};
use lazy_static::lazy_static;
use crossbeam_channel::Sender;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM, HINSTANCE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN, MSG,
};

lazy_static! {
    static ref LAST_PRESS: Mutex<Option<Instant>> = Mutex::new(None);
    static ref ENABLED: Mutex<bool> = Mutex::new(true);
    static ref DOUBLE_TAP_MS: Mutex<u64> = Mutex::new(400);
    static ref MUTE_SENDER: Mutex<Option<Sender<()>>> = Mutex::new(None);
    static ref HOOK_HANDLE: Mutex<Option<windows::Win32::UI::WindowsAndMessaging::HHOOK>> = Mutex::new(None);
}

/// Инициализация глобального хука. Должна быть вызвана ДО start_hook_thread.
pub fn init(config: std::sync::Arc<Mutex<crate::config::Config>>, sender: Sender<()>) {
    {
        let cfg = config.lock().unwrap();
        *ENABLED.lock().unwrap() = cfg.enabled;
        *DOUBLE_TAP_MS.lock().unwrap() = cfg.double_tap_ms;
    }
    *MUTE_SENDER.lock().unwrap() = Some(sender);
}

/// Включить/выключить обработку double-tap
pub fn set_enabled(enabled: bool) {
    *ENABLED.lock().unwrap() = enabled;
    log::info!("[MediaHook] TapMute enabled = {}", enabled);
}

/// Изменить timeout double-tap
pub fn set_double_tap_ms(ms: u64) {
    *DOUBLE_TAP_MS.lock().unwrap() = ms;
    log::info!("[MediaHook] Double-tap timeout = {} ms", ms);
}

/// Запускает отдельный поток с Windows message loop.
/// WH_KEYBOARD_LL требует message loop для доставки событий.
pub fn start_hook_thread() {
    std::thread::spawn(|| {
        unsafe {
            let hmod = GetModuleHandleW(None).expect("[MediaHook] GetModuleHandleW failed");
            let hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                HINSTANCE(hmod.0),
                0,
            ).expect("[MediaHook] SetWindowsHookExW failed");

            *HOOK_HANDLE.lock().unwrap() = Some(hook);
            log::info!("[MediaHook] Keyboard hook установлен в отдельном потоке");

            // Message loop — обязательна для low-level hooks
            let mut msg = MSG::default();
            loop {
                let result = GetMessageW(&mut msg, None, 0, 0);
                if result.0 == 0 {
                    break; // WM_QUIT
                }
                if result.0 == -1 {
                    log::error!("[MediaHook] GetMessageW вернул ошибку");
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            log::info!("[MediaHook] Поток хука завершён");
        }
    });
}

/// Callback процедура low-level keyboard hook.
/// Перехватывает VK_MEDIA_PLAY_PAUSE (0xB3).
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code < 0 {
        return CallNextHookEx(None, n_code, w_param, l_param);
    }

    let kbd = *(l_param.0 as *const KBDLLHOOKSTRUCT);

    // VK_MEDIA_PLAY_PAUSE = 0xB3
    if kbd.vkCode == 0xB3
        && (w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN)
    {
        let now = Instant::now();
        let mut last = LAST_PRESS.lock().unwrap();
        let enabled = *ENABLED.lock().unwrap();
        let timeout = *DOUBLE_TAP_MS.lock().unwrap();

        if let Some(t) = *last {
            if now.duration_since(t) < Duration::from_millis(timeout) {
                // ===== ДВОЙНОЙ ТАП =====
                *last = None;
                log::info!("[MediaHook] Обнаружен double-tap MediaPlayPause! Отправляем мьют.");
                if enabled {
                    if let Some(ref tx) = *MUTE_SENDER.lock().unwrap() {
                        let _ = tx.send(());
                    }
                }
                // Suppress — не пропускаем событие дальше в ОС
                return LRESULT(1);
            }
        }

        // ===== ОДИНОЧНЫЙ ТАП =====
        // Запоминаем время, но пропускаем событие дальше
        *last = Some(now);
        log::debug!("[MediaHook] Одиночный tap MediaPlayPause — пропускаем в ОС");
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}
