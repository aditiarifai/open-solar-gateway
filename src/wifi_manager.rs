use anyhow::{Context, Result};
use embedded_svc::wifi::{AccessPointConfiguration, AuthMethod, ClientConfiguration, Configuration};
use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::handle::RawHandle;
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, EspNvsPartition, NvsDefault};
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};
use log::{info, warn};
use std::sync::{Arc, Mutex};

// =========================================================================
// WIFI MANAGER: NVS CREDENTIAL STORAGE, SOFTAP SETUP PORTAL & STA CLIENT
// =========================================================================

pub const SETUP_AP_SSID: &str = "OpenSolarGateway-Setup";
#[allow(dead_code)]
pub const SETUP_AP_PASS: &str = ""; // Open Wi-Fi for frictionless onboarding

#[derive(Clone, Debug, PartialEq)]
pub enum WifiModeStatus {
    ConnectedSta { ip: String, ssid: String },
    SoftApPortal { ip: String, ssid: String },
    Disconnected,
}

pub struct WifiManager {
    pub nvs: EspNvsPartition<NvsDefault>,
    pub status: Arc<Mutex<WifiModeStatus>>,
}

impl WifiManager {
    pub fn new(nvs: EspDefaultNvsPartition) -> Self {
        Self {
            nvs,
            status: Arc::new(Mutex::new(WifiModeStatus::Disconnected)),
        }
    }

    /// Read saved SSID & Password from NVS Flash
    pub fn load_credentials(&self) -> Option<(String, String)> {
        let nvs = match EspNvs::new(self.nvs.clone(), "wifi_store", false) {
            Ok(n) => n,
            Err(_) => return None,
        };

        let mut ssid_buf = [0u8; 64];
        let mut pass_buf = [0u8; 64];

        let ssid = match nvs.get_str("ssid", &mut ssid_buf) {
            Ok(Some(s)) if !s.is_empty() => s.to_string(),
            _ => return None,
        };

        let pass = match nvs.get_str("pass", &mut pass_buf) {
            Ok(Some(p)) => p.to_string(),
            _ => String::new(),
        };

        Some((ssid, pass))
    }

    /// Save SSID & Password to NVS Flash
    pub fn save_credentials(&self, ssid: &str, pass: &str) -> Result<()> {
        let mut nvs = EspNvs::new(self.nvs.clone(), "wifi_store", true)
            .context("Failed to open NVS for writing")?;
        nvs.set_str("ssid", ssid)?;
        nvs.set_str("pass", pass)?;
        info!("💾 Wi-Fi credentials saved to NVS (SSID: '{}')", ssid);
        Ok(())
    }

    /// Delete Wi-Fi credentials from NVS Flash
    pub fn clear_credentials(&self) -> Result<()> {
        let mut nvs = EspNvs::new(self.nvs.clone(), "wifi_store", true)
            .context("Failed to open NVS for clearing")?;
        nvs.set_str("ssid", "")?;
        nvs.set_str("pass", "")?;
        info!("🗑️ Wi-Fi credentials cleared from NVS.");
        Ok(())
    }

    /// Initialize Wi-Fi: Connect to STA (if configured), fallback to SoftAP if connection fails or unconfigured
    pub fn init_wifi(
        &self,
        modem: Modem,
        sys_loop: EspSystemEventLoop,
    ) -> Result<BlockingWifi<EspWifi<'static>>> {
        let esp_wifi = EspWifi::new(modem, sys_loop.clone(), Some(self.nvs.clone()))?;
        let mut wifi = BlockingWifi::wrap(esp_wifi, sys_loop)?;

        // Set local hostname 'solar' on station interface
        unsafe {
            let h = b"solar\0".as_ptr() as *const _;
            esp_idf_svc::sys::esp_netif_set_hostname(wifi.wifi().sta_netif().handle(), h);
        }

        // Check credentials in NVS or config.rs
        let creds = self.load_credentials().or_else(|| {
            if !crate::config::APP_CONFIG.wifi_ssid.is_empty() {
                Some((
                    crate::config::APP_CONFIG.wifi_ssid.to_string(),
                    crate::config::APP_CONFIG.wifi_pass.to_string(),
                ))
            } else {
                None
            }
        });

        let mut sta_connected = false;

        if let Some((ssid, pass)) = &creds {
            info!("📡 Connecting to home Wi-Fi: '{}'...", ssid);

            let client_config = ClientConfiguration {
                ssid: ssid.as_str().try_into().unwrap_or_default(),
                password: pass.as_str().try_into().unwrap_or_default(),
                ..Default::default()
            };

            wifi.set_configuration(&Configuration::Client(client_config))?;
            wifi.start()?;

            // Maximize TX power and disable power saving for optimal RF range
            unsafe {
                esp_idf_svc::sys::esp_wifi_set_ps(esp_idf_svc::sys::wifi_ps_type_t_WIFI_PS_NONE);
                esp_idf_svc::sys::esp_wifi_set_max_tx_power(84);
            }

            for attempt in 1..=3 {
                info!("Attempting connection to '{}' (Attempt {}/3)...", ssid, attempt);
                if wifi.connect().is_ok() && wifi.wait_netif_up().is_ok() {
                    let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
                    let ip_str = format!("{}", ip_info.ip);
                    info!("✅ Successfully connected to '{}'! IP: http://{}", ssid, ip_str);
                    *self.status.lock().unwrap() = WifiModeStatus::ConnectedSta {
                        ip: ip_str,
                        ssid: ssid.clone(),
                    };
                    sta_connected = true;
                    break;
                }
                esp_idf_svc::hal::delay::FreeRtos::delay_ms(1500);
            }
        }

        // If no credentials or connection to STA failed: Start SoftAP Setup Portal mode!
        if !sta_connected {
            warn!("⚠️ Client mode inactive -> Starting SoftAP Setup Portal ('{}')...", SETUP_AP_SSID);

            let ap_config = AccessPointConfiguration {
                ssid: SETUP_AP_SSID.try_into().unwrap(),
                channel: 1,
                auth_method: AuthMethod::None,
                ..Default::default()
            };

            wifi.set_configuration(&Configuration::AccessPoint(ap_config))?;
            wifi.start()?;

            unsafe {
                esp_idf_svc::sys::esp_wifi_set_ps(esp_idf_svc::sys::wifi_ps_type_t_WIFI_PS_NONE);
                // Lower TX power to 44 (11 dBm) to prevent voltage rail collapse on ESP32-C3 SuperMini
                esp_idf_svc::sys::esp_wifi_set_max_tx_power(44);
                let mut pwr: i8 = 0;
                esp_idf_svc::sys::esp_wifi_get_max_tx_power(&mut pwr);
                info!("Wi-Fi SoftAP TX power set to: {} (0.25 dBm units = {:.2} dBm)", pwr, pwr as f32 * 0.25);

                // Configure DHCP Server to advertise 192.168.71.1 as the DNS Server so 'http://solar' works out of the box
                let ap_handle = wifi.wifi().ap_netif().handle();
                esp_idf_svc::sys::esp_netif_dhcps_stop(ap_handle);
                let mut dns_info: esp_idf_svc::sys::esp_netif_dns_info_t = core::mem::zeroed();
                dns_info.ip.type_ = esp_idf_svc::sys::lwip_ip_addr_type_IPADDR_TYPE_V4 as u8;
                dns_info.ip.u_addr.ip4.addr = u32::from_ne_bytes([192, 168, 71, 1]);
                esp_idf_svc::sys::esp_netif_set_dns_info(
                    ap_handle,
                    esp_idf_svc::sys::esp_netif_dns_type_t_ESP_NETIF_DNS_MAIN,
                    &mut dns_info,
                );
                esp_idf_svc::sys::esp_netif_dhcps_start(ap_handle);
            }

            let ip_info = wifi.wifi().ap_netif().get_ip_info()?;
            let ip_str = format!("{}", ip_info.ip);
            info!("🌐 Web Setup Portal active on SoftAP '{}' | Open http://{} or http://solar in browser", SETUP_AP_SSID, ip_str);

            *self.status.lock().unwrap() = WifiModeStatus::SoftApPortal {
                ip: ip_str,
                ssid: SETUP_AP_SSID.to_string(),
            };
        }

        Ok(wifi)
    }
}
