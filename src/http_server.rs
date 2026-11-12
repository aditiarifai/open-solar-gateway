use anyhow::Result;
use embedded_svc::http::Method;
use embedded_svc::io::Write;
use esp_idf_svc::http::server::{Configuration as HttpServerConfig, EspHttpServer};
use log::{info, warn};
use std::sync::{Arc, Mutex};

use crate::growatt::GrowattTelemetry;
use crate::local_telemetry::SharedTelemetryManager;
use crate::wifi_manager::{WifiManager, WifiModeStatus};

// =========================================================================
// EMBEDDED HTTP SERVER FOR LOCAL MONITORING (DASHBOARD + WIFI SETUP)
// =========================================================================

pub fn start_http_server(
    shared_telemetry: SharedTelemetryManager,
    latest_telemetry: Arc<Mutex<Option<GrowattTelemetry>>>,
    wifi_mgr: Arc<WifiManager>,
    ip_addr: String,
) -> Result<EspHttpServer<'static>> {
    let server_config = HttpServerConfig {
        stack_size: 10240,
        uri_match_wildcard: true,
        ..Default::default()
    };

    let mut server = EspHttpServer::new(&server_config)?;

    // 1. Endpoint REST API: /api/live (Realtime telemetry per second)
    let latest_clone = latest_telemetry.clone();
    server.fn_handler::<anyhow::Error, _>("/api/live", Method::Get, move |req| {
        let live_data = latest_clone.lock().unwrap().clone();
        let json_body = match live_data {
            Some(data) => serde_json::to_string(&data)?,
            None => r#"{"status":"waiting_for_data"}"#.to_string(),
        };

        let mut resp = req.into_ok_response()?;
        resp.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n")?;
        resp.write_all(json_body.as_bytes())?;
        Ok(())
    })?;

    // 2. Endpoint REST API: /api/history (7-day hourly telemetry buffer)
    let history_clone = shared_telemetry.clone();
    server.fn_handler::<anyhow::Error, _>("/api/history", Method::Get, move |req| {
        let history = history_clone.lock().unwrap().get_history(168);
        let json_body = serde_json::to_string(&history)?;

        let mut resp = req.into_ok_response()?;
        resp.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n")?;
        resp.write_all(json_body.as_bytes())?;
        Ok(())
    })?;

    // 3. Endpoint API: Save Wi-Fi Credentials & Restart
    let wifi_mgr_save = wifi_mgr.clone();
    server.fn_handler::<anyhow::Error, _>("/api/save_wifi", Method::Post, move |mut req| {
        let mut buf = [0u8; 256];
        let bytes_read = req.read(&mut buf).unwrap_or(0);
        let body_str = std::str::from_utf8(&buf[..bytes_read]).unwrap_or("");

        // Parse JSON payload: {"ssid":"...","pass":"..."}
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(body_str);
        let (ssid, pass) = match parsed {
            Ok(val) => (
                val["ssid"].as_str().unwrap_or("").to_string(),
                val["pass"].as_str().unwrap_or("").to_string(),
            ),
            Err(_) => ("".to_string(), "".to_string()),
        };

        let mut resp = req.into_ok_response()?;
        if !ssid.is_empty() {
            if let Err(e) = wifi_mgr_save.save_credentials(&ssid, &pass) {
                warn!("Failed to save credentials: {:?}", e);
            }
            resp.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"message\":\"Saved! ESP32 will reconnect to your Wi-Fi...\"}")?;

            // Schedule device restart after 1.5 seconds
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                unsafe { esp_idf_svc::sys::esp_restart(); }
            });
        } else {
            resp.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\":\"error\",\"message\":\"SSID cannot be empty\"}")?;
        }
        Ok(())
    })?;

    // 4. Endpoint API: Reset / Clear Wi-Fi Credentials
    let wifi_mgr_reset = wifi_mgr.clone();
    server.fn_handler::<anyhow::Error, _>("/api/reset_wifi", Method::Post, move |req| {
        let _ = wifi_mgr_reset.clear_credentials();
        let mut resp = req.into_ok_response()?;
        resp.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"status\":\"ok\",\"message\":\"Wi-Fi credentials reset. Restarting in Setup Mode...\"}")?;

        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            unsafe { esp_idf_svc::sys::esp_restart(); }
        });
        Ok(())
    })?;

    // 5. Wi-Fi Setup Portal Page: /setup
    server.fn_handler::<anyhow::Error, _>("/setup", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    // 5b. Captive Portal Probes (Windows NCSI/WISPr, Android, Apple)
    // Directly serve the Wi-Fi setup HTML page on Windows redirect so the popup window works immediately!
    server.fn_handler::<anyhow::Error, _>("/redirect*", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler::<anyhow::Error, _>("/connecttest.txt*", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler::<anyhow::Error, _>("/ncsi.txt*", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler::<anyhow::Error, _>("/generate_204*", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    server.fn_handler::<anyhow::Error, _>("/hotspot-detect.html*", Method::Get, move |req| {
        let html = render_wifi_setup_html();
        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    // 6. Main Root Endpoint: / (Dashboard or Setup Portal if still in SoftAP mode)
    let latest_dash = latest_telemetry.clone();
    let history_dash = shared_telemetry.clone();
    let wifi_mgr_root = wifi_mgr.clone();
    let ip_str = ip_addr.clone();

    server.fn_handler::<anyhow::Error, _>("/", Method::Get, move |req| {
        let status = wifi_mgr_root.status.lock().unwrap().clone();

        // If in SoftAP mode (not connected to home router yet), auto-render the Setup Portal!
        if let WifiModeStatus::SoftApPortal { .. } = status {
            let html = render_wifi_setup_html();
            let mut resp = req.into_ok_response()?;
            resp.write_all(html.as_bytes())?;
            return Ok(());
        }

        let live_data = latest_dash.lock().unwrap().clone();
        let hist_count = history_dash.lock().unwrap().ring_count;

        let mut html = String::with_capacity(2048);
        html.push_str("<!DOCTYPE html><html><head><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'>");
        html.push_str("<title>Open Solar Gateway - Dashboard</title>");
        html.push_str("<style>body{font-family:sans-serif;margin:15px;background:#121212;color:#eee;}");
        html.push_str(".card{background:#1e1e1e;border-radius:10px;padding:15px;margin-bottom:15px;border:1px solid #333;}");
        html.push_str("h1{font-size:20px;color:#4caf50;}h2{font-size:16px;color:#ff9800;border-bottom:1px solid #333;padding-bottom:5px;}");
        html.push_str(".val{font-size:22px;font-weight:bold;color:#4fc3f7;}.grid{display:grid;grid-template-columns:1fr 1fr;gap:10px;}");
        html.push_str("table{width:100%;border-collapse:collapse;margin-top:10px;}th,td{border:1px solid #333;padding:6px;text-align:center;font-size:12px;}");
        html.push_str("th{background:#2a2a2a;color:#aaa;}a{color:#4fc3f7;text-decoration:none;}.btn{display:inline-block;padding:6px 12px;background:#333;color:#eee;border-radius:6px;font-size:12px;margin-left:8px;}</style></head><body>");

        html.push_str("<div style='display:flex;justify-content:space-between;align-items:center;'>");
        html.push_str("<h1>☀️ Open Solar Gateway</h1>");
        html.push_str("<a href='/setup' class='btn'>⚙️ Wi-Fi Setup</a>");
        html.push_str("</div>");

        html.push_str(&format!(
            "<p style='color:#888;font-size:12px;'>Access: <b>http://solar.local</b> | IP: {} | History: {} Hours | <a href='/api/live' target='_blank'>JSON Live</a> | <a href='/api/history' target='_blank'>JSON History</a></p>",
            ip_str, hist_count
        ));

        if let Some(data) = live_data {
            // Inverter Card
            html.push_str("<div class='card'><h2>Growatt SPF 5000 ES</h2><div class='grid'>");
            html.push_str(&format!("<div><small>Solar PV</small><div class='val'>{:.0} W</div><small>{:.1} V</small></div>", data.solar.pv_power_w, data.solar.pv_voltage_v));
            html.push_str(&format!("<div><small>Home Load</small><div class='val'>{:.0} W</div><small>{:.1} V</small></div>", data.load.power_w, data.load.voltage_v));
            html.push_str(&format!("<div><small>Energy Today</small><div class='val'>{:.2} kWh</div></div>", data.solar.energy_today_kwh));
            html.push_str(&format!("<div><small>Inverter Status</small><div class='val' style='font-size:14px;color:#81c784;'>{}</div></div>", data.status));
            html.push_str("</div></div>");

            // BMS Cards
            for bms in &data.bms_packs {
                html.push_str(&format!("<div class='card'><h2>Battery Pack {} ({})</h2><div class='grid'>", bms.pack_id, bms.name));
                html.push_str(&format!("<div><small>Pack Voltage</small><div class='val'>{:.2} V</div></div>", bms.total_voltage_v));
                html.push_str(&format!("<div><small>Current / Power</small><div class='val'>{:.1} A</div><small>{:.0} W</small></div>", bms.current_a, bms.power_w));
                html.push_str(&format!("<div><small>Capacity SoC</small><div class='val'>{:.0}%</div><small>{:.1} Ah</small></div>", bms.soc_percent, bms.remaining_ah));
                html.push_str(&format!("<div><small>Cell Delta (Balance)</small><div class='val'>{:.0} mV</div><small>Temp: {:.1}&deg;C</small></div>", bms.cell_delta_mv, bms.temp1_c));
                html.push_str("</div>");

                // 16-Cell Voltage Table
                html.push_str("<table><tr><th>Cell</th>");
                for i in 1..=16 {
                    html.push_str(&format!("<th>#{}</th>", i));
                }
                html.push_str("</tr><tr><td>Volt</td>");
                for v in &bms.cell_voltages_v {
                    html.push_str(&format!("<td>{:.3}</td>", v));
                }
                html.push_str("</tr></table></div>");
            }
        } else {
            html.push_str("<div class='card'><p>⏳ Waiting for initial telemetry from RS485 / BLE...</p></div>");
        }

        html.push_str("<script>setTimeout(() => location.reload(), 5000);</script></body></html>");

        let mut resp = req.into_ok_response()?;
        resp.write_all(html.as_bytes())?;
        Ok(())
    })?;

    info!("🌐 Embedded HTTP Server ready on port 80!");
    Ok(server)
}

/// Helper: Render HTML Wi-Fi Setup Portal
fn render_wifi_setup_html() -> String {
    let mut h = String::with_capacity(2048);
    h.push_str("<!DOCTYPE html><html><head><meta charset='utf-8'><meta name='viewport' content='width=device-width,initial-scale=1'>");
    h.push_str("<title>Open Solar Gateway - Setup Wi-Fi</title>");
    h.push_str("<style>body{font-family:sans-serif;margin:20px;background:#121212;color:#eee;max-width:480px;margin:auto;}");
    h.push_str(".card{background:#1e1e1e;border-radius:12px;padding:20px;border:1px solid #333;margin-top:20px;}");
    h.push_str("h1{font-size:20px;color:#4caf50;margin-bottom:5px;}p{font-size:13px;color:#aaa;}");
    h.push_str("label{font-size:12px;color:#aaa;display:block;margin-top:12px;}");
    h.push_str("input{width:100%;padding:10px;margin-top:4px;border-radius:8px;border:1px solid #444;background:#2a2a2a;color:#fff;box-sizing:border-box;}");
    h.push_str(".btn-save{width:100%;padding:12px;background:#4caf50;color:#fff;border:none;border-radius:8px;font-weight:bold;margin-top:20px;cursor:pointer;}");
    h.push_str(".btn-reset{width:100%;padding:10px;background:#d32f2f;color:#fff;border:none;border-radius:8px;font-size:12px;margin-top:10px;cursor:pointer;}");
    h.push_str(".status{margin-top:15px;padding:10px;border-radius:8px;font-size:13px;display:none;}</style></head><body>");

    h.push_str("<h1>⚙️ Wi-Fi Setup Portal</h1>");
    h.push_str("<p>Connect Open Solar Gateway to your home Wi-Fi network. Once saved, access the dashboard at <b>http://solar.local</b>.</p>");

    h.push_str("<div class='card'>");
    h.push_str("<label>Home Wi-Fi Network (SSID):</label>");
    h.push_str("<input type='text' id='ssid' placeholder='e.g., MyHomeWiFi'>");

    h.push_str("<label>Wi-Fi Password:</label>");
    h.push_str("<input type='password' id='pass' placeholder='Enter password'>");

    h.push_str("<button class='btn-save' onclick='saveWifi()'>💾 Save & Connect</button>");
    h.push_str("<button class='btn-reset' onclick='resetWifi()'>🗑️ Reset Wi-Fi Credentials</button>");
    h.push_str("<div id='msg' class='status'></div>");
    h.push_str("</div>");

    h.push_str("<script>");
    h.push_str("function saveWifi() {");
    h.push_str("  const s = document.getElementById('ssid').value.trim();");
    h.push_str("  const p = document.getElementById('pass').value;");
    h.push_str("  const m = document.getElementById('msg');");
    h.push_str("  if(!s){ alert('Wi-Fi SSID cannot be empty!'); return; }");
    h.push_str("  m.style.display='block'; m.style.background='#333'; m.innerText='⏳ Saving to NVS & connecting...';");
    h.push_str("  fetch('/api/save_wifi', { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({ssid:s, pass:p}) })");
    h.push_str("  .then(r=>r.json()).then(d=>{ m.style.background='#2e7d32'; m.innerText=d.message; })");
    h.push_str("  .catch(e=>{ m.style.background='#c62828'; m.innerText='Failed to communicate with device'; });");
    h.push_str("}");
    h.push_str("function resetWifi() {");
    h.push_str("  if(!confirm('Reset Wi-Fi configuration?')) return;");
    h.push_str("  fetch('/api/reset_wifi', { method:'POST' }).then(()=>alert('Wi-Fi credentials reset! Gateway returning to Setup mode.')).catch(()=>alert('Done.'));");
    h.push_str("}");
    h.push_str("</script></body></html>");

    h
}
