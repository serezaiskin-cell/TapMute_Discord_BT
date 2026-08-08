use crossbeam_channel::Sender;
use std::time::Duration;

/// Состояние Bluetooth-гарнитуры
#[derive(Debug, Clone, Copy)]
pub struct BluetoothState {
    pub connected: bool,
    pub battery_percent: u8,
}

/// Запускает фоновый поток, который каждые 5 секунд опрашивает BT статус и батарею.
pub fn start_bluetooth_monitor(tx: Sender<BluetoothState>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("[Bluetooth] Не удалось создать tokio runtime");

        loop {
            let connected = rt.block_on(is_bluetooth_audio_connected());
            let battery = if connected {
                rt.block_on(scan_ble_battery()).unwrap_or(0)
            } else {
                0
            };

            let state = BluetoothState { connected, battery_percent: battery };
            log::info!("[Bluetooth] Состояние: connected={}, battery={}%", connected, battery);
            let _ = tx.send(state);

            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

// ============================================================================
// Windows WASAPI: определяем, подключено ли Bluetooth audio устройство
// ============================================================================

#[cfg(target_os = "windows")]
async fn is_bluetooth_audio_connected() -> bool {
    use windows::Win32::System::Com::{
        CoInitializeEx, CoCreateInstance, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::Media::Audio::{
        eRender, eMultimedia, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::Media::Audio::Endpoints::IMMDevice;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::UI::Shell::PropertiesSystem::PROPERTYKEY;
    use windows::Win32::Foundation::BOOL;
    use windows::core::GUID;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let enumerator: IMMDeviceEnumerator = match CoCreateInstance(
            &MMDeviceEnumerator,
            None,
            CLSCTX_ALL,
        ) {
            Ok(e) => e,
            Err(e) => {
                log::error!("[Bluetooth] CoCreateInstance MMDeviceEnumerator failed: {}", e);
                return false;
            }
        };

        let collection = match enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[Bluetooth] EnumAudioEndpoints failed: {}", e);
                return false;
            }
        };

        let count = match collection.GetCount() {
            Ok(c) => c,
            Err(_) => return false,
        };

        // PKEY_Device_Bluetooth = {a92f68b5-961a-40b3-8d02-a9c3d511d0d3}, 2
        let pk_bluetooth = PROPERTYKEY {
            fmtid: GUID::from_u128(0xa92f68b5961a40b38d02a9c3d511d0d3),
            pid: 2,
        };

        for i in 0..count {
            let device: IMMDevice = match collection.Item(i) {
                Ok(d) => d,
                Err(_) => continue,
            };

            let props = match device.OpenPropertyStore(STGM_READ) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let mut var: PROPVARIANT = std::mem::zeroed();
            if props.GetValue(&pk_bluetooth, &mut var).is_ok() {
                // Если свойство существует и true — это Bluetooth устройство
                if var.Anonymous.Anonymous.vt.0 == 11 { // VT_BOOL
                    let val: BOOL = std::mem::transmute(var.Anonymous.Anonymous.Anonymous.boolVal);
                    if val.as_bool() {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[cfg(not(target_os = "windows"))]
async fn is_bluetooth_audio_connected() -> bool {
    false
}

// ============================================================================
// btleplug: сканируем BLE и читаем Battery Service (0x180F)
// ============================================================================

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
async fn scan_ble_battery() -> Option<u8> {
    use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, Characteristic};
    use btleplug::platform::Manager;
    use uuid::Uuid;

    let manager = Manager::new().await.ok()?;
    let adapters = manager.adapters().await.ok()?;

    for adapter in adapters {
        let _ = adapter.start_scan(ScanFilter::default()).await;
        tokio::time::sleep(Duration::from_secs(2)).await;

        let peripherals = adapter.peripherals().await.ok()?;

        for peripheral in peripherals {
            let is_connected = peripheral.is_connected().await.ok()?;
            if !is_connected {
                let _ = peripheral.connect().await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            if !peripheral.is_connected().await.ok()? {
                continue;
            }

            let _ = peripheral.discover_services().await;
            let characteristics = peripheral.characteristics();

            // Battery Level characteristic: 00002a19-0000-1000-8000-00805f9b34fb
            let battery_uuid = Uuid::from_u128(0x00002a1900001000800000805f9b34fb);

            if let Some(ch) = characteristics.iter().find(|c| c.uuid == battery_uuid) {
                if let Ok(data) = peripheral.read(ch).await {
                    if let Some(&level) = data.first() {
                        log::info!("[Bluetooth] Battery level read: {}% from {:?}", level, peripheral.properties().await);
                        let _ = peripheral.disconnect().await;
                        return Some(level);
                    }
                }
            }

            let _ = peripheral.disconnect().await;
        }

        let _ = adapter.stop_scan().await;
    }

    None
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
async fn scan_ble_battery() -> Option<u8> {
    None
}
