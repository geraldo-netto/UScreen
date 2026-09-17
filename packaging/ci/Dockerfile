# Pinned Rust on Debian 12 keeps the release ABI at glibc 2.36.
FROM rust:1.90-bookworm@sha256:3914072ca0c3b8aad871db9169a651ccfce30cf58303e5d6f2db16d1d8a7e58f
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config git ca-certificates python3 \
    libdrm-dev libxkbcommon-dev libxkbcommon-x11-0 libegl1 libwayland-dev libxcb-render0-dev \
    libxcb-shape0-dev libxcb-xfixes0-dev libssl-dev \
    ffmpeg adb rpm fakeroot dpkg-dev binutils \
    xvfb xauth x11-utils dbus-x11 at-spi2-core libglib2.0-bin libgl1-mesa-dri \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch v1.15.0 https://github.com/DisplayLink/evdi /opt/evdi \
    && make -C /opt/evdi/library \
    && make -C /opt/evdi/library install PREFIX=/usr/local \
    && ldconfig
WORKDIR /work
