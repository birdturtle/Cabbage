#!/usr/bin/env python3
"""Sign the exact release downloads. The private key is supplied by CI."""
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sys


def b64(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=")


def main():
    version = sys.argv[1]
    folder = Path(sys.argv[2])
    platforms = {}
    for platform, label, extension in [("linux-x86_64", "Linux", "AppImage"), ("windows-x86_64", "Windows", "exe")]:
        name = f"Cabbage-{label}-x64-{version}.{extension}"
        data = (folder / name).read_bytes()
        platforms[platform] = {
            "url": f"https://github.com/birdturtle/Cabbage/releases/download/v{version}/{name}",
            "sha256": hashlib.sha256(data).hexdigest(),
            "size": len(data),
        }
    header = b64(json.dumps({"alg": "RS256", "typ": "JWT"}, separators=(",", ":")).encode())
    payload = b64(json.dumps({"version": version, "platforms": platforms}, separators=(",", ":")).encode())
    message = header + b"." + payload
    with tempfile.TemporaryDirectory() as tmp:
        key = Path(tmp) / "private.pem"
        key.write_text(os.environ["CABBAGE_UPDATE_PRIVATE_KEY"])
        key.chmod(0o600)
        public = subprocess.check_output(["openssl", "pkey", "-in", str(key), "-pubout"], stderr=subprocess.DEVNULL)
        expected = Path("src-tauri/updater-public.pem").read_bytes()
        if public.strip() != expected.strip():
            raise SystemExit("Release signing key does not match the launcher public key.")
        signature = subprocess.check_output(["openssl", "dgst", "-sha256", "-sign", str(key)], input=message, stderr=subprocess.DEVNULL)
        (folder / "cabbage-updater.jwt").write_bytes(message + b"." + b64(signature) + b"\n")


if __name__ == "__main__":
    main()
