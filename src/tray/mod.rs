#[cfg(target_os = "windows")]
pub mod windows;
pub mod icon;

#[cfg(target_os = "windows")]
pub use windows::WindowsTray as PlatformTray;

/// Команды, которые tray может отправлять в GUI
#[derive(Debug, Clone)]
pub enum TrayCommand {
    /// Показать главное окно
    ShowWindow,
    /// Переключить состояние микрофона (mute/unmute)
    ToggleMute,
    /// Завершить приложение
    Quit,
    /// Обновить данные о батарее (placeholder)
    RefreshBattery,
}
