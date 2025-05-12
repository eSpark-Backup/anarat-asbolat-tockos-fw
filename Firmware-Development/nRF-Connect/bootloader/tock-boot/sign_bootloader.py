#!/usr/bin/env python3

import subprocess
import sys
import argparse
from pathlib import Path

SYSTEM_PYTHON = "python3"  # Change if your system Python is elsewhere
SCRIPT_DIR = Path.home() / "ncs/v2.9.1/nrf/scripts/bootloader"

def run_cmd(command, **kwargs):
    print(f"[+] Running: {' '.join(str(c) for c in command)}")
    subprocess.run(command, check=True, **kwargs)

def generate_keys_and_provision():
    run_cmd(["imgtool", "keygen", "-t", "ecdsa-p256", "-k", "tock_priv.pem"])
    run_cmd(["imgtool", "getpub", "-k", "tock_priv.pem", "-e", "pem"], stdout=open("tock_pub.pem", "w"))

    run_cmd([
        SYSTEM_PYTHON, str(SCRIPT_DIR / "provision.py"),
        "--s0-addr", "0x8000",
        "--s1-addr", "0x84000",
        "--provision-addr", "0xff8100",
        "--public-key-files", "tock_pub.pem",
        "--output", "provision.hex",
        "--max-size", "0x280",
        "--num-counter-slots-version", "2",
        "--mcuboot-counters-slots", "2"
    ])

def sign_and_generate_validation():
    run_cmd([SYSTEM_PYTHON, str(SCRIPT_DIR / "hash.py"), "--in", "nrf5340dk-bootloader.hex"], stdout=open("tock.sha256", "w"))

    openssl = subprocess.Popen(
        ["openssl", "dgst", "-sha256", "-sign", "tock_priv.pem", "tock.sha256"],
        stdout=subprocess.PIPE
    )
    run_cmd(
        [SYSTEM_PYTHON, str(SCRIPT_DIR / "asn1parse.py"), "--alg", "ecdsa", "--contents", "signature"],
        stdin=openssl.stdout,
        stdout=open("tock.sig", "w")
    )
    openssl.wait()

    run_cmd([
        SYSTEM_PYTHON, str(SCRIPT_DIR / "validation_data.py"),
        "--input", "nrf5340dk-bootloader.hex",
        "-o", "nrf5340dk-bootloader_signed.hex",
        "--output-bin", "nrf5340dk-bootloader_signed.bin",
        "--offset", "0",
        "--signature", "tock.sig",
        "--public-key", "tock_pub.pem",
        "--magic-value", "0x281ee6de,0x86518483,0x13502"
    ])

def main():
    parser = argparse.ArgumentParser(description="Bootloader signing script for nRF5340.")
    parser.add_argument("--sign-only", action="store_true", help="Only sign and generate validation data, skip key/provision generation.")
    args = parser.parse_args()

    try:
        if not args.sign_only:
            generate_keys_and_provision()
        sign_and_generate_validation()
        print("[✓] All steps completed successfully.")

    except subprocess.CalledProcessError as e:
        print(f"[!] Command failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
