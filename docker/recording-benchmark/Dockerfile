FROM rust:1.92-slim-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    cmake \
    curl \
    ffmpeg \
    git \
    libasound2-dev \
    libclang-dev \
    libegl1 \
    libexpat1-dev \
    libfontconfig1-dev \
    libfreetype-dev \
    libgit2-dev \
    libssl-dev \
    libwayland-client0 \
    libwayland-egl1 \
    libx11-6 \
    libxcb1 \
    libxcursor1 \
    libxi6 \
    libxkbcommon-x11-0 \
    mesa-vulkan-drivers \
    pkg-config \
    protobuf-compiler \
    unzip \
    xauth \
    xvfb \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV CARGO_TARGET_DIR=/cargo-target
WORKDIR /workspace

COPY docker/recording-benchmark/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
