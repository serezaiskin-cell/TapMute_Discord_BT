#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod config;
mod mute_handler;
mod media_hook;
mod gui;
mod tray;
mod bluetooth;

use std::sync::{Arc, Mutex};
use crossbeam_channel::bounded;

use config::Config;
use mute_handler::GLOBAL_MUTE_HANDLER;
use tray::{TrayCommand, PlatformTray};
use bluetooth::{BluetoothState, start_bluetooth_monitor};

fn setup_logging() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
}

fn main() -> anyhow::Result<()> {
    setup_logging();

    // Загружаем конфиг из tapmute.toml рядом с exe
    let config = Config::load();
    let config_arc = Arc::new(Mutex::new(config.clone()));

    // Channel: media hook -> mute handler (отправляет () при double-tap)
    let (mute_tx, mute_rx) = bounded::<()>(10);

    // Channel: tray -> GUI
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<TrayCommand>();

    // Channel: bluetooth monitor -> GUI
    let (bt_tx, bt_rx) = crossbeam_channel::bounded::<BluetoothState>(10);

    // Устанавливаем keybind в mute handler
    {
        let cfg = config_arc.lock().unwrap();
        GLOBAL_MUTE_HANDLER.lock().unwrap().set_keybind(cfg.keybind.as_str());
    }

    // Инициализация и запуск глобального хука media-клавиш (Windows LL hook)
    media_hook::init(config_arc.clone(), mute_tx);
    media_hook::start_hook_thread();

    // Поток-обработчик: получает сигнал от hook и вызывает do_mute
    let config_for_mute = config_arc.clone();
    std::thread::spawn(move || {
        while let Ok(()) = mute_rx.recv() {
            let cfg = config_for_mute.lock().unwrap();
            if cfg.enabled {
                let keybind = cfg.keybind.clone();
                drop(cfg); // освобождаем мьютекс до вызова enigo
                GLOBAL_MUTE_HANDLER.lock().unwrap().do_mute(&keybind);
            }
        }
    });

    // Запускаем фоновый мониторинг Bluetooth (статус + батарея)
    start_bluetooth_monitor(bt_tx);

    // Инициализация системного трея
    let tray = PlatformTray::new(tray_tx.clone());

    // Настройки окна eframe
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(if config.compact_mode {
            egui::vec2(220.0, 200.0)
        } else {
            egui::vec2(980.0, 600.0)
        })
        .with_min_inner_size(egui::vec2(200.0, 180.0));

    // Загрузка иконки окна из assets/icon.png
    if let Ok(icon_data) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    // Создаём GUI-приложение
    let app = gui::TapMuteApp::new(config, config_arc, tray_tx, tray, tray_rx, bt_rx);

    // Запускаем eframe
    eframe::run_native(
        "TapMute Discord BT",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ).map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}
