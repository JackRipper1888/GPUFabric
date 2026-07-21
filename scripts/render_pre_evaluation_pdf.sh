#!/usr/bin/env bash
set -euo pipefail

require_env() {
    local name="$1"
    if [[ -z "${!name:-}" ]]; then
        printf 'missing required environment variable: %s\n' "$name" >&2
        exit 2
    fi
}

for command in curl sha256sum jq file; do
    command -v "$command" >/dev/null 2>&1 || {
        printf 'required command not found: %s\n' "$command" >&2
        exit 2
    }
done

for name in \
    GPUF_PRE_EVALUATION_API_URL \
    GPUF_PRE_EVALUATION_API_TOKEN \
    GPUF_PRE_EVALUATION_REPORT_ID; do
    require_env "$name"
done

renderer="${GPUF_PDF_RENDERER_BIN:-google-chrome-stable}"
if ! command -v "$renderer" >/dev/null 2>&1 && [[ ! -x "$renderer" ]]; then
    printf 'PDF renderer not found: %s\n' "$renderer" >&2
    exit 2
fi

output="${1:-${GPUF_PRE_EVALUATION_REPORT_ID}.pdf}"
output="$(realpath -m "$output")"
mkdir -p "$(dirname "$output")"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT
html_file="$tmp_dir/report.html"
headers_file="$tmp_dir/headers.txt"
request_headers_file="$tmp_dir/request-headers.txt"
printf 'Authorization: Bearer %s\n' "$GPUF_PRE_EVALUATION_API_TOKEN" >"$request_headers_file"
chmod 600 "$request_headers_file"

curl \
    --fail-with-body \
    --silent \
    --show-error \
    -D "$headers_file" \
    -H @"$request_headers_file" \
    -o "$html_file" \
    "${GPUF_PRE_EVALUATION_API_URL%/}/api/banking/provider/pre-evaluations/${GPUF_PRE_EVALUATION_REPORT_ID}/html"

declared_sha256="$(awk 'BEGIN {IGNORECASE=1} /^x-content-sha256:/ {gsub("\\r", "", $2); print tolower($2)}' "$headers_file" | tail -n 1)"
actual_sha256="$(sha256sum "$html_file" | awk '{print $1}')"
if [[ -z "$declared_sha256" || "$declared_sha256" != "$actual_sha256" ]]; then
    printf 'report HTML SHA-256 verification failed\n' >&2
    exit 1
fi

"$renderer" \
    --headless=new \
    --disable-gpu \
    --disable-background-networking \
    --disable-component-update \
    --disable-default-apps \
    --disable-sync \
    --metrics-recording-only \
    --no-first-run \
    '--host-resolver-rules=MAP * ~NOTFOUND' \
    --no-pdf-header-footer \
    --run-all-compositor-stages-before-draw \
    --user-data-dir="$tmp_dir/chrome" \
    --print-to-pdf="$output" \
    "file://$html_file" \
    >/dev/null 2>&1

if [[ ! -s "$output" ]] || [[ "$(head -c 5 "$output")" != '%PDF-' ]]; then
    printf 'renderer did not produce a valid PDF file\n' >&2
    exit 1
fi

pdf_sha256="$(sha256sum "$output" | awk '{print $1}')"
generated_at="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
sidecar="${output}.json"
jq -n \
    --arg schema_version 'gpuf.pre_evaluation_pdf.v1' \
    --arg hash_profile 'gpuf.pre-evaluation-pdf-bytes.v1' \
    --arg report_id "$GPUF_PRE_EVALUATION_REPORT_ID" \
    --arg source_html_sha256 "$actual_sha256" \
    --arg pdf_sha256 "$pdf_sha256" \
    --arg generated_at "$generated_at" \
    --arg renderer "$renderer" \
    '{schemaVersion:$schema_version,hashProfile:$hash_profile,reportId:$report_id,sourceHtmlSha256:$source_html_sha256,pdfSha256:$pdf_sha256,generatedAt:$generated_at,renderer:$renderer,signed:false}' \
    >"$sidecar"

jq -n \
    --arg output "$output" \
    --arg sidecar "$sidecar" \
    --arg source_html_sha256 "$actual_sha256" \
    --arg pdf_sha256 "$pdf_sha256" \
    '{pdf:$output,sidecar:$sidecar,sourceHtmlSha256:$source_html_sha256,pdfSha256:$pdf_sha256,signed:false}'
