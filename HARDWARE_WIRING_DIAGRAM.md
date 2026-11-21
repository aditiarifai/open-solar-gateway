# 📌 Hardware Wiring & Pinout Guide: CORE-ESP32 (ESP32-C3) to MAX485 Module

This document is a visual pinout reference for wiring the **CORE-ESP32 (ESP32-C3 Dev Kit)** to the **MAX485 TTL-to-RS485 transceiver module** to monitor a **Growatt SPF 5000 ES** off-grid inverter.

---

## 1. CORE-ESP32 (ESP32-C3) Physical Pinout
*(Position board **facing BACK / Pin-Label side UP** with **Antenna end UP** and **USB-C port DOWN**, matching the silkscreen labels printed on the back of the PCB.)*

```text
                      ┌───────────────────┐
                      │  [ ANTENNA END ]  │
                      └───────────────────┘
   (LEFT Row - Back/Label View)                 (RIGHT Row - Back/Label View)
┌──────────────────────────────┐               ┌──────────────────────┐
│ Pin  1: GND                  │               │ Pin  1: GND          │
│ Pin  2: 3.3V                 │ ──> LV (3.3V) │ Pin  2: IO00         │
│ Pin  3: IO02                 │               │ Pin  3: IO01         │
│ Pin  4: IO03 (GPIO 3)        │ ──> DE + RE   │ Pin  4: IO12         │
│ Pin  5: IO10                 │               │ Pin  5: IO18 (USB D-)│
│ Pin  6: IO06 (GPIO 6)        │ ──> RO (RX)   │ Pin  6: IO19 (USB D+)│
│ Pin  7: IO07                 │               │ Pin  7: GND          │
│ Pin  8: IO11                 │               │ Pin  8: RX0 (GPIO20) │
│ Pin  9: GND                  │ ──> GND       │ Pin  9: TX0 (GPIO21) │
│ Pin 10: 3.3V                 │               │ Pin 10: IO13         │
│ Pin 11: IO05 (GPIO 5)        │ ──> DI (TX)   │ Pin 11: NC           │
│ Pin 12: IO04 (GPIO 4)        │               │ Pin 12: EN           │
│ Pin 13: IO08                 │               │ Pin 13: 3.3V         │
│ Pin 14: IO09 (BOOT)          │               │ Pin 14: GND          │
│ Pin 15: +5V                  │ ──> VCC (5V)  │ Pin 15: PWR          │
│ Pin 16: GND                  │               │ Pin 16: +5V          │
└──────────────────────────────┘               └──────────────────────┘
                      ┌───────────────────┐
                      │    [ USB-C ]      │
                      └───────────────────┘
```

*(Reference photos: [CORE-ESP32 Top](snapshots/core-esp32-c3-top.jpg) | [CORE-ESP32 Back Pinout](snapshots/core-esp32-c3-back-pinout.jpg) | [Level Shifter Module](snapshots/level-converter-mh.jpg) | [1-Ch BSS138 Schematic](snapshots/bss138-level-shifter-schematic.png))*

> 💡 **All needed pins are on ONE side!**  
> On the **CORE-ESP32** board, `+5V`, `3.3V`, `GND`, `IO03`, `IO05`, and `IO06` are all located on the **same row** (the **Left Row** when reading the pin labels on the back, or **Right Row** when looking from the top/component side).

> 💡 **Tied DE + RE wire:**  
> The **DE and RE pins are jumped/tied together into a single wire** and connected to **IO03 (GPIO 3)** (Left Row, Pin 4).  
> In firmware, GPIO 3 is driven **HIGH** during Modbus transmission and switched to **LOW** to receive the inverter's reply.

---

## 2. MAX485 Module Wiring

The standard MAX485 module features 2 sides:

### TTL Header Side (to CORE-ESP32):

#### Option 1: Using 1-Channel Discrete BSS138 Level Shifter on `RO` Only (Recommended):
*(Only `RO` outputs 5V into the ESP32-C3; `DI` and `DE + RE` are 3.3V outputs from ESP32-C3 which exceed MAX485's 2.0V `VIH` threshold, so they connect directly.)*
```text
  CORE-ESP32 (3.3V)            1-Ch BSS138 (LBSS138LT1G)         MAX485 Module (5V)
┌───────────────────┐         ┌───────────────────────────┐      ┌─────────────────┐
│ 3.3V (Left #2)    │ ──────> │ Gate (Pin 1) & 10kΩ to S  │      │                 │
│ +5V  (Left #15)   │ ──────┬─│> 10kΩ pull-up to D        │ ───> │ VCC (5V)        │
│ GND  (Left #9)    │ ──────┼──────────────────────────────────> │ GND             │
│                   │       │ │                           │      │                 │
│ IO06 / GPIO 6 (RX)│ <─────│─│ Source (Pin 2) ── Drain(3)│ <─── │ RO              │
│ IO05 / GPIO 5 (TX)│ ──────┼─┴───────────────────────────┼────> │ DI (Direct)     │
│ IO03 / GPIO 3     │ ──────┴─────────────────────────────┼────> │ DE + RE (Direct)│
└───────────────────┘                                     └──────┴─────────────────┘
```
*(See [1-Ch BSS138 Schematic](snapshots/bss138-level-shifter-schematic.png) for exact `LBSS138LT1G` + `R14/R15 10kΩ` connections.)*

#### Option 2: Direct Connection (Testing / Prototype):
```text
  MAX485 Module (5V)                CORE-ESP32 (3.3V)
┌─────────────────┐               ┌────────────────────────────────────────┐
│      VCC        │ ───────────── │ Left Pin 15 (+5V)                      │
│      GND        │ ───────────── │ Left Pin 9  (GND)                      │
│      RO         │ ───────────── │ Left Pin 6  (IO06 / GPIO 6 / RX) ⚠️    │
│    DE + RE      │ ───────────── │ Left Pin 4  (IO03 / GPIO 3) 💡 Jumper  │
│      DI         │ ───────────── │ Left Pin 11 (IO05 / GPIO 5 / TX)       │
└─────────────────┘               └────────────────────────────────────────┘
⚠️ Note for direct connection: The MAX485 RO pin outputs a 5V logic signal to ESP32 GPIO 6 (3.3V tolerant domain). Using a Level Shifter (or 1-ch BSS138 on RO) is safer for long-term 24/7 use.
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
4. [ ] **Module Pins (Back/Label View):**
   * `RO` -> `IO06` / GPIO 6 (Left #6)
   * `DI` -> `IO05` / GPIO 5 (Left #11)
   * `DE + RE (Tied)` -> `IO03` / GPIO 3 (Left #4)
   * `VCC` -> `+5V` (Left #15)
   * `GND` -> `GND` (Left #9)
   * `3.3V` (Level Shifter) -> `3.3V` (Left #2)
   * `A` -> Pin 2 RJ45
   * `B` -> Pin 1 RJ45
