#!/usr/bin/env python3

import base64
import json
import subprocess

modulus_out = subprocess.check_output([
    "openssl", "rsa",
    "-pubin",
    "-in", "/etc/mikrom/neon-auth/public.pem",
    "-modulus", "-noout"
], text=True).strip()
modulus_hex = modulus_out.split("=", 1)[1]
if modulus_hex.startswith("00"):
    modulus_hex = modulus_hex[2:]

n = base64.urlsafe_b64encode(bytes.fromhex(modulus_hex)).rstrip(b"=").decode()
e = base64.urlsafe_b64encode((65537).to_bytes(3, "big")).rstrip(b"=").decode()

jwks = {
    "keys": [
        {
            "kty": "RSA",
            "use": "sig",
            "alg": "RS256",
            "kid": "mikrom-neon-key",
            "n": n,
            "e": e,
        }
    ]
}

with open("/etc/mikrom/neon-auth/jwks.json", "w", encoding="utf-8") as f:
    json.dump(jwks, f, separators=(",", ":"))
