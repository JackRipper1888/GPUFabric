FROM golang@sha256:1699c10032ca2582ec89a24a1312d986a3f094aed3d5c1147b19880afe40e052 AS builder

WORKDIR /src
COPY go.mod ./
COPY cmd ./cmd
COPY internal ./internal
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/e2e-support ./cmd/support

FROM debian@sha256:3352c2e13876c8a5c5873ef20870e1939e73cb9a3c1aeba5e3e72172a85ce9ed

ARG APT_MIRROR=deb.debian.org
ENV DEBIAN_FRONTEND=noninteractive
RUN sed -i "s|http://deb.debian.org|http://${APT_MIRROR}|g" /etc/apt/sources.list.d/debian.sources \
    && apt-get -o Acquire::http::Pipeline-Depth=0 -o Acquire::http::Timeout=30 -o Acquire::Retries=5 update \
    && apt-get -o Acquire::http::Pipeline-Depth=0 -o Acquire::http::Timeout=30 -o Acquire::Retries=5 install -y --no-install-recommends \
        ca-certificates \
        chromium \
        clamav \
        clamav-daemon \
        curl \
        fonts-liberation \
        libsofthsm2 \
        opensc \
        poppler-utils \
        softhsm2 \
        tesseract-ocr \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/e2e-support /usr/local/bin/e2e-support
COPY deploy/clamd.conf /etc/clamav/clamd-local-e2e.conf
COPY deploy/local-e2e.hdb /opt/local-e2e-clamav/local-e2e.hdb

LABEL org.opencontainers.image.title="GPUFabric asset assessment local E2E support"
LABEL org.opencontainers.image.description="NON-PRODUCTION Chromium, SoftHSM2, ClamAV and OCR sidecar"

ENTRYPOINT ["/usr/local/bin/e2e-support"]
