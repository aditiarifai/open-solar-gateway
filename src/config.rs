// =========================================================================
// ESP32-C3 OPEN SOLAR GATEWAY CONFIGURATION (RUST)
// =========================================================================

#[allow(dead_code)]
pub struct Config {
    pub wifi_ssid: &'static str,
    pub wifi_pass: &'static str,

    // Pinout ESP32-C3 SuperMini for MAX485 Transceiver Module:
    // DI -> GPIO 5 (TX), DE+RE -> GPIO 3 (tied together physically), RO -> GPIO 4 (RX)
    pub pin_uart_rx: u32,
    pub pin_uart_tx: u32,
    pub pin_rs485_dere: u32,

    // Modbus Settings (Growatt SPF 5000 ES)
    pub modbus_slave_id: u8,    // 1 (matches Inverter Program 28: Addr 1 028)
    pub modbus_baudrate: u32,   // 9600 baud (8N1)
    pub polling_interval_ms: u64,

    // JK Smart BMS BLE Settings (2P16S: Pack 1 & Pack 2)
    // Leave "" for Auto-Discovery of all nearby JK-BMS devices (Service 0xFFE0)!
    pub enable_jkbms_ble: bool,
    pub jkbms_poll_interval_sec: u64,
    pub jkbms_pack1_mac: &'static str, // Optional: MAC to pin Pack 1 order
    pub jkbms_pack2_mac: &'static str, // Optional: MAC to pin Pack 2 order

    // Cloud VPS Endpoint (MQTT - Optional)
    pub enable_mqtt: bool,
    pub mqtt_server: &'static str,
    pub mqtt_port: u16,
    pub mqtt_user: &'static str,
    pub mqtt_pass: &'static str,
    pub mqtt_topic_telemetry: &'static str,
}

pub const APP_CONFIG: Config = Config {
    // Leave empty to use credentials from NVS Flash / Web Setup Portal (Safe for Git/GitHub)
    wifi_ssid: "",
    wifi_pass: "",

    // Pinout ESP32-C3 SuperMini
    pin_uart_rx: 4,
    pin_uart_tx: 5,
    pin_rs485_dere: 3,

    modbus_slave_id: 1,
    modbus_baudrate: 9600,
    polling_interval_ms: 5000, // Poll Growatt RS485 every 5 seconds

    // JK Smart BMS Bluetooth BLE Auto-Scan & Read
    enable_jkbms_ble: true,
    jkbms_poll_interval_sec: 30, // Poll BLE JK-BMS every 30s (then disconnect so mobile apps can connect)
    jkbms_pack1_mac: "",         // Auto-detect all JK-BMS with Service UUID 0xFFE0
    jkbms_pack2_mac: "",

    enable_mqtt: false,
    mqtt_server: "your-vps-or-broker.com",
    mqtt_port: 1883,
    mqtt_user: "mqtt_user",
    mqtt_pass: "mqtt_pass",
    mqtt_topic_telemetry: "solar/gateway/telemetry",
};
