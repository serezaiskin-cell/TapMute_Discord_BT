use std::sync::{mpsc::Sender, Arc, Mutex};
use std::collections::HashMap;
use tray_icon::{
    TrayIcon, TrayIconBuilder, Icon, TrayIconEvent,
    menu::{Menu, MenuItem, PredefinedMenuItem, MenuEvent, MenuId},
};
use super::icon::{TrayIconConfig, generate_battery_icon_rgba};

/// Windows-реализация системного трея.
/// Создаёт иконку в системном tray, обновляет её при изменении батареи/мьюта.
pub struct WindowsTray {
    tray: Option<TrayIcon>,
    tx: Sender<super::TrayCommand>,
    callbacks: Arc<Mutex<HashMap<MenuId, Box<dyn Fn() + Send>>>>,
    last_percent: u8,
    last_charging: bool,
    last_muted: bool,
    enabled: bool,
    icon_config: TrayIconConfig,
}

impl WindowsTray {
    pub fn new(tx: Sender<super::TrayCommand>) -> Self {
        // Инициализация COM для tray-icon (обязательно на Windows)
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let callbacks: Arc<Mutex<HashMap<MenuId, Box<dyn Fn() + Send>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let icon_config = TrayIconConfig::load_or_create();

        // Начальная иконка: 0% батареи (серый цвет)
        let (rgba, w, h) = generate_battery_icon_rgba(&icon_config, 0, false);
        let icon = Icon::from_rgba(rgba, w, h)
            .unwrap_or_else(|_| Icon::from_rgba(vec![255; 4], 1, 1).unwrap());

        let tray = TrayIconBuilder::new()
            .with_tooltip("TapMute — подключение...")
            .with_icon(icon)
            .with_menu(Box::new(Menu::new()))
            .build();

        let tray: Option<TrayIcon> = match tray {
            Ok(t) => {
                log::info!("[Tray] Tray icon создан успешно");
                Some(t)
            }
            Err(e) => {
                log::error!("[Tray] Не удалось создать tray icon: {}. Продолжаем без трея.", e);
                None
            }
        };

        let mut this = Self {
            tray,
            tx: tx.clone(),
            callbacks,
            last_percent: 0,
            last_charging: false,
            last_muted: false,
            enabled: true,
            icon_config,
        };

        this.update_tooltip();
        this.rebuild_menu();

        // Поток обработки кликов по меню
        let tx_menu = tx.clone();
        let cb_clone = this.callbacks.clone();
        std::thread::spawn(move || {
            let rx = MenuEvent::receiver();
            loop {
                if let Ok(event) = rx.recv() {
                    log::info!("[Tray] Меню клик: id={:?}", event.id);
                    if let Ok(map) = cb_clone.try_lock() {
                        if let Some(f) = map.get(&event.id) {
                            f();
                        } else {
                            log::warn!("[Tray] Неизвестный menu id: {:?}", event.id);
                        }
                    }
                }
            }
        });

        // Поток обработки кликов по иконке (левая кнопка — показать окно)
        let tx_icon = tx.clone();
        std::thread::spawn(move || {
            let rx = TrayIconEvent::receiver();
            loop {
                if let Ok(event) = rx.recv() {
                    if let TrayIconEvent::Click {
                        button: tray_icon::MouseButton::Left,
                        ..
                    } = event {
                        let _ = tx_icon.send(super::TrayCommand::ShowWindow);
                    }
                }
            }
        });

        this
    }

    /// Пересобирает контекстное меню трея
    fn rebuild_menu(&mut self) {
        let Some(tray) = &self.tray else { return; };
        let menu = Menu::new();
        let mut new_callbacks: HashMap<MenuId, Box<dyn Fn() + Send>> = HashMap::new();

        // --- Информационные пункты ---
        let battery_text = if self.last_charging {
            format!("⚡ Заряд: {}%", self.last_percent)
        } else if self.last_percent > 0 {
            format!("🔋 Батарея: {}%", self.last_percent)
        } else {
            "🔋 Батарея: недоступна".to_string()
        };
        let _ = menu.append(&MenuItem::new(&battery_text, false, None));

        let mic_text = if self.last_muted {
            "🔇 Микрофон: выкл"
        } else {
            "🎙️ Микрофон: вкл"
        };
        let _ = menu.append(&MenuItem::new(mic_text, false, None));

        let _ = menu.append(&PredefinedMenuItem::separator());

        // --- Включить/Выключить TapMute ---
        let toggle_text = if self.enabled { "⏸️ Выключить TapMute" } else { "▶️ Включить TapMute" };
        let toggle_i = MenuItem::new(toggle_text, true, None);
        let _ = menu.append(&toggle_i);
        let tx_toggle = self.tx.clone();
        new_callbacks.insert(
            toggle_i.id().clone(),
            Box::new(move || {
                let _ = tx_toggle.send(super::TrayCommand::ToggleMute);
            }),
        );

        let _ = menu.append(&PredefinedMenuItem::separator());

        // --- Открыть ---
        let open_i = MenuItem::new("Открыть", true, None);
        let _ = menu.append(&open_i);
        let tx_open = self.tx.clone();
        new_callbacks.insert(
            open_i.id().clone(),
            Box::new(move || { let _ = tx_open.send(super::TrayCommand::ShowWindow); }),
        );

        // --- Переключить мьют ---
        let mute_i = MenuItem::new("Переключить мьют", true, None);
        let _ = menu.append(&mute_i);
        let tx_mute = self.tx.clone();
        new_callbacks.insert(
            mute_i.id().clone(),
            Box::new(move || { let _ = tx_mute.send(super::TrayCommand::ToggleMute); }),
        );

        let _ = menu.append(&PredefinedMenuItem::separator());

        // --- Выход ---
        let quit_i = MenuItem::new("Выход", true, None);
        let _ = menu.append(&quit_i);
        let tx_quit = self.tx.clone();
        new_callbacks.insert(
            quit_i.id().clone(),
            Box::new(move || { let _ = tx_quit.send(super::TrayCommand::Quit); }),
        );

        *self.callbacks.lock().unwrap() = new_callbacks;
        let _ = tray.set_menu(Some(Box::new(menu)));
    }

    fn update_tooltip(&self) {
        let Some(tray) = &self.tray else { return; };
        let mic_status = if self.last_muted { "🔇 Выключен" } else { "🎙️ Включён" };
        let tooltip = if self.last_charging {
            format!("TapMute Discord BT\n⚡ Заряжается: {}%\n🎤 Микрофон: {}", self.last_percent, mic_status)
        } else if self.last_percent > 0 {
            format!("TapMute Discord BT\n🔋 Батарея: {}%\n🎤 Микрофон: {}", self.last_percent, mic_status)
        } else {
            format!("TapMute Discord BT\n🎧 BT статус недоступен\n🎤 Микрофон: {}", mic_status)
        };
        let _ = tray.set_tooltip(Some(&tooltip));
    }

    fn update_icon(&mut self) {
        let Some(tray) = &self.tray else { return; };
        let (rgba, w, h) = generate_battery_icon_rgba(&self.icon_config, self.last_percent, self.last_charging);
        if let Ok(icon) = Icon::from_rgba(rgba, w, h) {
            let _ = tray.set_icon(Some(icon));
        }
    }

    /// Обновляет данные о батарее и перерисовывает иконку/меню
    pub fn update_battery(&mut self, percent: u8, charging: bool) {
        log::info!(
            "[Tray] update_battery: percent={} charging={} (last={}/{})",
            percent, charging, self.last_percent, self.last_charging
        );
        if self.last_percent != percent || self.last_charging != charging {
            self.last_percent = percent;
            self.last_charging = charging;
            self.update_tooltip();
            self.update_icon();
            self.rebuild_menu();
        }
    }

    /// Обновляет статус микрофона
    pub fn update_mute(&mut self, muted: bool) {
        if self.last_muted != muted {
            self.last_muted = muted;
            self.update_tooltip();
            self.rebuild_menu();
        }
    }

    /// Установить состояние enabled (для обновления меню)
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.rebuild_menu();
        }
    }

    pub fn poll(&self) {}
}
