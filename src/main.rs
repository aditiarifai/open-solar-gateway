use anyhow::{bail, Result};
use esp_idf_hal::delay::{FreeRtos, BLOCK};
use esp_idf_hal::gpio::{Output, PinDriver};
use esp_idf_hal::prelude::*;
use esp_idf_hal::uart::{config::Config as UartConfig, UartDriver};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use log::{info, warn};
use std::sync::{Arc, Mutex};
use std::time::Instant;

mod config;
mod dns_server;
mod growatt;
mod http_server;
mod jkbms;
mod local_telemetry;
mod wifi_manager;

use config::APP_CONFIG;
use dns_server::CaptiveDnsServer;
use growatt::*;
use http_server::start_http_server;
use jkbms::{poll_all_jkbms_ble, JkBmsTelemetry};
use local_telemetry::{LocalTelemetryManager, SharedTelemetryManager};
use wifi_manager::{WifiManager, WifiModeStatus};

fn main() -> Result<()> {
    // 1. Initialize Logger & NVS
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("=========================================================");
    info!("  OPEN SOLAR GATEWAY - GROWATT & JK-BMS FIRMWARE (RUST)");
    info!("=========================================================");

    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // Shared state for Local Telemetry (7-day RAM ring buffer) & Live Cache
    let shared_telemetry: SharedTelemetryManager =
        Arc::new(Mutex::new(LocalTelemetryManager::new()));
    shared_telemetry.lock().unwrap().init_partition();

    let latest_telemetry: Arc<Mutex<Option<GrowattTelemetry>>> = Arc::new(Mutex::new(None));

    // Spawn background serial command listener: type 'FETCH' or 'DUMP' via serial to export CSV history
    let telemetry_serial_clone = shared_telemetry.clone();
    std::thread::Builder::new()
        .stack_size(4096)
        .name("serial_cmd".into())
        .spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            let mut reader = stdin.lock();
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).is_ok() {
                    let cmd = line.trim();
                    if cmd.eq_ignore_ascii_case("FETCH") || cmd.eq_ignore_ascii_case("DUMP") {
                        telemetry_serial_clone.lock().unwrap().dump_csv();
                    }
                }
                FreeRtos::delay_ms(100);
            }
        })
        .ok();

    // 2. Initialize Wi-Fi Manager (NVS Storage, SoftAP Portal & Auto-Hostname 'solar')
    let wifi_mgr = Arc::new(WifiManager::new(nvs));
    let mut _wifi = None;
    let mut _http_server = None;
    let mut _dns_server = None;

    match wifi_mgr.init_wifi(peripherals.modem, sys_loop) {
        Ok(wifi) => {
            let (ip_str, is_ap) = match &*wifi_mgr.status.lock().unwrap() {
                WifiModeStatus::ConnectedSta { ip, .. } => (ip.clone(), false),
                WifiModeStatus::SoftApPortal { ip, .. } => (ip.clone(), true),
                _ => ("192.168.4.1".to_string(), false),
            };

            if is_ap {
                let ip_bytes = match ip_str.parse::<std::net::Ipv4Addr>() {
                    Ok(addr) => addr.octets(),
                    Err(_) => [192, 168, 71, 1],
                };
                _dns_server = Some(CaptiveDnsServer::start(ip_bytes));
            }

            match start_http_server(
                shared_telemetry.clone(),
                latest_telemetry.clone(),
                wifi_mgr.clone(),
                ip_str.clone(),
            ) {
                Ok(srv) => {
                    if is_ap {
                        info!("🌐 Wi-Fi Setup Portal active! Connect to 'OpenSolarGateway-Setup' and browse to http://{} or http://solar", ip_str);
                    } else {
                        info!("🌐 HTTP Server running! Open http://{} or http://solar in your browser", ip_str);
                    }
                    _http_server = Some(srv);
                }
                Err(e) => warn!("Failed to start HTTP Server: {:?}", e),
            }
            _wifi = Some(wifi);
        }
        Err(e) => warn!("Failed to initialize Wi-Fi: {:?}", e),
    }

    // 3. Hardware UART RS485 MAX485
    // DI -> GPIO 5 (TX), DE+RE -> GPIO 3 (tied together), RO -> GPIO 4 (RX)
    let mut dere_pin = PinDriver::output(peripherals.pins.gpio3)?;
    dere_pin.set_low()?;

    let uart_config = UartConfig::new().baudrate(Hertz(APP_CONFIG.modbus_baudrate));
    let mut uart = UartDriver::new(
        peripherals.uart1,
        peripherals.pins.gpio5,
        peripherals.pins.gpio4,
        Option::<esp_idf_hal::gpio::AnyIOPin>::None,
        Option::<esp_idf_hal::gpio::AnyIOPin>::None,
        &uart_config,
    )?;

    info!("Hardware RS485 ready: DI=GPIO5(TX), DE+RE=GPIO3, RO=GPIO4(RX)");

    let start_time = Instant::now();
    let mut last_bms_poll: Option<Instant> = None;
    let mut last_minute_tick = Instant::now();
    let mut cached_bms_packs: Vec<JkBmsTelemetry> = Vec::new();

    // 4. Main polling loop for RS485 Modbus & BLE JK-BMS
    loop {
        // A. Poll JK Smart BMS via BLE every jkbms_poll_interval_sec (paused while user is in Web Setup mode)
        let is_ap_mode = matches!(&*wifi_mgr.status.lock().unwrap(), WifiModeStatus::SoftApPortal { .. });
        if APP_CONFIG.enable_jkbms_ble && !is_ap_mode {
            let should_poll_bms = match last_bms_poll {
                None => true,
                Some(last) => last.elapsed().as_secs() >= APP_CONFIG.jkbms_poll_interval_sec,
            };

            if should_poll_bms {
                let latest_packs = poll_all_jkbms_ble();
                if !latest_packs.is_empty() {
                    cached_bms_packs = latest_packs;
                }
                last_bms_poll = Some(Instant::now());
            }
        }

        // B. Poll Growatt Inverter via RS485
        match poll_growatt_inverter(&mut uart, &mut dere_pin) {
            Ok(mut telemetry) => {
                telemetry.uptime_sec = start_time.elapsed().as_secs();
                telemetry.bms_packs = cached_bms_packs.clone();

                info!(
                    "☀️ PV: {:.1}W ({:.1}V) | 🔋 Bat: {:.2}V | 📦 BMS: {} Pack(s) | ⚡ Load: {:.1}W | Today: {:.2}kWh | Status: {}",
                    telemetry.solar.pv_power_w,
                    telemetry.solar.pv_voltage_v,
                    telemetry.battery.voltage_v,
                    telemetry.bms_packs.len(),
                    telemetry.load.power_w,
                    telemetry.solar.energy_today_kwh,
                    telemetry.status
                );

                // Update minute accumulator sample
                shared_telemetry.lock().unwrap().ingest_sample(
                    telemetry.solar.pv_power_w,
                    telemetry.solar.pv_voltage_v,
                    telemetry.load.power_w,
                    telemetry.load.voltage_v,
                    telemetry.thermal.inverter_temp_c,
                    telemetry.solar.energy_today_kwh,
                    telemetry.battery.voltage_v,
                    &telemetry.bms_packs,
                );

                // Update live cache for HTTP Server
                *latest_telemetry.lock().unwrap() = Some(telemetry);
            }
            Err(e) => {
                warn!("Modbus RS485 Timeout / Error: {:?}", e);
            }
        }

        // C. Check if 1 minute (60s) has elapsed to commit 1-min record to RAM buffer and 1MB Flash
        if last_minute_tick.elapsed().as_secs() >= 60 {
            let epoch_now = start_time.elapsed().as_secs() as u32;
            shared_telemetry.lock().unwrap().finalize_minute(epoch_now);
            last_minute_tick = Instant::now();
        }

        FreeRtos::delay_ms(APP_CONFIG.polling_interval_ms as u32);
    }
}

// =========================================================================
// HELPER: SEND REQUEST & READ RESPONSE FOR MODBUS RTU
// =========================================================================
fn poll_growatt_inverter<'d, T>(
    uart: &mut UartDriver,
    dere_pin: &mut PinDriver<'d, T, Output>,
) -> Result<GrowattTelemetry>
where
    T: esp_idf_hal::gpio::Pin,
{
    uart.clear_rx()?;

    let request_frame = build_read_input_registers_frame(APP_CONFIG.modbus_slave_id, 0, 52);

    dere_pin.set_high()?; // HIGH: Transmit mode (DE=1, RE=1)

    uart.write(&request_frame)?;
    uart.wait_tx_done(BLOCK)?;

    // Guard delay 1 ms so the last byte (CRC) is completely shifted out before switching to RX
    FreeRtos::delay_ms(1);

    dere_pin.set_low()?; // LOW: Receive mode (DE=0, RE=0)

    let mut rx_buf = [0u8; 160];
    let mut bytes_read = 0;
    let read_start = Instant::now();

    // Accumulate incoming bytes up to 109 bytes (3-byte header + 104-byte payload [52 registers] + 2-byte CRC)
    while bytes_read < 109 && read_start.elapsed().as_millis() < 1000 {
        match uart.read(&mut rx_buf[bytes_read..], 50) {
            Ok(n) if n > 0 => bytes_read += n,
            _ => {
                FreeRtos::delay_ms(5);
            }
        }
    }

    if bytes_read < 109 {
        bail!("Modbus response too short / timeout: {} bytes (expected 109)", bytes_read);
    }

    if rx_buf[0] != APP_CONFIG.modbus_slave_id || rx_buf[1] != 0x04 {
        bail!("Invalid Modbus header: ID={}, FC={}", rx_buf[0], rx_buf[1]);
    }

    let received_crc = ((rx_buf[bytes_read - 1] as u16) << 8) | (rx_buf[bytes_read - 2] as u16);
    let calculated_crc = calculate_crc16(&rx_buf[0..bytes_read - 2]);

    if received_crc != calculated_crc {
        bail!("CRC16 Mismatch! Received: {:04X}, Calculated: {:04X}", received_crc, calculated_crc);
    }

    let payload = &rx_buf[3..bytes_read - 2];

    let status_code = get_reg_u16(payload, 0);
    let status_str = get_status_string(status_code);

    let pv_volt = (get_reg_u16(payload, 3) as f32) * 0.1;
    let mut pv_power = (get_reg_u32(payload, 1, 2) as f32) * 0.1;
    if pv_power == 0.0 {
        pv_power = (get_reg_u32(payload, 5, 6) as f32) * 0.1;
    }

    let mut ac_volt = (get_reg_u16(payload, 13) as f32) * 0.1;
    if ac_volt == 0.0 {
        ac_volt = (get_reg_u16(payload, 16) as f32) * 0.1;
    }
    let mut ac_freq = (get_reg_u16(payload, 14) as f32) * 0.01;
    if ac_freq == 0.0 {
        ac_freq = (get_reg_u16(payload, 17) as f32) * 0.01;
    }
    let mut load_w = (get_reg_u32(payload, 9, 10) as f32) * 0.1;
    if load_w == 0.0 {
        load_w = (get_reg_u16(payload, 20) as f32) * 0.1;
    }
    let mut load_va = (get_reg_u32(payload, 11, 12) as f32) * 0.1;
    if load_va == 0.0 {
        load_va = (get_reg_u16(payload, 19) as f32) * 0.1;
    }

    let mut bat_volt = (get_reg_u16(payload, 17) as f32) * 0.01;
    if bat_volt == 0.0 {
        bat_volt = (get_reg_u16(payload, 27) as f32) * 0.01;
    }
    let mut bat_soc = get_reg_u16(payload, 18) as u8;
    if bat_soc == 0 {
        bat_soc = get_reg_u16(payload, 28) as u8;
    }

    let mut et_today = (get_reg_u32(payload, 48, 49) as f32) * 0.1;
    if et_today == 0.0 {
        et_today = (get_reg_u32(payload, 33, 34) as f32) * 0.1;
    }
    let mut et_total = (get_reg_u32(payload, 50, 51) as f32) * 0.1;
    if et_total == 0.0 {
        et_total = (get_reg_u32(payload, 35, 36) as f32) * 0.1;
    }

    let mut inv_temp = (get_reg_u16(payload, 25) as f32) * 0.1;
    if inv_temp == 0.0 {
        inv_temp = (get_reg_u16(payload, 21) as f32) * 0.1;
    }
    if inv_temp == 0.0 {
        inv_temp = (get_reg_u16(payload, 37) as f32) * 0.1;
    }

    let mut dcdc_temp = (get_reg_u16(payload, 23) as f32) * 0.1;
    if dcdc_temp == 0.0 {
        dcdc_temp = (get_reg_u16(payload, 38) as f32) * 0.1;
    }

    Ok(GrowattTelemetry {
        device_id: "growatt_spf5000es".to_string(),
        uptime_sec: 0,
        status_code,
        status: status_str,
        valid: true,
        solar: SolarData {
            pv_voltage_v: pv_volt,
            pv_power_w: pv_power,
            energy_today_kwh: et_today,
            energy_total_kwh: et_total,
        },
        battery: BatteryData {
            voltage_v: bat_volt,
            soc_percent: bat_soc,
            power_w: 0.0,
        },
        bms_packs: Vec::new(),
        load: LoadData {
            voltage_v: ac_volt,
            frequency_hz: ac_freq,
            power_w: load_w,
            apparent_power_va: load_va,
        },
        grid: GridData {
            voltage_v: 0.0,
            frequency_hz: 0.0,
        },
        thermal: ThermalData {
            inverter_temp_c: inv_temp,
            dcdc_temp_c: dcdc_temp,
        },
    })
}
