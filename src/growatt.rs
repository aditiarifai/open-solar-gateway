use serde::{Deserialize, Serialize};
use crate::jkbms::JkBmsTelemetry;

// =========================================================================
// SERDE JSON TELEMETRY DATA STRUCTURES FOR CLOUD & CLIENTS
// =========================================================================

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SolarData {
    pub pv_voltage_v: f32,
    pub pv_power_w: f32,
    pub energy_today_kwh: f32,
    pub energy_total_kwh: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatteryData {
    pub voltage_v: f32,
    pub soc_percent: u8,
    pub power_w: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LoadData {
    pub voltage_v: f32,
    pub frequency_hz: f32,
    pub power_w: f32,
    pub apparent_power_va: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GridData {
    pub voltage_v: f32,
    pub frequency_hz: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ThermalData {
    pub inverter_temp_c: f32,
    pub dcdc_temp_c: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GrowattTelemetry {
    pub device_id: String,
    pub uptime_sec: u64,
    pub status_code: u16,
    pub status: String,
    pub valid: bool,
    pub solar: SolarData,
    pub battery: BatteryData,
    pub bms_packs: Vec<JkBmsTelemetry>,
    pub load: LoadData,
    pub grid: GridData,
    pub thermal: ThermalData,
}

// Growatt SPF 5000 ES inverter status description
pub fn get_status_string(code: u16) -> String {
    match code {
        0 => "Standby".into(),
        1 => "Initial / Self-Test".into(),
        2 => "Battery Discharge (Inverter Mode)".into(),
        3 => "Fault / Error".into(),
        4 => "Flash / Updating".into(),
        5 => "PV Charging Only".into(),
        6 => "AC Grid Charging Only".into(),
        7 => "Combined Charging (PV + Grid)".into(),
        8 => "Combined Charging + Bypass Grid".into(),
        9 => "PV Charge + Bypass Grid".into(),
        10 => "AC Charge + Bypass Grid".into(),
        11 => "Bypass Grid Mode (Utility)".into(),
        12 => "PV Charge & Discharge".into(),
        _ => format!("Unknown ({})", code),
    }
}

// =========================================================================
// MODBUS RTU OVER RS485 PROTOCOL (FUNCTION CODE 0x04 - READ INPUT REGISTERS)
// =========================================================================

// Compute standard Modbus RTU CRC16
pub fn calculate_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if (crc & 0x0001) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

// Build Modbus request frame (Function Code 0x04)
pub fn build_read_input_registers_frame(slave_id: u8, start_addr: u16, count: u16) -> [u8; 8] {
    let mut frame = [0u8; 8];
    frame[0] = slave_id;
    frame[1] = 0x04; // Read Input Registers
    frame[2] = (start_addr >> 8) as u8;
    frame[3] = (start_addr & 0xFF) as u8;
    frame[4] = (count >> 8) as u8;
    frame[5] = (count & 0xFF) as u8;

    let crc = calculate_crc16(&frame[0..6]);
    frame[6] = (crc & 0xFF) as u8;        // CRC Low
    frame[7] = ((crc >> 8) & 0xFF) as u8; // CRC High
    frame
}

// Helper to parse 16-bit register value (Big Endian)
#[inline]
pub fn get_reg_u16(payload: &[u8], reg_index: usize) -> u16 {
    let offset = reg_index * 2;
    if offset + 1 < payload.len() {
        ((payload[offset] as u16) << 8) | (payload[offset + 1] as u16)
    } else {
        0
    }
}

// Helper to parse 32-bit register value (2 consecutive registers)
#[inline]
pub fn get_reg_u32(payload: &[u8], high_index: usize, low_index: usize) -> u32 {
    let high = get_reg_u16(payload, high_index) as u32;
    let low = get_reg_u16(payload, low_index) as u32;
    (high << 16) | low
}
