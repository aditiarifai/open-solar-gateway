# 📌 Hardware Wiring & Pinout Guide: ESP32-C3 SuperMini to MAX485 Module

This document is a visual pinout reference for wiring the **ESP32-C3 SuperMini** to the **MAX485 TTL-to-RS485 transceiver module** to monitor a **Growatt SPF 5000 ES** off-grid inverter.

---

## 1. ESP32-C3 SuperMini Physical Pinout
*(Position board with the **USB-C port facing UP**)*

```text
                     ┌───────────────────┐
                     │    [ USB-C ]      │
                     └───────────────────┘
   (LEFT Row)                                  (RIGHT Row)
┌─────────────────┐                         ┌─────────────────┐
│ Pin 1: 5V (VIN) │ ──> To VCC (MAX485) ⚠️  │ Pin 1: GPIO 5   │ ──> To DI (MAX485)
│ Pin 2: GND      │ ──> To GND (MAX485)     │ Pin 2: GPIO 6   │
│ Pin 3: 3V3      │                         │ Pin 3: GPIO 7   │
│ Pin 4: GPIO 4   │ ──> To RO (MAX485)      │ Pin 4: GPIO 8   │ (Internal Blue LED)
│ Pin 5: GPIO 3   │ ──> To DE + RE (Tied)   │ Pin 5: GPIO 9   │ (BOOT Button)
│ Pin 6: GPIO 2   │                         │ Pin 6: GPIO 10  │
│ Pin 7: GPIO 1   │                         │ Pin 7: GPIO 20  │
│ Pin 8: GPIO 0   │                         │ Pin 8: GPIO 21  │
└─────────────────┘                         └─────────────────┘
```

> 💡 **Tied DE + RE wire:**  
> The **DE and RE pins are jumped/tied together into a single wire** and connected to **Left Row, Pin 5 (GPIO 3)**.  
> In firmware, GPIO 3 is driven **HIGH** during Modbus transmission and switched to **LOW** to receive the inverter's reply.

---

## 2. MAX485 Module Wiring

The standard MAX485 module features 2 sides:

### TTL Header Side (to ESP32-C3):

#### Option 1: Using 3.3V <-> 5V Logic Level Shifter (Recommended for 24/7 reliability):
```text
  ESP32-C3 (3.3V)               Level Shifter (4-Ch)            MAX485 Module (5V)
┌─────────────────┐           ┌──────────────────────┐        ┌─────────────────┐
│ 3V3 (Left #3)   │ ────────> │ LV (3.3V Side)       │        │                 │
│ 5V/VIN (Left #1)│ ───────────────────────────────> │ HV ──> │ VCC (5V)        │
│ GND (Left #2)   │ ────────> │ GND ───────────────── GND ──> │ GND             │
│                 │           │                      │        │                 │
│ GPIO 4 (RX)     │ <──────── │ LV1 <─────────── HV1 │ <───── │ RO              │
│ GPIO 5 (TX)     │ ────────> │ LV2 ───────────> HV2 │ ─────> │ DI              │
│ GPIO 3 (DE+RE)  │ ────────> │ LV3 ───────────> HV3 │ ─────> │ DE + RE (Jumper)│
└─────────────────┘           └──────────────────────┘        └─────────────────┘
```

#### Option 2: Direct Connection (Testing / Prototype):
```text
  MAX485 Module (5V)                ESP32-C3 SuperMini (3.3V)
┌─────────────────┐               ┌───────────────────────────────────────┐
│      VCC        │ ───────────── │ Left Pin 1  (5V / VIN)                │
│      GND        │ ───────────── │ Left Pin 2  (GND)                     │
│      RO         │ ───────────── │ Left Pin 4  (GPIO 4 / RX) ⚠️          │
│    DE + RE      │ ───────────── │ Left Pin 5  (GPIO 3) 💡 Jumper wire   │
│      DI         │ ───────────── │ Right Pin 1 (GPIO 5 / TX)             │
└─────────────────┘               └───────────────────────────────────────┘
⚠️ Note for direct connection: The MAX485 RO pin outputs a 5V logic signal to ESP32 GPIO 4 (3.3V tolerant domain).
```

### Green Screw Terminal Side to Growatt Inverter (RJ45):
```text
  MAX485 Module (Green Terminal)      RJ45 Cable (Inverter RS485 Port)
┌──────────────────────────────┐     ┌────────────────────────────────────┐
│              A               │ ──> │ Pin 2 (Solid Orange Wire)          │
│              B               │ ──> │ Pin 1 (White-Orange Stripe Wire)   │
└──────────────────────────────┘     └────────────────────────────────────┘
```

---

## 3. Port Locations on Growatt SPF 5000 ES Bottom Panel

On the bottom connectivity panel of the Growatt inverter:
```text
           ┌──────────┐      ┌──────────┐
           │   BMS    │      │  RS485   │ <─── [ PLUG CABLE HERE! ]
           │  (LEFT)  │      │ (RIGHT)  │      (Inverter Modbus RS485 Port)
           └──────────┘      └──────────┘
             [ DO NOT ]         [ RIGHT ]
          (Disabled in USE)  (Always Active)
```

---

## 4. Pre-Flight Checklist:
1. [x] **Firmware:** ESP32-C3 flashed with Open Solar Gateway Rust firmware.
2. [x] **Inverter Settings:** Program 28 set to `001` (`Addr 1 028`), Program 05 set to `USE`.
3. [ ] **RJ45 Jack:** Connected to the **RIGHT port (`RS485`)**.
4. [ ] **Module Pins:**
   * `RO` -> GPIO 4 (Left #4)
   * `DI` -> GPIO 5 (Right #1)
   * `DE + RE (Tied)` -> GPIO 3 (Left #5)
   * `A` -> Pin 2 RJ45
   * `B` -> Pin 1 RJ45
