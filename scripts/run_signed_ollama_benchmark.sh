#!/usr/bin/env bash
set -euo pipefail

require_env() {
    local name="$1"
    if [[ -z "${!name:-}" ]]; then
        printf 'missing required environment variable: %s\n' "$name" >&2
        exit 2
    fi
}

for command in curl jq openssl sha256sum date stat; do
    command -v "$command" >/dev/null 2>&1 || {
        printf 'required command not found: %s\n' "$command" >&2
        exit 2
    }
done

for name in \
    GPUF_BENCHMARK_API_URL \
    GPUF_BENCHMARK_PRODUCER_TOKEN \
    GPUF_BENCHMARK_PRIVATE_KEY \
    GPUF_BENCHMARK_KEY_ID \
    GPUF_BENCHMARK_SOURCE_REF \
    GPUF_BENCHMARK_TARGET_URL \
    GPUF_BENCHMARK_MODEL; do
    require_env "$name"
done

if [[ ! "$GPUF_BENCHMARK_SOURCE_REF" =~ ^[0-9a-fA-F]{64}$ ]]; then
    printf 'GPUF_BENCHMARK_SOURCE_REF must be a 64-character SHA-256 value\n' >&2
    exit 2
fi
if [[ ! -r "$GPUF_BENCHMARK_PRIVATE_KEY" ]]; then
    printf 'benchmark private key is not readable\n' >&2
    exit 2
fi
if [[ -L "$GPUF_BENCHMARK_PRIVATE_KEY" || ! -f "$GPUF_BENCHMARK_PRIVATE_KEY" ]]; then
    printf 'benchmark private key must be a regular non-symlink file\n' >&2
    exit 2
fi
private_key_mode="$(stat -c '%a' "$GPUF_BENCHMARK_PRIVATE_KEY")"
if (( (8#$private_key_mode & 077) != 0 )); then
    printf 'benchmark private key must not be accessible by group or other users\n' >&2
    exit 2
fi
if [[ ! "$GPUF_BENCHMARK_KEY_ID" =~ ^[A-Za-z0-9._:-]{1,64}$ ]]; then
    printf 'GPUF_BENCHMARK_KEY_ID has an invalid format\n' >&2
    exit 2
fi

trials="${GPUF_BENCHMARK_TRIALS:-3}"
num_predict="${GPUF_BENCHMARK_NUM_PREDICT:-128}"
if [[ ! "$trials" =~ ^[1-9][0-9]*$ ]] || (( trials < 3 || trials > 10 )); then
    printf 'GPUF_BENCHMARK_TRIALS must be between 3 and 10\n' >&2
    exit 2
fi
if [[ ! "$num_predict" =~ ^[1-9][0-9]*$ ]] || (( num_predict > 4096 )); then
    printf 'GPUF_BENCHMARK_NUM_PREDICT must be between 1 and 4096\n' >&2
    exit 2
fi

prompt="${GPUF_BENCHMARK_PROMPT:-Explain why deterministic benchmark inputs matter in one concise paragraph.}"
suite="${GPUF_BENCHMARK_SUITE:-GPUFabric-Ollama}"
suite_version="${GPUF_BENCHMARK_SUITE_VERSION:-1.0}"
task="${GPUF_BENCHMARK_TASK:-LLM generation}"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

parameters_file="$tmp_dir/parameters.json"
jq -cn \
    --arg model "$GPUF_BENCHMARK_MODEL" \
    --arg prompt "$prompt" \
    --argjson num_predict "$num_predict" \
    --argjson trials "$trials" \
    '{model:$model,prompt:$prompt,stream:false,options:{temperature:0,num_predict:$num_predict},trials:$trials}' \
    >"$parameters_file"
parameters_sha256="$(sha256sum "$parameters_file" | awk '{print $1}')"

target_headers_file="$tmp_dir/target-headers.txt"
printf 'Content-Type: application/json\n' >"$target_headers_file"
if [[ -n "${GPUF_BENCHMARK_TARGET_TOKEN:-}" ]]; then
    printf 'Authorization: Bearer %s\n' "$GPUF_BENCHMARK_TARGET_TOKEN" >>"$target_headers_file"
fi
chmod 600 "$target_headers_file"

for ((trial = 1; trial <= trials; trial++)); do
    response_file="$tmp_dir/trial-$trial.json"
    jq 'del(.trials)' "$parameters_file" | curl \
        --fail-with-body \
        --silent \
        --show-error \
        --connect-timeout "${GPUF_BENCHMARK_CONNECT_TIMEOUT_SECONDS:-10}" \
        --max-time "${GPUF_BENCHMARK_TIMEOUT_SECONDS:-300}" \
        -H @"$target_headers_file" \
        --data-binary @- \
        "${GPUF_BENCHMARK_TARGET_URL%/}/api/generate" \
        >"$response_file"
    jq -e '
        .done == true and
        (.eval_count | type == "number" and . > 0) and
        (.eval_duration | type == "number" and . > 0)
    ' "$response_file" >/dev/null
done

total_eval_count="$(jq -s 'map(.eval_count) | add' "$tmp_dir"/trial-*.json)"
total_eval_duration="$(jq -s 'map(.eval_duration) | add' "$tmp_dir"/trial-*.json)"
tokens_per_second="$(jq -cn \
    --argjson count "$total_eval_count" \
    --argjson duration "$total_eval_duration" \
    '$count * 1000000000 / $duration')"
trial_rates="$(jq -s '[.[] | .eval_count * 1000000000 / .eval_duration]' "$tmp_dir"/trial-*.json)"
sustained_throughput_percent="$(jq -cn \
    --argjson rates "$trial_rates" \
    '($rates | min) / ($rates | max) * 100')"

tested_at="$(date -u +'%Y-%m-%dT%H:%M:%SZ')"
expires_at="$(date -u -d '+29 days' +'%Y-%m-%dT%H:%M:%SZ')"
producer_headers_file="$tmp_dir/producer-headers.txt"
printf 'Content-Type: application/json\nAuthorization: Bearer %s\n' \
    "$GPUF_BENCHMARK_PRODUCER_TOKEN" >"$producer_headers_file"
chmod 600 "$producer_headers_file"

register_metric() {
    local suffix="$1"
    local metric_suite="$2"
    local metric_task="$3"
    local metric="$4"
    local metric_value="$5"
    local metric_unit="$6"
    local registration_file="$7"
    local evidence_id="bench-$(date -u +'%Y%m%dT%H%M%SZ')-$(openssl rand -hex 8)-$suffix"
    local payload_file="$tmp_dir/payload-$suffix.json"
    local signature_file="$tmp_dir/signature-$suffix.bin"
    local envelope_file="$tmp_dir/envelope-$suffix.json"

    jq -cn \
        --arg schema_version 'gpuf.benchmark_evidence.v1' \
        --arg evidence_id "$evidence_id" \
        --arg source_ref "${GPUF_BENCHMARK_SOURCE_REF,,}" \
        --arg suite "$metric_suite" \
        --arg suite_version "$suite_version" \
        --arg task "$metric_task" \
        --arg metric "$metric" \
        --argjson value "$metric_value" \
        --arg unit "$metric_unit" \
        --arg tested_at "$tested_at" \
        --arg expires_at "$expires_at" \
        --arg parameters_sha256 "$parameters_sha256" \
        '{schemaVersion:$schema_version,evidenceId:$evidence_id,sourceRef:$source_ref,suite:$suite,suiteVersion:$suite_version,task:$task,metric:$metric,value:$value,unit:$unit,testedAt:$tested_at,expiresAt:$expires_at,parametersSha256:$parameters_sha256}' \
        >"$payload_file"

    openssl pkeyutl \
        -sign \
        -rawin \
        -inkey "$GPUF_BENCHMARK_PRIVATE_KEY" \
        -in "$payload_file" \
        -out "$signature_file"
    local signature_base64
    signature_base64="$(openssl base64 -A -in "$signature_file")"

    jq -cn \
        --rawfile payload_json "$payload_file" \
        --arg key_id "$GPUF_BENCHMARK_KEY_ID" \
        --arg signature_base64 "$signature_base64" \
        '{payloadJson:$payload_json,keyId:$key_id,signatureBase64:$signature_base64}' \
        >"$envelope_file"

    curl \
        --fail-with-body \
        --silent \
        --show-error \
        -H @"$producer_headers_file" \
        --data-binary @"$envelope_file" \
        "${GPUF_BENCHMARK_API_URL%/}/api/banking/provider/benchmark-evidence" \
        >"$registration_file"
}

llm_registration_file="$tmp_dir/registration-llm.json"
stability_registration_file="$tmp_dir/registration-stability.json"
register_metric \
    'llm' "$suite" "$task" 'tokens_per_second' "$tokens_per_second" 'tokens/s' \
    "$llm_registration_file"
register_metric \
    'stability' "${suite}-Stability" 'repeated LLM generation' \
    'sustained_throughput_percent' "$sustained_throughput_percent" 'percent' \
    "$stability_registration_file"

jq -n \
    --arg parameters_sha256 "$parameters_sha256" \
    --argjson trials "$trials" \
    --argjson total_eval_count "$total_eval_count" \
    --argjson total_eval_duration_ns "$total_eval_duration" \
    --argjson tokens_per_second "$tokens_per_second" \
    --argjson sustained_throughput_percent "$sustained_throughput_percent" \
    --slurpfile llm_registration "$llm_registration_file" \
    --slurpfile stability_registration "$stability_registration_file" \
    '{parametersSha256:$parameters_sha256,trials:$trials,totalEvalCount:$total_eval_count,totalEvalDurationNs:$total_eval_duration_ns,tokensPerSecond:$tokens_per_second,sustainedThroughputPercent:$sustained_throughput_percent,registrations:{llm:$llm_registration[0],stability:$stability_registration[0]}}'
