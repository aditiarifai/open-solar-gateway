#!/usr/bin/env python3
"""
ESP32 Telemetry Fetcher Script (Open Solar Gateway)
Downloads historical 1-minute solar & battery telemetry records from ESP32 internal 1MB Flash via Serial Port (COM9).
"""

import serial
import time
import os
import datetime

PORT = "COM9"
BAUD = 115200

def fetch_data():
    print(f"Connecting to ESP32 on {PORT} at {BAUD} baud...")
    try:
        ser = serial.Serial()
        ser.port = PORT
        ser.baudrate = BAUD
        ser.timeout = 2
        ser.dtr = False
        ser.rts = False
        ser.open()
    except Exception as e:
        print(f"❌ Failed to open {PORT}: {e}")
        return

    # Allow USB connection to stabilize
    time.sleep(1.5)
    ser.reset_input_buffer()

    print("📤 Sending 'FETCH' command to ESP32...")
    ser.write(b"FETCH\n")
    ser.flush()

    csv_lines = []
    recording = False
    start_time = time.time()

    print("📥 Downloading telemetry data from 1MB Flash...")
    while time.time() - start_time < 30:
        raw = ser.readline()
        if not raw:
            continue
        try:
            line = raw.decode("utf-8", errors="replace").strip()
        except:
            continue

        if "=== CSV_START ===" in line:
            recording = True
            continue
        elif "=== CSV_END ===" in line:
            recording = False
            break

        if recording:
            # Skip ESP-IDF system logs if interleaved
            if line.startswith("\x1b") or line.startswith("I (") or line.startswith("W (") or line.startswith("E ("):
                continue
            if "," in line:
                csv_lines.append(line)

    ser.close()

    if not csv_lines:
        print("⚠️ No CSV data received or storage is empty.")
        return

    today_str = datetime.datetime.now().strftime("%Y-%m-%d_%H%M%S")
    filename = f"telemetry_export_{today_str}.csv"

    with open(filename, "w", encoding="utf-8") as f:
        for l in csv_lines:
            f.write(l + "\n")

    print(f"✅ Successfully downloaded {len(csv_lines)-1} telemetry record(s)!")
    print(f"💾 File saved to: {os.path.abspath(filename)}")

    # Calculate summary if data is present
    if len(csv_lines) > 1:
        header = csv_lines[0].split(",")
        rows = [l.split(",") for l in csv_lines[1:] if len(l.split(",")) == len(header)]

        try:
            pv_w = [float(r[1]) for r in rows]
            load_w = [float(r[3]) for r in rows]
            bat_v = [float(r[5]) for r in rows]
            soc = [int(r[6]) for r in rows]
            delta = [float(r[8]) for r in rows]
            kwh = [float(r[9]) for r in rows]

            print("\n" + "="*50)
            print("📊 TELEMETRY DATA SUMMARY:")
            print("="*50)
            print(f"☀️ Total Solar Production : {max(kwh) - min(kwh):.2f} kWh (Today's Peak: {max(kwh):.2f} kWh)")
            print(f"⚡ Peak Solar Power (PV)   : {max(pv_w):.0f} W (Average: {sum(pv_w)/len(pv_w):.0f} W)")
            print(f"🔌 AC Home Load            : Peak {max(load_w):.0f} W (Average: {sum(load_w)/len(load_w):.0f} W)")
            print(f"🔋 Battery Voltage Range   : {min(bat_v):.2f}V to {max(bat_v):.2f}V")
            print(f"🔋 Battery SoC Range       : {min(soc)}% to {max(soc)}%")
            print(f"⚖️ Max Cell Delta (BMS)    : {max(delta):.0f} mV (Balance Status: {'EXCELLENT' if max(delta) < 30 else 'CHECK REQUIRED'})")
            print("="*50)
        except Exception as e:
            print(f"Failed to calculate summary: {e}")

if __name__ == "__main__":
    fetch_data()
