#!/usr/bin/env bash
# System packages the desktop app needs to build on Debian/Ubuntu: GPUI
# (windowing, fonts, Vulkan), GTK for the tray icon, FUSE for AppImage.
set -euo pipefail
apt-get update
apt-get install -y --no-install-recommends \
  pkg-config clang cmake \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-xcb-dev libxcb1-dev libxcb-xkb-dev \
  libfontconfig1-dev libfreetype-dev \
  libvulkan-dev libzstd-dev libssl-dev libdbus-1-dev \
  libgtk-3-dev libayatana-appindicator3-dev libxdo-dev libfuse2
