# ☀️ Open Solar Gateway (Rust Edition)
### *Universal ESP32-C3 Firmware for Solar Inverters, Smart BMS & Offline Telemetry*

[![Rust](https://img.shields.io/badge/Rust-ESP--IDF%20v5.2-orange.svg)](https://github.com/esp-rs/esp-idf-svc)
[![Target](https://img.shields.io/badge/Target-ESP32--C3%20(RISC--V)-blue.svg)](https://www.espressif.com/en/products/socs/esp32-c3)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

**Open Solar Gateway** is a standalone, open-source IoT gateway firmware built with **Rust (`esp-idf-svc`)** targeting the **ESP32-C3 (RISC-V)** microcontroller. It polls real-time telemetry from off-grid/hybrid solar inverters (Growatt SPF 5000 ES) via **RS485 Modbus RTU** and lithium battery packs (JK Smart BMS 16S–32S) via **Bluetooth Low Energy (BLE)**, records 1-minute historical data to internal **1MB SPI Flash (7-day circular buffer)**, and serves an embedded **web dashboard** alongside a zero-friction **Wi-Fi captive setup portal**.

---

## ✨ Features

1. **Zero Hardcoded Secrets (Safe for Open-Source):**
   * No hardcoded Wi-Fi credentials in source code.
   * Built-in **SoftAP Setup Portal** (`OpenSolarGateway-Setup`): Users connect to this hotspot from phone or laptop and configure home Wi-Fi via browser.
   * Credentials persist securely across reboots in **NVS Flash**.

2. **Local Domain Name Access (`http://solar`):**
   * Embedded hostname resolution: access the dashboard simply by browsing to **`http://solar`** or `http://192.168.71.1` without needing to look up the router's DHCP lease table.

3. **7-Day Offline Telemetry Buffer (1MB SPI Flash):**
   * Dedicated 1MB raw data partition formatted as a circular ring buffer.
   * Logs 1-minute aggregated records (solar PV power, AC load, battery voltage, current, SoC, individual cell voltages, and cumulative energy).
   * **Power-loss resilient:** Telemetry is preserved through grid blackouts or Wi-Fi outages.

4. **Automated Serial Data Exporter (`tools/fetch_telemetry.py`):**
   * One command dumps 7-day historical records over USB serial directly into **CSV (Excel-compatible)** format with automatic statistical summaries.

5. **Multi-Protocol & High Performance (Rust):**
   * **RS485 Modbus RTU:** Polling 52 registers from Growatt SPF 5000 ES every 5 seconds.
   * **Bluetooth BLE 5.0:** Auto-discovery of 1 to 2 JK-BMS packs (e.g. 2P16S configuration), extracting 16 individual cell voltages, MOSFET temps, and active balancing states.
   * **Memory-Safe:** Pure Rust implementation with zero memory leaks for uninterrupted 24/7 reliability.

---

## 🛠️ Hardware Wiring & Pinout

### ESP32-C3 SuperMini to MAX485 Transceiver Module
| MAX485 Pin | ESP32-C3 SuperMini Pin | Function / Description |
| :--- | :---: | :--- |
| **VCC** | **5V (VIN)** | 5V Module Supply |
| **GND** | **GND** | Common Ground |
| **RO** (Receiver Out) | **GPIO 4** | UART1 RX |
| **DE + RE (Tied Together)** | **GPIO 3** | Transceiver Direction (HIGH: TX, LOW: RX) |
| **DI** (Driver In) | **GPIO 5** | UART1 TX |

### Inverter RS485 Port Connection (RJ45 LAN Cable)
* Connect to the **`RS485`** port (the **RIGHT** RJ45 port under the Growatt SPF 5000 ES inverter):
  * **Pin 2 RJ45** (Solid Orange Wire) -> MAX485 Terminal **A**
  * **Pin 1 RJ45** (White-Orange Stripe Wire) -> MAX485 Terminal **B**

> Detailed visual pinout diagrams and level shifter options are available in [HARDWARE_WIRING_DIAGRAM.md](HARDWARE_WIRING_DIAGRAM.md).

---

## 🚀 Getting Started

### 1. Prerequisites & Environment Setup
Run the automated environment setup script in PowerShell:
```powershell
.\setup_env.ps1
```

### 2. Build & Flash
Compile the firmware and flash it to the ESP32-C3:
```powershell
# Build firmware binary
cargo build

# Flash firmware and partition table
espflash flash --port COMx --partition-table partitions.csv C:\esp-target\riscv32imc-esp-espidf\debug\open-solar-gateway
```

---

## 📱 Wi-Fi Setup & Onboarding

1. On first boot, the ESP32 broadcasts a setup hotspot:
   * **SSID:** `OpenSolarGateway-Setup`
   * **Password:** *(Open / None)*
2. Connect your phone or laptop to `OpenSolarGateway-Setup`.
3. Open a browser and navigate to:
   👉 **`http://solar`** or **`http://192.168.71.1`**
4. Enter your home Wi-Fi SSID and password, then click **"Save & Connect"**.
5. The ESP32 joins your home Wi-Fi network and makes its real-time dashboard accessible at:
   👉 **`http://solar`**

---

## 📥 Offline Telemetry Export via Serial

If the ESP32 is deployed in an isolated location without Wi-Fi, you can retrieve historical logs over USB:
```powershell
python tools/fetch_telemetry.py
```
A timestamped `.csv` file will be generated in your working directory alongside an instant performance report.

---

## 📄 License
This project is licensed under the **MIT License**. Free for commercial and non-commercial use, modification, and distribution.
