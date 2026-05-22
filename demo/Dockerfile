FROM ghcr.io/astral-sh/uv:0.11.7 AS uv

FROM rust:1-slim-bookworm

COPY --from=uv /uv /uvx /usr/local/bin/

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        fonts-dejavu-core \
        procps \
        python3 \
        tmux \
    && rm -rf /var/lib/apt/lists/*

ENV CARGO_TARGET_DIR=/tmp/tmuxtui-target \
    TERM=xterm-256color \
    UV_LINK_MODE=copy

WORKDIR /work
