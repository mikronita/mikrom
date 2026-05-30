#!/usr/bin/env python3

import base64
import json
import subprocess

pub = subprocess.check_output([
    "openssl", "pkey", "-pubin",
    "-in", "/etc/mikrom/neon-auth/public.pem",
    "-text", "-noout"
], text=True)

# Extract modulus and exponent from the OpenSSL text output
lines = [line.strip() for line in pub.splitlines()]
modulus_hex = []
capture = False
for line in lines:
    if line.startswith("Modulus:"):
        capture = True
        continue
    if capture:
        if line.startswith("Exponent:"):
            exponent = 65537
            break
        modulus_hex.append(line.replace(":", "").replace(" ", ""))
modulus_hex = "".join(modulus_hex)
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
