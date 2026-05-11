#!/bin/bash
set -e

# 1. Build the binary
echo "Building mikrom-router..."
cargo build --release -p mikrom-router

# 2. Copy the binary to the debian structure
echo "Copying binary to debian structure..."
cp ../target/release/mikrom-router debian/usr/bin/

# 3. Set permissions
chmod 755 debian/usr/bin/mikrom-router
chmod 755 debian/DEBIAN/postinst

# 4. Build the package
echo "Building debian package..."
dpkg-deb --root-owner-group --build debian mikrom-router.deb

echo "Done! mikrom-router.deb created."
