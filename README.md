# Tock Bootloader + TockOS + NSIB Setup for nRF5340DK

This guide explains how to build, sign, and flash the Tock-based bootloader and TockOS firmware for the Nordic nRF5340DK development kit.

## 🛠 Troubleshooting

If you encounter issues while building the project, ensure your Rust environment is correctly set up. Run the following commands:

```sh
rustup update nightly
rustup override set nightly
rustup component add rust-src --toolchain nightly-x86_64-unknown-linux-gnu
rustup component add llvm-tools
```

---

## 📦 1. Build Second Stage Bootloader (Tock-Boot)

1. Navigate to the Tock-Bootloader directory:
   ```sh
   cd Firmware-Development/Rust/tock-bootloader-master/boards/nrf5340dk-bootloader/
   ```

2. Build the bootloader:
   ```sh
   make
   ```

3. Copy the generated HEX file:
   ```sh
   Firmware-Development/nRF-Connect/bootloader/tock-boot/
   ```

---

## ⚙️ 2. Build the Application (TockOS)

1. Navigate to the TockOS build directory:
   ```sh
   cd Firmware-Development/Rust/tock-bootloader-master/tock-nrf53foropenSK/boards/nordic/nrf5340dk/
   ```

2. Build the firmware:
   ```sh
   make
   ```

3. Copy the generated BIN file:
   ```sh
   Firmware-Development/nRF-Connect/bootloader/tock-boot/
   ```

---

## 🧱 3. Build First Stage Bootloader (NSIB)

1. Open the NSIB bootloader project in **nRF Connect for VS Code**:
   ```sh
   cd Firmware-Development/nRF-Connect/bootloader/
   ```

2. Build the project from VS Code.

3. Flash the NSIB bootloader to the board using J-Link.

---

## 🔏 4. Sign and Flash the Tock Bootloader

1. Navigate to the `tock-boot` directory:
   ```sh
   cd Firmware-Development/nRF-Connect/bootloader/tock-boot/
   ```

2. Sign the Tock-Boot hex:
   ```sh
   python3 sign_bootloader.py
   ```

3. Flash `provision.hex` and the signed bootloader:
   ```sh
   JLinkExe -device nrf5340_xxaa -if swd -speed 4000 -autoconnect 1
   loadfile provision.hex
   loadfile nrf5340dk-bootloader_signed.hex
   ```

> **Note:** LED1 will remain **solid ON** when the device is in bootloader mode.

---

## 🚀 5. Flash TockOS Firmware

Once the device is in bootloader mode, flash the TockOS application:

### Option A: Using DFU (USB)
```sh
sudo dfu-util -a 0 -s 0x00018000:leave -D nrf5340dk.bin
```

### Option B: Using UART
Connect the USB-TTL adapter to the following nRF5340DK pins:

- **TX (from USB-TTL)** → **P0.08 (RX on board)**
- **RX (from USB-TTL)** → **P0.06 (TX on board)**
- **GND (from USB-TTL)** → **GND**

Once connected, all `tockloader` commands will work over UART:
```sh
tockloader flash --address 0x18000 nrf5340dk.bin --serial
```

---

## 🔁 Re-Sign Only the Bootloader (No Reprovision)

If you only modified the Tock-Boot bootloader and don't want to regenerate keys or provision data:

```sh
python3 sign_bootloader.py --sign-only
```

Then flash only the updated signed bootloader:
```sh
JLinkExe -device nrf5340_xxaa -if swd -speed 4000 -autoconnect 1
loadfile nrf5340dk-bootloader_signed.hex
```

---

## ✅ Done

You're now ready to run TockOS on the nRF5340DK using your custom signed bootloader!
