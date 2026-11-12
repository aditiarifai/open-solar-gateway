use log::{info, warn};
use std::net::UdpSocket;

// =========================================================================
// CAPTIVE DNS SERVER (UDP PORT 53) FOR SOFTAP SETUP PORTAL
// =========================================================================
// Automatically resolves all incoming DNS queries (e.g., 'http://solar',
// 'http://solar.local', or captive portal probes) to the ESP32 SoftAP IP.

pub struct CaptiveDnsServer;

impl CaptiveDnsServer {
    pub fn start(ip_octets: [u8; 4]) -> Self {
        std::thread::Builder::new()
            .stack_size(4096)
            .name("dns_server".into())
            .spawn(move || {
                let bind_addr = "0.0.0.0:53";
                let socket = match UdpSocket::bind(bind_addr) {
                    Ok(s) => {
                        info!(
                            "📡 Captive DNS Server active on UDP port 53 (Redirecting all queries to {}.{}.{}.{})",
                            ip_octets[0], ip_octets[1], ip_octets[2], ip_octets[3]
                        );
                        s
                    }
                    Err(e) => {
                        warn!("❌ Failed to bind DNS Server on UDP port 53: {:?}", e);
                        return;
                    }
                };

                let mut buf = [0u8; 512];

                loop {
                    let (len, src) = match socket.recv_from(&mut buf) {
                        Ok(res) => res,
                        Err(e) => {
                            warn!("DNS recv error: {:?}", e);
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            continue;
                        }
                    };

                    // DNS header must be at least 12 bytes
                    if len < 12 {
                        continue;
                    }

                    // Check QR bit (buf[2] & 0x80): must be 0 for standard query
                    let is_query = (buf[2] & 0x80) == 0;
                    let opcode = (buf[2] >> 3) & 0x0F;
                    if !is_query || opcode != 0 {
                        continue;
                    }

                    // Parse QNAME: starts at offset 12
                    let mut offset = 12;
                    while offset < len {
                        let label_len = buf[offset] as usize;
                        if label_len == 0 {
                            offset += 1;
                            break;
                        }
                        if (label_len & 0xC0) == 0xC0 {
                            offset += 2;
                            break;
                        }
                        offset += 1 + label_len;
                    }

                    // Must have 4 bytes for QTYPE (2) and QCLASS (2)
                    if offset + 4 > len {
                        continue;
                    }

                    let qtype = ((buf[offset] as u16) << 8) | (buf[offset + 1] as u16);
                    let question_end = offset + 4;

                    // Build response packet
                    let mut resp = Vec::with_capacity(question_end + 16);

                    // Copy Transaction ID (2 bytes)
                    resp.push(buf[0]);
                    resp.push(buf[1]);

                    // Flags: Standard query response, Authoritative, No error (0x8180)
                    resp.push(0x81);
                    resp.push(0x80);

                    // QDCOUNT: 1 (Question count)
                    resp.push(0x00);
                    resp.push(0x01);

                    // If query is for A record (type 1) or ANY (type 255):
                    let is_a_query = qtype == 1 || qtype == 255;
                    if is_a_query {
                        resp.push(0x00);
                        resp.push(0x01); // ANCOUNT = 1
                    } else {
                        resp.push(0x00);
                        resp.push(0x00); // ANCOUNT = 0 (e.g., AAAA query returns no answer, triggering IPv4 fallback)
                    }

                    // NSCOUNT: 0
                    resp.push(0x00);
                    resp.push(0x00);

                    // ARCOUNT: 0
                    resp.push(0x00);
                    resp.push(0x00);

                    // Copy original question section
                    resp.extend_from_slice(&buf[12..question_end]);

                    if is_a_query {
                        // Answer Section:
                        // Name compression pointer to offset 12 (0xC00C)
                        resp.push(0xc0);
                        resp.push(0x0c);
                        // TYPE: A (0x0001)
                        resp.push(0x00);
                        resp.push(0x01);
                        // CLASS: IN (0x0001)
                        resp.push(0x00);
                        resp.push(0x01);
                        // TTL: 60 seconds (0x0000003C)
                        resp.push(0x00);
                        resp.push(0x00);
                        resp.push(0x00);
                        resp.push(0x3c);
                        // RDLENGTH: 4 bytes
                        resp.push(0x00);
                        resp.push(0x04);
                        // RDATA: 4 bytes IPv4
                        resp.extend_from_slice(&ip_octets);
                    }

                    let _ = socket.send_to(&resp, src);
                }
            })
            .ok();

        CaptiveDnsServer
    }
}
