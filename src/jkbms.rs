use anyhow::{bail, Result};
use esp32_nimble::{utilities::BleUuid, BLEAddress, BLEDevice, BLEScan};
use esp_idf_hal::delay::FreeRtos;
use esp_idf_svc::hal::task::block_on;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

use crate::config::APP_CONFIG;

// =========================================================================
// JK SMART BMS TELEMETRY DATA STRUCTURE (PACK 1 & PACK 2 - 2P16S)
// =========================================================================

pub const JKBMS_SERVICE_UUID16: u16 = 0xFFE0;
pub const JKBMS_CHAR_UUID16: u16 = 0xFFE1;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum JkProtocolVersion {
    Jk02_24S,
    Jk02_32S,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JkBmsTelemetry {
    pub pack_id: u8,                // 1 = Pack 1, 2 = Pack 2
    pub name: String,               // BLE advertised name
    pub mac_address: String,        // BLE MAC address
    pub total_voltage_v: f32,       // Total pack voltage (V)
    pub current_a: f32,             // Current (+ Charge, - Discharge) (A)
    pub power_w: f32,               // Pack power (W)
    pub soc_percent: u8,            // Coulomb-counting State of Charge (%)
    pub remaining_ah: f32,          // Remaining capacity (Ah)
    pub nominal_ah: f32,            // Nominal capacity (Ah)
    pub cycle_count: u32,           // Cycle count
    pub cell_count: u8,             // Active cell count (e.g. 16S)
    pub cell_voltages_v: Vec<f32>,  // Individual cell voltages 1..16 (V)
    pub cell_min_v: f32,            // Lowest cell voltage (V)
    pub cell_max_v: f32,            // Highest cell voltage (V)
    pub cell_delta_mv: f32,         // Delta Max - Min (mV) -> Cell health indicator
    pub balance_current_a: f32,     // Active balancer current (A)
    pub balance_active: bool,       // Active balancer status (ON/OFF)
    pub mos_temp_c: f32,            // BMS MOSFET temperature (°C)
    pub temp1_c: f32,               // Battery temperature sensor T1 (°C)
    pub temp2_c: f32,               // Battery temperature sensor T2 (°C)
}

// =========================================================================
// JK-BMS BLE COMMAND FRAME GENERATOR (20 BYTES)
// =========================================================================

pub fn build_jkbms_command(cmd: u8) -> [u8; 20] {
    let mut frame = [0u8; 20];
    frame[0] = 0xAA;
    frame[1] = 0x55;
    frame[2] = 0x90;
    frame[3] = 0xEB;
    frame[4] = cmd;
    frame[5] = 0x00;

    let mut crc: u8 = 0;
    for i in 0..19 {
        crc = crc.wrapping_add(frame[i]);
    }
    frame[19] = crc;
    frame
}

// =========================================================================
// JK-BMS BLE NOTIFICATION FRAME PARSER (300 BYTES)
// =========================================================================

#[inline]
fn read_u16_le(buf: &[u8], offset: usize) -> u16 {
    if offset + 1 < buf.len() {
        u16::from_le_bytes([buf[offset], buf[offset + 1]])
    } else {
        0
    }
}

#[inline]
fn read_i16_le(buf: &[u8], offset: usize) -> i16 {
    if offset + 1 < buf.len() {
        i16::from_le_bytes([buf[offset], buf[offset + 1]])
    } else {
        0
    }
}

#[inline]
fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    if offset + 3 < buf.len() {
        u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
    } else {
        0
    }
}

#[inline]
fn read_i32_le(buf: &[u8], offset: usize) -> i32 {
    if offset + 3 < buf.len() {
        i32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
    } else {
        0
    }
}

pub fn parse_jkbms_cell_info_frame(
    pack_id: u8,
    name: &str,
    mac_address: &str,
    frame: &[u8],
    protocol: JkProtocolVersion,
) -> Result<JkBmsTelemetry> {
    if frame.len() < 200 {
        bail!("JK-BMS frame too short: {} bytes", frame.len());
    }

    if frame[0] != 0x55 || frame[1] != 0xAA || frame[2] != 0xEB || frame[3] != 0x90 {
        bail!("Invalid JK-BMS header: {:02X} {:02X}", frame[0], frame[1]);
    }

    if frame[4] != 0x02 {
        bail!("Not a Cell Info frame (0x02), received: 0x{:02X}", frame[4]);
    }

    let mut cell_voltages_v = Vec::with_capacity(16);
    let mut cell_min_v = f32::MAX;
    let mut cell_max_v = 0.0_f32;

    for i in 0..24 {
        let mv = read_u16_le(frame, 6 + i * 2);
        if mv > 500 && mv < 5000 {
            let v = (mv as f32) * 0.001;
            cell_voltages_v.push(v);
            if v < cell_min_v {
                cell_min_v = v;
            }
            if v > cell_max_v {
                cell_max_v = v;
            }
        }
    }

    if cell_voltages_v.is_empty() {
        cell_min_v = 0.0;
    }
    let cell_delta_mv = if !cell_voltages_v.is_empty() {
        (cell_max_v - cell_min_v) * 1000.0
    } else {
        0.0
    };

    let shift: usize = match protocol {
        JkProtocolVersion::Jk02_24S => 0,
        JkProtocolVersion::Jk02_32S => 32,
    };

    let mos_temp_c = (read_i16_le(frame, 112 + shift) as f32) * 0.1;
    let total_voltage_v = (read_u32_le(frame, 118 + shift) as f32) * 0.001;
    let current_a = (read_i32_le(frame, 126 + shift) as f32) * 0.001;
    let power_w = total_voltage_v * current_a;

    let temp1_c = (read_i16_le(frame, 130 + shift) as f32) * 0.1;
    let temp2_c = (read_i16_le(frame, 132 + shift) as f32) * 0.1;

    let balance_current_a = (read_i16_le(frame, 138 + shift) as f32) * 0.001;
    let balance_action = frame.get(140 + shift).copied().unwrap_or(0);
    let balance_active = balance_action != 0;

    let soc_percent = frame.get(141 + shift).copied().unwrap_or(0);
    let remaining_ah = (read_u32_le(frame, 142 + shift) as f32) * 0.001;
    let nominal_ah = (read_u32_le(frame, 146 + shift) as f32) * 0.001;
    let cycle_count = read_u32_le(frame, 150 + shift);

    Ok(JkBmsTelemetry {
        pack_id,
        name: name.to_string(),
        mac_address: mac_address.to_string(),
        total_voltage_v,
        current_a,
        power_w,
        soc_percent,
        remaining_ah,
        nominal_ah,
        cycle_count,
        cell_count: cell_voltages_v.len() as u8,
        cell_voltages_v,
        cell_min_v,
        cell_max_v,
        cell_delta_mv,
        balance_current_a,
        balance_active,
        mos_temp_c,
        temp1_c,
        temp2_c,
    })
}

// =========================================================================
// JK SMART BMS BLE AUTO-DISCOVERY & POLLING
// =========================================================================

#[derive(Clone, Debug)]
struct DiscoveredJkBms {
    addr: BLEAddress,
    name: String,
}

pub fn poll_all_jkbms_ble() -> Vec<JkBmsTelemetry> {
    let mut results = Vec::new();

    let res: Result<()> = block_on(async {
        let ble_device = BLEDevice::take();
        let mut ble_scan = BLEScan::new();
        let discovered: Arc<Mutex<Vec<DiscoveredJkBms>>> = Arc::new(Mutex::new(Vec::new()));
        let discovered_clone = discovered.clone();

        let jkbms_svc_uuid = BleUuid::from_uuid16(JKBMS_SERVICE_UUID16);

        info!("🔍 BLE Scan: Scanning for JK Smart BMS (Service 0xFFE0)...");
        ble_scan
            .active_scan(true)
            .interval(100)
            .window(99)
            .start(ble_device, 3000, move |device, data| {
                let is_jkbms_svc = data.is_advertising_service(&jkbms_svc_uuid);
                let dev_name = data
                    .name()
                    .map(|n| String::from_utf8_lossy(n).to_string())
                    .unwrap_or_default();

                let addr_str = format!("{}", device.addr());
                let is_configured_mac = (!APP_CONFIG.jkbms_pack1_mac.is_empty()
                    && addr_str.eq_ignore_ascii_case(APP_CONFIG.jkbms_pack1_mac))
                    || (!APP_CONFIG.jkbms_pack2_mac.is_empty()
                        && addr_str.eq_ignore_ascii_case(APP_CONFIG.jkbms_pack2_mac));

                if is_jkbms_svc || is_configured_mac || dev_name.starts_with("JK") {
                    if let Ok(mut list) = discovered_clone.lock() {
                        if !list.iter().any(|d| d.addr == device.addr()) {
                            let label = if dev_name.is_empty() {
                                format!("JK-BMS ({})", addr_str)
                            } else {
                                dev_name
                            };
                            info!("📡 Discovered JK-BMS: {} [MAC: {}]", label, addr_str);
                            list.push(DiscoveredJkBms {
                                addr: device.addr(),
                                name: label,
                            });
                        }
                    }
                }
                None::<()>
            })
            .await?;

        let mut targets = discovered.lock().unwrap().clone();
        if targets.is_empty() {
            info!("ℹ️ No JK Smart BMS detected in BLE range.");
            return Ok(());
        }

        targets.sort_by(|a, b| format!("{}", a.addr).cmp(&format!("{}", b.addr)));

        for (idx, target) in targets.iter().enumerate() {
            let pack_id = (idx + 1) as u8;
            let mac_str = format!("{}", target.addr);
            info!(
                "🔗 Connecting to Pack {} ({} [{}])...",
                pack_id, target.name, mac_str
            );

            let mut client = ble_device.new_client();
            if let Err(e) = client.connect(&target.addr).await {
                warn!("Failed to connect BLE to {}: {:?}", target.name, e);
                continue;
            }

            let svc_uuid = BleUuid::from_uuid16(JKBMS_SERVICE_UUID16);
            let chr_uuid = BleUuid::from_uuid16(JKBMS_CHAR_UUID16);

            let frame_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::with_capacity(320)));
            let complete_frame: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));

            let gatt_res: Result<()> = async {
                let service = client.get_service(svc_uuid).await?;
                let characteristic = service.get_characteristic(chr_uuid).await?;

                let buf_clone = frame_buf.clone();
                let done_clone = complete_frame.clone();

                characteristic
                    .on_notify(move |data| {
                        log::debug!("📥 Received BLE packet: {} bytes", data.len());
                        if let Ok(mut buf) = buf_clone.lock() {
                            if data.len() >= 4
                                && data[0] == 0x55
                                && data[1] == 0xAA
                                && data[2] == 0xEB
                                && data[3] == 0x90
                            {
                                buf.clear();
                            }
                            buf.extend_from_slice(data);

                            if buf.len() >= 300 && buf.get(4) == Some(&0x02) {
                                if let Ok(mut done) = done_clone.lock() {
                                    if done.is_none() {
                                        *done = Some(buf.clone());
                                    }
                                }
                            }
                        }
                    })
                    .subscribe_notify(true)
                    .await?;

                let cmd_info = build_jkbms_command(0x97);
                let _ = characteristic.write_value(&cmd_info, false).await;
                FreeRtos::delay_ms(150);

                let cmd_cell = build_jkbms_command(0x96);
                characteristic.write_value(&cmd_cell, false).await?;

                for _ in 0..30 {
                    FreeRtos::delay_ms(100);
                    if complete_frame.lock().unwrap().is_some() {
                        break;
                    }
                }

                Ok(())
            }
            .await;

            let _ = client.disconnect();

            if let Err(e) = gatt_res {
                warn!("Failed GATT communication with {}: {:?}", target.name, e);
                continue;
            }

            let raw_opt = complete_frame.lock().unwrap().take();
            if let Some(raw) = raw_opt {
                let parsed = parse_jkbms_cell_info_frame(
                    pack_id,
                    &target.name,
                    &mac_str,
                    &raw,
                    JkProtocolVersion::Jk02_32S,
                )
                .and_then(|t| {
                    if t.total_voltage_v > 10.0 && t.total_voltage_v < 70.0 {
                        Ok(t)
                    } else {
                        parse_jkbms_cell_info_frame(
                            pack_id,
                            &target.name,
                            &mac_str,
                            &raw,
                            JkProtocolVersion::Jk02_24S,
                        )
                    }
                });

                match parsed {
                    Ok(telemetry) => {
                        info!(
                            "🔋 BMS Pack {} ({}): {:.2}V | {:.1}A ({:.0}W) | SoC: {}% | {}S [Min: {:.3}V, Max: {:.3}V, Delta: {:.0}mV]",
                            telemetry.pack_id,
                            telemetry.name,
                            telemetry.total_voltage_v,
                            telemetry.current_a,
                            telemetry.power_w,
                            telemetry.soc_percent,
                            telemetry.cell_count,
                            telemetry.cell_min_v,
                            telemetry.cell_max_v,
                            telemetry.cell_delta_mv
                        );
                        results.push(telemetry);
                    }
                    Err(e) => warn!("Failed to parse JK-BMS data for {}: {:?}", target.name, e),
                }
            } else {
                warn!("Timeout waiting for 0x02 frame from JK-BMS {}", target.name);
            }
        }

        Ok(())
    });

    if let Err(e) = res {
        warn!("BLE Polling error: {:?}", e);
    }

    results
}
