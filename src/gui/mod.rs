use eframe::egui;
use crate::config::{Config, Keybind};
use crate::tray::{TrayCommand, PlatformTray};
use crate::mute_handler::GLOBAL_MUTE_HANDLER;
use crate::bluetooth::BluetoothState;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use crossbeam_channel::Receiver;

/// Главное приложение GUI (eframe App)
pub struct TapMuteApp {
    pub config: Config,
    pub config_arc: Arc<Mutex<Config>>,
    pub tray_tx: Option<std::sync::mpsc::Sender<TrayCommand>>,
    pub tray: Option<PlatformTray>,
    pub tray_rx: Option<std::sync::mpsc::Receiver<TrayCommand>>,
    pub bt_rx: Receiver<BluetoothState>,
    pub needs_save: bool,
    pub bt_connected: bool,
    pub battery_percent: u8,
    pub is_muted: bool,
    pub selected_tab: Tab,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Dashboard,
    Settings,
}

impl Default for Tab {
    fn default() -> Self { Tab::Dashboard }
}

impl TapMuteApp {
    pub fn new(
        config: Config,
        config_arc: Arc<Mutex<Config>>,
        tray_tx: std::sync::mpsc::Sender<TrayCommand>,
        tray: PlatformTray,
        tray_rx: std::sync::mpsc::Receiver<TrayCommand>,
        bt_rx: Receiver<BluetoothState>,
    ) -> Self {
        Self {
            config,
            config_arc,
            tray_tx: Some(tray_tx),
            tray: Some(tray),
            tray_rx: Some(tray_rx),
            bt_rx,
            needs_save: false,
            bt_connected: false,
            battery_percent: 0,
            is_muted: false,
            selected_tab: Tab::Dashboard,
        }
    }

    /// Сохраняет конфиг на диск и обновляет runtime-настройки
    fn save_config(&mut self) {
        log::info!("[GUI] Сохранение конфигурации...");
        let mut cfg = self.config_arc.lock().unwrap();
        *cfg = self.config.clone();
        if let Err(e) = cfg.save() {
            log::error!("[GUI] Ошибка сохранения: {}", e);
        } else {
            log::info!("[GUI] Конфигурация сохранена");
            self.needs_save = false;
        }
        // Обновляем настройки хука и mute handler
        crate::media_hook::set_enabled(self.config.enabled);
        crate::media_hook::set_double_tap_ms(self.config.double_tap_ms);
        GLOBAL_MUTE_HANDLER.lock().unwrap().set_keybind(self.config.keybind.as_str());
    }

    /// Обновляет автозапуск Windows
    fn update_autostart(&mut self) {
        #[cfg(target_os = "windows")]
        {
            if let Err(e) = set_windows_autostart(self.config.start_with_os) {
                log::error!("[GUI] Ошибка автозапуска: {}", e);
            }
        }
    }

    /// Обновляет tray из текущего состояния
    fn sync_tray(&mut self) {
        if let Some(tray) = &mut self.tray {
            tray.update_battery(self.battery_percent, false);
            tray.update_mute(self.is_muted);
            tray.set_enabled(self.config.enabled);
        }
    }
}

impl eframe::App for TapMuteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Запрашиваем перерисовку каждые 100 мс для актуальности UI
        ctx.request_repaint_after(Duration::from_millis(100));

        // Тёмная тема в стиле HyperX (красные акценты)
        ctx.set_visuals(egui::Visuals::dark());
        ctx.style_mut(|s| {
            s.visuals.selection.bg_fill = egui::Color32::from_rgb(200, 30, 30);
            s.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(200, 30, 30);
            s.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(160, 20, 20);
        });

        // --- Обработка данных от Bluetooth monitor ---
        while let Ok(state) = self.bt_rx.try_recv() {
            self.bt_connected = state.connected;
            self.battery_percent = state.battery_percent;
            self.sync_tray();
        }

        // --- Обработка команд из трея ---
        if let Some(rx) = &self.tray_rx {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    TrayCommand::ShowWindow => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    TrayCommand::ToggleMute => {
                        self.is_muted = !self.is_muted;
                        self.sync_tray();
                    }
                    TrayCommand::Quit => {
                        std::process::exit(0);
                    }
                    TrayCommand::RefreshBattery => {}
                }
            }
        }

        // Закрытие окна = выход
        if ctx.input(|i| i.viewport().close_requested()) {
            std::process::exit(0);
        }

        // ===== КОМПАКТНЫЙ РЕЖИМ (220×200) =====
        if self.config.compact_mode {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([220.0, 200.0].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            self.show_compact_ui(ctx);
        } else {
            // ===== ПОЛНЫЙ РЕЖИМ (980×600) =====
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));

            // Верхняя панель с табами и статусом
            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.selected_tab, Tab::Dashboard, "📊 Дашборд");
                    ui.selectable_value(&mut self.selected_tab, Tab::Settings, "⚙️ Настройки");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Кнопка переключения компактного режима
                        if ui.button("⛶").clicked() {
                            self.config.compact_mode = !self.config.compact_mode;
                            self.needs_save = true;
                            if !self.config.compact_mode {
                                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([980.0, 600.0].into()));
                            }
                        }
                        ui.separator();
                        // Статус BT
                        let status = if self.bt_connected {
                            let bat = if self.battery_percent > 0 {
                                format!(" ({}%)", self.battery_percent)
                            } else {
                                "".to_string()
                            };
                            egui::RichText::new(format!("🎧 BT подключён{}", bat)).color(egui::Color32::GREEN)
                        } else {
                            egui::RichText::new("❌ BT не подключён").color(egui::Color32::RED)
                        };
                        ui.label(status);
                    });
                });
            });

            // Центральная панель
            egui::CentralPanel::default().show(ctx, |ui| {
                match self.selected_tab {
                    Tab::Dashboard => self.show_dashboard(ui),
                    Tab::Settings => self.show_settings(ui),
                }
            });

            // Панель несохранённых изменений
            if self.needs_save {
                egui::TopBottomPanel::bottom("save_bar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::YELLOW, "⚠️ Несохранённые изменения");
                        if ui.button("💾 Сохранить").clicked() {
                            self.save_config();
                            self.update_autostart();
                        }
                    });
                });
            }
        }
    }
}

impl TapMuteApp {
    /// Компактный UI (220×200) — батарея, микрофон, BT статус, кнопка развернуть
    fn show_compact_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);

                // --- Индикатор батареи ---
                let battery_text = if self.battery_percent > 0 {
                    format!("🔋 {}%", self.battery_percent)
                } else if self.bt_connected {
                    "🔋 Батарея недоступна".to_string()
                } else {
                    "❌ BT не подключён".to_string()
                };
                let battery_color = if self.battery_percent > 50 {
                    egui::Color32::GREEN
                } else if self.battery_percent > 20 {
                    egui::Color32::YELLOW
                } else if self.battery_percent > 0 {
                    egui::Color32::RED
                } else {
                    egui::Color32::GRAY
                };
                ui.colored_label(battery_color, battery_text);
                if self.battery_percent > 0 {
                    ui.add(egui::ProgressBar::new(self.battery_percent as f32 / 100.0)
                        .desired_width(180.0)
                        .fill(battery_color));
                }

                ui.add_space(8.0);

                // --- Статус микрофона ---
                let mic_icon = if self.is_muted { "🔇" } else { "🎙️" };
                let mic_text = if self.is_muted { "Мьют" } else { "Микрофон вкл" };
                let mic_color = if self.is_muted { egui::Color32::RED } else { egui::Color32::GREEN };
                ui.colored_label(mic_color, format!("{} {}", mic_icon, mic_text));

                ui.add_space(8.0);

                // --- BT статус ---
                let status = if self.bt_connected {
                    egui::RichText::new("🎧 BT подключено").color(egui::Color32::GREEN)
                } else {
                    egui::RichText::new("❌ BT не подключено").color(egui::Color32::RED)
                };
                ui.label(status);

                ui.add_space(10.0);

                // --- Кнопка развернуть ---
                if ui.button("⛶ Развернуть").clicked() {
                    self.config.compact_mode = false;
                    self.needs_save = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([980.0, 600.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                }
            });
        });
    }

    /// Дашборд полного режима
    fn show_dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("📊 Дашборд");
        ui.separator();

        ui.horizontal(|ui| {
            // --- Батарея ---
            ui.vertical(|ui| {
                ui.label("Батарея гарнитуры:");
                if self.battery_percent > 0 {
                    let color = if self.battery_percent > 50 { egui::Color32::GREEN }
                        else if self.battery_percent > 20 { egui::Color32::YELLOW }
                        else { egui::Color32::RED };
                    ui.colored_label(color, format!("{}%", self.battery_percent));
                    ui.add(egui::ProgressBar::new(self.battery_percent as f32 / 100.0)
                        .fill(color)
                        .desired_width(200.0));
                } else if self.bt_connected {
                    ui.colored_label(egui::Color32::GRAY, "Батарея недоступна (устройство не поддерживает GATT)");
                } else {
                    ui.colored_label(egui::Color32::GRAY, "BT не подключён");
                }
            });

            ui.separator();

            // --- Микрофон и BT ---
            ui.vertical(|ui| {
                ui.label("Микрофон:");
                if self.is_muted {
                    ui.colored_label(egui::Color32::RED, "🔇 Мьют");
                } else {
                    ui.colored_label(egui::Color32::GREEN, "🎙️ Активен");
                }

                ui.add_space(10.0);

                ui.label("BT статус:");
                if self.bt_connected {
                    ui.colored_label(egui::Color32::GREEN, "🎧 Подключено");
                } else {
                    ui.colored_label(egui::Color32::RED, "❌ Не подключено");
                }
            });
        });

        ui.separator();
        ui.heading("Быстрые действия");
        ui.horizontal(|ui| {
            if ui.button(if self.is_muted { "🎙️ Размьютить" } else { "🔇 Замьютить" }).clicked() {
                self.is_muted = !self.is_muted;
                self.sync_tray();
            }
        });
    }

    /// Настройки полного режима
    fn show_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("⚙️ Настройки");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            // --- Keybind ---
            ui.horizontal(|ui| {
                ui.label("Keybind для мьют:");
                egui::ComboBox::from_id_salt("keybind_combo")
                    .selected_text(self.config.keybind.as_str())
                    .show_ui(ui, |ui| {
                        for key in Keybind::all() {
                            if ui.selectable_value(&mut self.config.keybind, key.clone(), key.as_str()).changed() {
                                self.needs_save = true;
                            }
                        }
                    });
            });

            ui.add_space(5.0);

            // --- Включить TapMute ---
            if ui.checkbox(&mut self.config.enabled, "Включить TapMute").changed() {
                self.needs_save = true;
            }

            ui.add_space(5.0);

            // --- Double-tap timeout ---
            ui.horizontal(|ui| {
                ui.label("Double-tap timeout:");
                ui.add(egui::Slider::new(&mut self.config.double_tap_ms, 200..=1000).text("мс"));
                if ui.button("Применить").clicked() {
                    self.needs_save = true;
                }
            });

            ui.add_space(5.0);

            // --- Автозапуск ---
            if ui.checkbox(&mut self.config.start_with_os, "Запускать с системой").changed() {
                self.needs_save = true;
            }

            ui.add_space(10.0);

            // --- Test Mute ---
            if ui.button("🧪 Test Mute").clicked() {
                let keybind = self.config.keybind.clone();
                GLOBAL_MUTE_HANDLER.lock().unwrap().test_mute(&keybind);
            }

            ui.add_space(15.0);
            ui.separator();
            ui.heading("О приложении");
            ui.label("TapMute Discord BT v0.1.0");
            ui.label("Двойной тап Play/Pause на Bluetooth-гарнитуре для мьюта в Discord.");
            ui.hyperlink_to("GitHub", "https://github.com/yourname/TapMute_discord_bt");
        });
    }
}

/// Управление автозапуском Windows через реестр
#[cfg(target_os = "windows")]
fn set_windows_autostart(enabled: bool) -> Result<(), Box<dyn std::error::Error>> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let (key, _) = hkcu.create_subkey(path)?;
    let exe_path = std::env::current_exe()?.to_string_lossy().to_string();

    if enabled {
        key.set_value("TapMuteDiscordBT", &exe_path)?;
        log::info!("[Autostart] Добавлено в автозапуск: {}", exe_path);
    } else {
        key.delete_value("TapMuteDiscordBT").ok();
        log::info!("[Autostart] Удалено из автозапуска");
    }
    Ok(())
}
