#!/usr/bin/env python3
"""
GeminiDesk Release Manifest & Ed25519 Signing Tool.

Usage:
  1) Generate an offline Ed25519 release signing keypair (keep private key ONLY in GitHub Secrets!):
       python scripts/release_manifest.py --generate-keypair

  2) Build `latest.json`, `SHA256SUMS.txt`, and (when UPDATER_SIGNING_PRIVATE_KEY is set)
     detached Ed25519 signature `latest.json.sig`:
       python scripts/release_manifest.py \
         --version 0.4.0 \
         --base-url https://github.com/geminidesk/geminidesk/releases/download/v0.4.0 \
         --dist-dir dist \
         --notes-file .github/RELEASE_NOTES_TEMPLATE.md

Supports Ed25519 via Python `cryptography` (or `openssl` CLI fallback) with zero hardcoded secrets.
"""

import argparse
import datetime
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ARTIFACT_RULES = [
    ("windows-x86_64", "-windows-x86_64-setup.exe", "nsis_installer"),
    ("windows-x86_64", "-x64-setup.exe", "nsis_installer"),
    ("linux-x86_64", "-linux-x86_64.AppImage", "linux_app_image"),
    ("linux-x86_64", "-linux-x86_64.tar.gz", "linux_tarball"),
    ("macos-aarch64", "-macos-aarch64.dmg", "macos_dmg"),
    ("macos-x86_64", "-macos-x86_64.dmg", "macos_dmg"),
]


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def generate_keypair() -> None:
    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
        from cryptography.hazmat.primitives.serialization import Encoding, NoEncryption, PrivateFormat, PublicFormat

        sk = Ed25519PrivateKey.generate()
        priv_hex = sk.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()).hex()
        pub_hex = sk.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()
        print("Generated Ed25519 updater keypair:")
        print(f"  GEMINIDESK_UPDATER_PUBKEY   (safe to embed in CI env): {pub_hex}")
        print(f"  UPDATER_SIGNING_PRIVATE_KEY (SECRET - store ONLY in GitHub Secrets): {priv_hex}")
    except ImportError:
        print("Install `cryptography` (`pip install cryptography`) to generate Ed25519 keypairs.", file=sys.stderr)
        sys.exit(1)


def sign_ed25519_hex(data: bytes, priv_hex: str) -> str:
    raw_seed = bytes.fromhex(priv_hex.strip())
    if len(raw_seed) != 32:
        raise ValueError("UPDATER_SIGNING_PRIVATE_KEY must be 64 hex chars (32-byte Ed25519 seed).")

    try:
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

        sk = Ed25519PrivateKey.from_private_bytes(raw_seed)
        return sk.sign(data).hex()
    except ImportError:
        # Fallback using OpenSSL DER PKCS#8 wrapper for a raw 32-byte Ed25519 seed.
        pkcs8_prefix = bytes.fromhex("302e020100300506032b657004220420")
        with tempfile.TemporaryDirectory() as td:
            key_der = Path(td) / "key.der"
            msg_path = Path(td) / "msg.bin"
            sig_path = Path(td) / "sig.bin"
            key_der.write_bytes(pkcs8_prefix + raw_seed)
            msg_path.write_bytes(data)
            subprocess.run(
                [
                    "openssl",
                    "pkeyutl",
                    "-sign",
                    "-inkey",
                    str(key_der),
                    "-keyform",
                    "DER",
                    "-rawin",
                    "-in",
                    str(msg_path),
                    "-out",
                    str(sig_path),
                ],
                check=True,
            )
            return sig_path.read_bytes().hex()


def main() -> None:
    parser = argparse.ArgumentParser(description="Build & sign GeminiDesk release metadata")
    parser.add_argument("--generate-keypair", action="store_true")
    parser.add_argument("--version", default="0.4.0")
    parser.add_argument("--base-url", default="")
    parser.add_argument("--dist-dir", default="dist")
    parser.add_argument("--notes-file", default="")
    parser.add_argument("--min-supported-version", default="0.3.0")
    args = parser.parse_args()

    if args.generate_keypair:
        generate_keypair()
        return

    dist_dir = Path(args.dist_dir)
    if not dist_dir.is_dir():
        print(f"dist directory not found: {dist_dir}", file=sys.stderr)
        sys.exit(1)

    all_files = sorted(
        p for p in dist_dir.iterdir()
        if p.is_file() and p.name not in ("latest.json", "latest.json.sig", "SHA256SUMS.txt")
    )

    sums_lines = []
    for f in all_files:
        sums_lines.append(f"{sha256_file(f)}  {f.name}")
    (dist_dir / "SHA256SUMS.txt").write_text("\n".join(sums_lines) + ("\n" if sums_lines else ""), encoding="utf-8")
    print(f"Wrote {dist_dir / 'SHA256SUMS.txt'} ({len(sums_lines)} files)")

    if not args.base_url:
        print("No --base-url specified; skipped latest.json generation.")
        return

    base_url = args.base_url.rstrip("/")
    if not base_url.startswith("https://"):
        print("ERROR: --base-url must use https://", file=sys.stderr)
        sys.exit(1)

    artifacts = {}
    for platform_key, suffix, kind in ARTIFACT_RULES:
        if platform_key in artifacts:
            continue
        for f in all_files:
            if f.name.endswith(suffix):
                artifacts[platform_key] = {
                    "url": f"{base_url}/{f.name}",
                    "sha256": sha256_file(f),
                    "size": f.stat().st_size,
                    "kind": kind,
                    "filename": f.name,
                }
                break

    release_notes = f"GeminiDesk v{args.version}"
    if args.notes_file and Path(args.notes_file).is_file():
        release_notes = Path(args.notes_file).read_text(encoding="utf-8").strip()[:4096]

    manifest = {
        "version": args.version.lstrip("v"),
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "release_notes": release_notes,
        "min_supported_version": args.min_supported_version,
        "artifacts": artifacts,
    }
    manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8")
    manifest_path = dist_dir / "latest.json"
    manifest_path.write_bytes(manifest_bytes)
    print(f"Wrote {manifest_path} ({len(artifacts)} platform artifacts)")

    priv_hex = os.environ.get("UPDATER_SIGNING_PRIVATE_KEY", "").strip()
    if priv_hex:
        sig_hex = sign_ed25519_hex(manifest_bytes, priv_hex)
        sig_path = dist_dir / "latest.json.sig"
        sig_path.write_text(sig_hex + "\n", encoding="utf-8")
        print(f"Signed {sig_path} with Ed25519")
    else:
        print("INFO: UPDATER_SIGNING_PRIVATE_KEY not set; latest.json left unsigned (draft/CI dry-run).")


if __name__ == "__main__":
    main()
