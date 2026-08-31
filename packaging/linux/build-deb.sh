#!/usr/bin/env bash
# Copyright (C) 2026 Gaultier HUBERT
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

# Usage: ./build-deb.sh <version> <arch> <binary-path> <outdir>
VERSION="${1:?version}"
ARCH="${2:?arch}"
BINARY="${3:?binary}"
OUTDIR="${4:?outdir}"

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

PKG="hecate-lampad-proxmox_${VERSION}_${ARCH}"
DEST="$STAGE/$PKG"

mkdir -p "$DEST/DEBIAN" "$DEST/usr/bin" "$DEST/usr/lib/systemd/system"
install -m 0755 "$BINARY" "$DEST/usr/bin/hecate-lampad-proxmox"
install -m 0644 "$ROOT/packaging/linux/systemd/hecate-lampad-proxmox.service" \
  "$DEST/usr/lib/systemd/system/hecate-lampad-proxmox.service"

cat >"$DEST/DEBIAN/control" <<EOF
Package: hecate-lampad-proxmox
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Maintainer: Hecate Contributors
Recommends: hecate-lampad
Enhances: hecate-lampad
Description: Hecate Lampad Proxmox VM console helper
 Provides authenticated local IPC for restricted Proxmox QEMU console access.
EOF

cat >"$DEST/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if ! getent group hecate-ipc >/dev/null 2>&1; then
  groupadd --system hecate-ipc 2>/dev/null || true
fi
if ! id hecate-lampad-proxmox >/dev/null 2>&1; then
  useradd --system --home-dir /var/lib/hecate-lampad-proxmox --shell /usr/sbin/nologin --gid hecate-ipc hecate-lampad-proxmox 2>/dev/null || true
fi
usermod -a -G hecate-ipc hecate-lampad-proxmox 2>/dev/null || true
if id hecate-lampad >/dev/null 2>&1; then
  usermod -a -G hecate-ipc hecate-lampad 2>/dev/null || true
fi
if [ -f /etc/hecate-lampad/pve-token ]; then
  chown hecate-lampad-proxmox:hecate-ipc /etc/hecate-lampad/pve-token 2>/dev/null || true
  chmod 0600 /etc/hecate-lampad/pve-token 2>/dev/null || true
fi
if command -v systemctl >/dev/null 2>&1; then
  systemctl daemon-reload >/dev/null 2>&1 || true
  systemctl enable hecate-lampad-proxmox.service >/dev/null 2>&1 || true
  systemctl restart hecate-lampad-proxmox.service >/dev/null 2>&1 \
    || systemctl start hecate-lampad-proxmox.service >/dev/null 2>&1 \
    || true
fi
EOF
chmod 0755 "$DEST/DEBIAN/postinst"

mkdir -p "$OUTDIR"
dpkg-deb --build "$DEST" "$OUTDIR/${PKG}.deb"
echo "built $OUTDIR/${PKG}.deb"
