use esp_idf_svc::sys::{
    esp_partition_erase_range, esp_partition_find_first, esp_partition_read,
    esp_partition_t, esp_partition_type_t_ESP_PARTITION_TYPE_DATA, esp_partition_write,
};
use log::info;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

// =========================================================================
// SOLAR + BMS RECORD DATA STRUCTURE (1-MINUTE RESOLUTION WITH INDIVIDUAL CELL VOLTAGES)
// =========================================================================

pub const FLASH_KIND_MINUTE: u8 = 1;
pub const RAM_RING_MINUTES: usize = 360; // Last 6 hours in RAM (360 min x 100 bytes ≈ 36 KB)

#[repr(C, packed)]
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct LocalSolarRecord {
    pub kind: u8,                   // 1 = 1-Minute Sample
    pub flags: u8,                  // 0x01 = UTC Synced
    pub crc16: u16,                 // CRC16 Checksum
    pub timestamp_epoch: u32,       // UNIX Timestamp (seconds)

    // --- SOLAR PV & AC LOAD (GROWATT) ---
    pub pv_energy_today_wh: u32,    // Today's cumulative PV energy (Wh)
    pub load_energy_today_wh: u32,  // Today's cumulative AC load energy (Wh)
    pub pv_power_w: u16,            // Average 1-min PV power (W)
    pub pv_voltage_cv: u16,         // PV voltage (0.01V)
    pub load_power_w: u16,          // Average 1-min AC load power (W)
    pub ac_voltage_cv: u16,         // AC 220V voltage (0.01V)
    pub inv_temp_c: i8,             // Inverter temperature (°C)

    // --- GLOBAL BATTERY STATUS ---
    pub active_packs: u8,           // Detected battery packs (1 or 2)
    pub bat_voltage_cv: u16,        // Inverter battery terminal voltage (0.01V)

    // --- PACK 1 (16S JK-BMS) ---
    pub p1_total_voltage_cv: u16,   // Pack 1 total voltage (0.01V)
    pub p1_current_ca: i16,         // Pack 1 current (0.01A)
    pub p1_soc: u8,                 // Pack 1 SoC (%)
    pub p1_cell_delta_mv: u16,      // Pack 1 max-min cell delta (mV)
    pub p1_mos_temp_c: i8,          // BMS 1 MOSFET temperature (°C)
    pub p1_bat_temp_c: i8,          // Battery 1 probe temperature (°C)
    pub p1_cells_mv: [u16; 16],     // Individual cell voltages 1..16 (mV)

    // --- PACK 2 (16S JK-BMS - If present) ---
    pub p2_total_voltage_cv: u16,   // Pack 2 total voltage (0.01V)
    pub p2_current_ca: i16,         // Pack 2 current (0.01A)
    pub p2_soc: u8,                 // Pack 2 SoC (%)
    pub p2_cell_delta_mv: u16,      // Pack 2 max-min cell delta (mV)
    pub p2_mos_temp_c: i8,          // BMS 2 MOSFET temperature (°C)
    pub p2_bat_temp_c: i8,          // Battery 2 probe temperature (°C)
    pub p2_cells_mv: [u16; 16],     // Individual cell voltages 1..16 (mV)

    pub sample_count: u8,           // Number of samples aggregated in 1 min (~12 samples)
}

impl Default for LocalSolarRecord {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

// =========================================================================
// 1-MINUTE ACCUMULATOR (IN RAM)
// =========================================================================

#[derive(Default, Clone)]
pub struct MinuteAccumulator {
    pub sample_count: u32,

    pub pv_power_sum: u64,
    pub pv_volt_sum: u64,
    pub load_power_sum: u64,
    pub ac_volt_sum: u64,
    pub inv_temp_last: i8,
    pub pv_energy_wh: u32,
    pub load_energy_wh: u32,

    pub bat_volt_sum: u64,
    pub active_packs: u8,

    // Pack 1
    pub p1_volt_sum: u64,
    pub p1_curr_sum: i64,
    pub p1_soc_last: u8,
    pub p1_delta_sum: u64,
    pub p1_mos_temp_last: i8,
    pub p1_bat_temp_last: i8,
    pub p1_cells_last: [u16; 16],

    // Pack 2
    pub p2_volt_sum: u64,
    pub p2_curr_sum: i64,
    pub p2_soc_last: u8,
    pub p2_delta_sum: u64,
    pub p2_mos_temp_last: i8,
    pub p2_bat_temp_last: i8,
    pub p2_cells_last: [u16; 16],
}

// =========================================================================
// TELEMETRY STORAGE MANAGER (RAM RING BUFFER + 1MB FLASH RING BUFFER)
// =========================================================================

pub struct LocalTelemetryManager {
    // RAM Ring Buffer (last 6 hours, 1 record per minute)
    pub ring_buffer: Vec<LocalSolarRecord>,
    pub ring_head: usize,
    pub ring_count: usize,

    // Running 1-minute accumulator
    pub current_minute: MinuteAccumulator,

    // Flash partition tracking (1 MB = 10,000+ records = 7+ days)
    pub part_ptr: *const esp_partition_t,
    pub part_write_offset: usize,
    pub part_size: usize,
    pub part_found: bool,
}

unsafe impl Send for LocalTelemetryManager {}

impl LocalTelemetryManager {
    pub fn new() -> Self {
        Self {
            ring_buffer: vec![LocalSolarRecord::default(); RAM_RING_MINUTES],
            ring_head: 0,
            ring_count: 0,
            current_minute: MinuteAccumulator::default(),
            part_ptr: std::ptr::null(),
            part_write_offset: 0,
            part_size: 1024 * 1024, // 1 MB
            part_found: false,
        }
    }

    /// Find 1MB 'telemetry' flash partition and scan for last write offset
    pub fn init_partition(&mut self) {
        unsafe {
            let label = b"telemetry\0".as_ptr();
            let part = esp_partition_find_first(
                esp_partition_type_t_ESP_PARTITION_TYPE_DATA,
                0xff, // ESP_PARTITION_SUBTYPE_ANY
                label,
            );
            if !part.is_null() {
                self.part_ptr = part;
                self.part_size = (*part).size as usize;
                self.part_found = true;

                // Find last write offset (first unwritten record slot marked 0xFF)
                let rec_size = std::mem::size_of::<LocalSolarRecord>();
                let mut check_buf = [0u8; 4];
                let mut found_offset = 0;

                while found_offset + rec_size <= self.part_size {
                    if esp_partition_read(part, found_offset, check_buf.as_mut_ptr() as *mut _, 4) == 0 {
                        // If first 4 bytes are 0xFF, this slot is unwritten
                        if check_buf == [0xFF, 0xFF, 0xFF, 0xFF] {
                            break;
                        }
                    }
                    found_offset += rec_size;
                }

                self.part_write_offset = if found_offset >= self.part_size { 0 } else { found_offset };

                info!(
                    "💾 1MB Flash 'telemetry' active! Size: {} KB (Write Offset: {})",
                    self.part_size / 1024,
                    self.part_write_offset
                );
            } else {
                info!("ℹ️ 'telemetry' partition not found -> Operating in RAM Ring Buffer only.");
            }
        }
    }

    /// Ingest 5-second sample from RS485 and BMS into 1-minute accumulator
    pub fn ingest_sample(
        &mut self,
        pv_w: f32,
        pv_v: f32,
        load_w: f32,
        ac_v: f32,
        inv_temp: f32,
        et_today: f32,
        bat_v: f32,
        bms_packs: &[crate::jkbms::JkBmsTelemetry],
    ) {
        let acc = &mut self.current_minute;
        acc.sample_count += 1;

        let pw = pv_w.max(0.0) as u16;
        let lw = load_w.max(0.0) as u16;

        acc.pv_power_sum += pw as u64;
        acc.load_power_sum += lw as u64;
        acc.pv_volt_sum += (pv_v * 100.0).max(0.0) as u64;
        acc.ac_volt_sum += (ac_v * 100.0).max(0.0) as u64;
        acc.bat_volt_sum += (bat_v * 100.0).max(0.0) as u64;
        acc.inv_temp_last = inv_temp as i8;
        acc.pv_energy_wh = (et_today * 1000.0).max(0.0) as u32;

        acc.active_packs = bms_packs.len() as u8;

        if let Some(p1) = bms_packs.iter().find(|p| p.pack_id == 1) {
            acc.p1_volt_sum += (p1.total_voltage_v * 100.0).max(0.0) as u64;
            acc.p1_curr_sum += (p1.current_a * 100.0) as i64;
            acc.p1_soc_last = p1.soc_percent;
            acc.p1_delta_sum += p1.cell_delta_mv.max(0.0) as u64;
            acc.p1_mos_temp_last = p1.mos_temp_c as i8;
            acc.p1_bat_temp_last = p1.temp1_c as i8;

            for (idx, &v) in p1.cell_voltages_v.iter().take(16).enumerate() {
                acc.p1_cells_last[idx] = (v * 1000.0) as u16;
            }
        }

        if let Some(p2) = bms_packs.iter().find(|p| p.pack_id == 2) {
            acc.p2_volt_sum += (p2.total_voltage_v * 100.0).max(0.0) as u64;
            acc.p2_curr_sum += (p2.current_a * 100.0) as i64;
            acc.p2_soc_last = p2.soc_percent;
            acc.p2_delta_sum += p2.cell_delta_mv.max(0.0) as u64;
            acc.p2_mos_temp_last = p2.mos_temp_c as i8;
            acc.p2_bat_temp_last = p2.temp1_c as i8;

            for (idx, &v) in p2.cell_voltages_v.iter().take(16).enumerate() {
                acc.p2_cells_last[idx] = (v * 1000.0) as u16;
            }
        }
    }

    /// Finalize 1-minute record: push to RAM Ring Buffer + persist to 1MB Flash
    pub fn finalize_minute(&mut self, epoch_sec: u32) {
        let acc = &self.current_minute;
        if acc.sample_count == 0 {
            return;
        }

        let n = acc.sample_count as u64;
        let mut rec = LocalSolarRecord {
            kind: FLASH_KIND_MINUTE,
            flags: 0x01,
            crc16: 0,
            timestamp_epoch: epoch_sec,
            pv_energy_today_wh: acc.pv_energy_wh,
            load_energy_today_wh: acc.load_energy_wh,
            pv_power_w: (acc.pv_power_sum / n) as u16,
            pv_voltage_cv: (acc.pv_volt_sum / n) as u16,
            load_power_w: (acc.load_power_sum / n) as u16,
            ac_voltage_cv: (acc.ac_volt_sum / n) as u16,
            inv_temp_c: acc.inv_temp_last,
            active_packs: acc.active_packs,
            bat_voltage_cv: (acc.bat_volt_sum / n) as u16,

            p1_total_voltage_cv: if acc.active_packs >= 1 { (acc.p1_volt_sum / n) as u16 } else { 0 },
            p1_current_ca: if acc.active_packs >= 1 { (acc.p1_curr_sum / (n as i64)) as i16 } else { 0 },
            p1_soc: acc.p1_soc_last,
            p1_cell_delta_mv: if acc.active_packs >= 1 { (acc.p1_delta_sum / n) as u16 } else { 0 },
            p1_mos_temp_c: acc.p1_mos_temp_last,
            p1_bat_temp_c: acc.p1_bat_temp_last,
            p1_cells_mv: acc.p1_cells_last,

            p2_total_voltage_cv: if acc.active_packs >= 2 { (acc.p2_volt_sum / n) as u16 } else { 0 },
            p2_current_ca: if acc.active_packs >= 2 { (acc.p2_curr_sum / (n as i64)) as i16 } else { 0 },
            p2_soc: acc.p2_soc_last,
            p2_cell_delta_mv: if acc.active_packs >= 2 { (acc.p2_delta_sum / n) as u16 } else { 0 },
            p2_mos_temp_c: acc.p2_mos_temp_last,
            p2_bat_temp_c: acc.p2_bat_temp_last,
            p2_cells_mv: acc.p2_cells_last,

            sample_count: acc.sample_count.min(255) as u8,
        };

        // Compute CRC16
        rec.crc16 = crate::growatt::calculate_crc16(unsafe {
            std::slice::from_raw_parts(
                (&rec as *const _ as *const u8).add(4),
                std::mem::size_of::<LocalSolarRecord>() - 4,
            )
        });

        // 1. Store in RAM Ring Buffer
        self.ring_buffer[self.ring_head] = rec;
        self.ring_head = (self.ring_head + 1) % RAM_RING_MINUTES;
        if self.ring_count < RAM_RING_MINUTES {
            self.ring_count += 1;
        }

        // 2. Write to 1MB Flash (Sector Erase & Ring Append circular storage)
        if self.part_found && !self.part_ptr.is_null() {
            unsafe {
                let rec_size = std::mem::size_of::<LocalSolarRecord>();
                let sector_size = (*self.part_ptr).erase_size as usize;

                if self.part_write_offset + rec_size > self.part_size {
                    self.part_write_offset = 0; // Wrap around (7-day circular buffer)
                }

                // Erase 4KB sector only when starting at sector boundary
                if (self.part_write_offset % sector_size) == 0 {
                    esp_partition_erase_range(self.part_ptr, self.part_write_offset, sector_size);
                }

                esp_partition_write(
                    self.part_ptr,
                    self.part_write_offset,
                    &rec as *const _ as *const _,
                    rec_size,
                );

                self.part_write_offset += rec_size;
            }
        }

        let pv_w = rec.pv_power_w;
        let load_w = rec.load_power_w;
        let p1_v = (rec.p1_total_voltage_cv as f32) * 0.01;
        let p1_soc = rec.p1_soc;
        let p1_delta = rec.p1_cell_delta_mv;

        info!(
            "⏱️ [1-Min Log] PV: {}W | Load: {}W | Bat: {:.2}V (SoC: {}%, Delta: {}mV)",
            pv_w, load_w, p1_v, p1_soc, p1_delta
        );

        // Reset accumulator for next minute
        self.current_minute = MinuteAccumulator::default();
    }

    pub fn get_history(&self, limit: usize) -> Vec<LocalSolarRecord> {
        let count = self.ring_count.min(limit);
        let mut list = Vec::with_capacity(count);

        for i in 0..count {
            let idx = (self.ring_head + RAM_RING_MINUTES - count + i) % RAM_RING_MINUTES;
            list.push(self.ring_buffer[idx]);
        }
        list
    }

    /// Dump historical data (up to 7 days) from 1MB Flash to Serial in CSV format
    pub fn dump_csv(&self) {
        println!("=== CSV_START ===");
        println!("epoch_sec,pv_w,pv_v,load_w,ac_v,bat_v,p1_soc,p1_curr_a,p1_delta_mv,pv_kwh,inv_temp_c");

        let rec_size = std::mem::size_of::<LocalSolarRecord>();
        if self.part_found && !self.part_ptr.is_null() {
            let mut buf = vec![0u8; rec_size];
            let mut offset = 0;
            let mut valid_count = 0;

            while offset + rec_size <= self.part_size {
                unsafe {
                    if esp_partition_read(self.part_ptr, offset, buf.as_mut_ptr() as *mut _, rec_size) == 0 {
                        let rec: LocalSolarRecord = *(buf.as_ptr() as *const LocalSolarRecord);
                        if rec.kind == FLASH_KIND_MINUTE && rec.timestamp_epoch > 0 {
                            valid_count += 1;
                            let ts = rec.timestamp_epoch;
                            let pw = rec.pv_power_w;
                            let pv = (rec.pv_voltage_cv as f32) * 0.01;
                            let lw = rec.load_power_w;
                            let av = (rec.ac_voltage_cv as f32) * 0.01;
                            let bv = (rec.bat_voltage_cv as f32) * 0.01;
                            let soc = rec.p1_soc;
                            let ca = (rec.p1_current_ca as f32) * 0.01;
                            let dm = rec.p1_cell_delta_mv;
                            let kwh = (rec.pv_energy_today_wh as f32) * 0.001;
                            let temp = rec.inv_temp_c;

                            println!(
                                "{},{},{:.1},{},{:.1},{:.2},{},{:.2},{},{:.2},{}",
                                ts, pw, pv, lw, av, bv, soc, ca, dm, kwh, temp
                            );
                        }
                    }
                }
                offset += rec_size;
            }

            // If flash is still empty (fresh first boot), dump from RAM Ring Buffer
            if valid_count == 0 {
                for rec in self.get_history(RAM_RING_MINUTES) {
                    if rec.kind == FLASH_KIND_MINUTE {
                        valid_count += 1;
                        let ts = rec.timestamp_epoch;
                        let pw = rec.pv_power_w;
                        let pv = (rec.pv_voltage_cv as f32) * 0.01;
                        let lw = rec.load_power_w;
                        let av = (rec.ac_voltage_cv as f32) * 0.01;
                        let bv = (rec.bat_voltage_cv as f32) * 0.01;
                        let soc = rec.p1_soc;
                        let ca = (rec.p1_current_ca as f32) * 0.01;
                        let dm = rec.p1_cell_delta_mv;
                        let kwh = (rec.pv_energy_today_wh as f32) * 0.001;
                        let temp = rec.inv_temp_c;

                        println!(
                            "{},{},{:.1},{},{:.1},{:.2},{},{:.2},{},{:.2},{}",
                            ts, pw, pv, lw, av, bv, soc, ca, dm, kwh, temp
                        );
                    }
                }
            }
            info!("📊 Successfully dumped {} records", valid_count);
        } else {
            for rec in self.get_history(RAM_RING_MINUTES) {
                if rec.kind == FLASH_KIND_MINUTE {
                    let ts = rec.timestamp_epoch;
                    let pw = rec.pv_power_w;
                    let pv = (rec.pv_voltage_cv as f32) * 0.01;
                    let lw = rec.load_power_w;
                    let av = (rec.ac_voltage_cv as f32) * 0.01;
                    let bv = (rec.bat_voltage_cv as f32) * 0.01;
                    let soc = rec.p1_soc;
                    let ca = (rec.p1_current_ca as f32) * 0.01;
                    let dm = rec.p1_cell_delta_mv;
                    let kwh = (rec.pv_energy_today_wh as f32) * 0.001;
                    let temp = rec.inv_temp_c;

                    println!(
                        "{},{},{:.1},{},{:.1},{:.2},{},{:.2},{},{:.2},{}",
                        ts, pw, pv, lw, av, bv, soc, ca, dm, kwh, temp
                    );
                }
            }
        }
        println!("=== CSV_END ===");
    }
}

pub type SharedTelemetryManager = Arc<Mutex<LocalTelemetryManager>>;
