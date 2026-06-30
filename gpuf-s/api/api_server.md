# gpuf-s API Server

## Base
- **Base URL**: `http://127.0.0.1:18081` by default; use `http://<host>:18081` only for a protected deployment
- **Content-Type**: `application/json`

## Frontend Integration And Security Defaults

The standalone management API now binds to `127.0.0.1` by default. Start it with `--bind-addr 127.0.0.1` for local frontend development; choose `--bind-addr 0.0.0.0` only behind a reverse proxy/firewall and with deployment-level access control.

Existing REST paths and response envelopes remain compatible for frontends. The model APIs add optional `download_url`, `checksum`, and `expected_size` fields so UIs can show SHA256-verified artifact metadata without breaking older clients.
Control TLS is separate from this REST API. If the same deployment accepts remote gpuf-c workers over non-loopback networks, enable `gpuf-s --control-tls` and configure clients with `gpuf-c --control-tls --control-tls-server-name <name> --cert-chain-path <ca.pem>`. Mobile native workers can use the additive `startRemoteWorkerWithTls` SDK entry point; this does not change frontend REST paths or response envelopes. Android native SDK rebuild/test fixes validated on 2026-06-09 also do not require frontend REST changes.

## Common Response Envelope
All endpoints return this envelope type:

```json
{
  "success": true,
  "data": {},
  "message": "success",
  "timestamp": "2026-02-03T06:35:28.784161Z"
}
```

- **success**: `bool`
- **data**: `T | null`
- **message**: `string`
- **timestamp**: `RFC3339 string`

`GET /api/compute-map` is a frontend map payload and intentionally returns raw
JSON instead of this envelope.
`/api/banking/admin/*` endpoints use the frontend banking page envelope:

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

---

# Compute Map API

## GET `/api/compute-map`
Get nationwide city-level compute map data. This endpoint returns raw JSON, not
`ApiResponse<T>`.

### Response `ComputeMapResponse`
| Field | Type | Notes |
|---|---|---|
| summary | ComputeMapSummary\|null | Returns inventory totals and token aggregates from `inference_token_usage` |
| nodes | ComputeMapNode[] | City-level aggregation, not per-device details |
| links | ComputeMapLink[] | Currently `[]`; future route/traffic metrics can populate it |

`ComputeMapSummary`:
| Field | Type | Notes |
|---|---|---|
| onlineNodes | number | Strict online mapped node count, normalized from `client_status` |
| totalTflops | number | Real aggregated TFLOPS from `system_info.total_tflops` |
| tokenTps | number | Real average token TPS from the latest 10-second window |
| todayTokenTotal | number | Real total tokens today, expressed in `todayTokenUnit` |
| todayTokenUnit | string | Auto-scaled unit: `K`, `M`, `B`, or `T` |
| usedNodes | number | Best-effort used node count from real model assignment and telemetry load |

`ComputeMapNode`:
| Field | Type | Notes |
|---|---|---|
| id | string | Derived stable city id from `gpu_assets.geo_city`, e.g. `beijing` |
| name | string | Derived city display name, e.g. `北京` |
| lng | number | Longitude; static known-city coordinate or averaged stored geo coordinate |
| lat | number | Latitude; static known-city coordinate or averaged stored geo coordinate |
| nodeCount | number | Real city aggregated inventory count |
| tflops | number | Real city aggregated TFLOPS from `system_info.total_tflops` |
| gpuModel | string\|null | Real top device model names from `device_info.device_name`, joined with `/` |
| region | string\|null | Derived compute region, e.g. `华北算力区` |
| status | string | Derived from `client_status` and `valid_status`: `online`, `warning`, `offline`, or `maintenance` |

`ComputeMapLink`:
| Field | Type | Notes |
|---|---|---|
| from | string | Reserved for future route source |
| to | string | Reserved for future route source |
| value | number | Link weight |

### Data Rules
- Uses `gpu_assets.geo_country`, `geo_city`, `geo_latitude`, `geo_longitude`,
  `client_status`, `valid_status`, `system_info.total_tflops`, and
  `device_info.device_name`.
- Includes only rows with valid/warning status and usable city coordinates.
- Limits the nationwide map to China/CN/中国/HK/Macau/Taiwan geo country
  values, while allowing null country for backward-compatible test data.
- `links` is empty until a reliable city-to-city route/traffic source exists.

### Field Authenticity
| Category | Fields | Meaning |
|---|---|---|
| Real persisted data | `nodes[].nodeCount`, `nodes[].tflops`, `nodes[].gpuModel`, `summary.totalTflops` | Aggregated from currently persisted database rows |
| Real status-derived data | `nodes[].status`, `summary.onlineNodes` | Calculated from persisted `client_status` and `valid_status` |
| Best-effort usage data | `summary.usedNodes` | Count of online nodes with assigned model metadata or current telemetry load >= 5% |
| Geo-derived/static data | `nodes[].id`, `name`, `lng`, `lat`, `region` | Derived from persisted geo fields; known Chinese cities use built-in city coordinates/regions for stable frontend display |
| Token usage data | `summary.tokenTps`, `summary.todayTokenTotal` | Aggregated from `inference_token_usage`; starts accumulating after this version is deployed |
| Placeholder data | `links` | Returned as `[]` in v1 because there is no persisted route source yet |

### Missing API Logic In V1
| Missing logic | Current behavior | Needed source/implementation |
|---|---|---|
| Exact serving-allocation state | `summary.usedNodes` is a best-effort derived value, not an exact scheduler allocation count | Persist inference/allocation/session state if the product needs exact serving workload occupancy |
| Historical token backfill | Token aggregates only include rows recorded after `inference_token_usage` is deployed | Backfill from older logs only if historical token charts are required |
| City network links | `links` is always `[]` | Add route, traffic, scheduling, or data-flow metrics between city ids |
| Geo freshness flag | Response does not distinguish backfilled/test geo from freshly reported geo | Store/report geo source and last update time, e.g. `geo_source`, `geo_updated_at` |

Test environment note: historical offline devices may have geo fields filled by a
test backfill script so the map has enough display data. When a real device
reports fresh public IP/geo data, the persisted geo fields are expected to be
updated by the normal ingest flow.

### Example
```bash
curl "http://<host>:18081/api/compute-map"
```

```json
{
  "summary": {
    "onlineNodes": 1,
    "totalTflops": 80,
    "tokenTps": 0,
    "todayTokenTotal": 0.0,
    "todayTokenUnit": "T",
    "usedNodes": 0
  },
  "nodes": [
    {
      "id": "beijing",
      "name": "北京",
      "lng": 116.4074,
      "lat": 39.9042,
      "nodeCount": 1,
      "tflops": 80,
      "gpuModel": "H100",
      "region": "华北算力区",
      "status": "online"
    }
  ],
  "links": []
}
```

# Banking Admin API

These endpoints back the `/banking/admin` dashboard. The first version uses
only currently persisted node inventory fields from `gpu_assets`,
`system_info`, and `device_info`.

## Data Authenticity Legend

| Label | Meaning |
|---|---|
| Real | Directly read from persisted tables or aggregated from persisted rows |
| Derived | Calculated/normalized from real persisted rows, or mapped through built-in city/region rules |
| Placeholder | Fixed `0`, empty array, or generated series kept only to satisfy the frontend contract |
| Test backfill possible | The field can be real in production, but test data may include historical/demo backfill values |

Current real persisted sources are:

- `gpu_assets`: `client_id`, `client_name`, `client_status`, `valid_status`, `os_type`, geo fields, timestamps
- `system_info`: `total_tflops`, CPU/memory/disk usage, timestamps
- `device_info`: device rows, GPU model names, GPU usage, timestamps
- `inference_token_usage`: inference token usage rows written by gpuf-s gateway

## Version 1 Support Matrix

| Area | Status | Notes |
|---|---|---|
| City/node inventory | Supported | Derived from `gpu_assets` geo/status fields |
| Online node count | Supported | Counts normalized `active`/`online` nodes |
| GPU model/count | Supported | Aggregated from `device_info.device_name` and device rows |
| TFLOPS | Supported | Aggregated from `system_info.total_tflops`, unit is TFLOPS |
| Used nodes/devices | Best-effort supported | Counts online nodes with assigned model metadata or current telemetry load >= 5% |
| OS/device filter | Supported | Normalizes `gpu_assets.os_type` to `linux/windows/mac/unknown` |
| Region/city filter | Supported | Supports city id/name, province, region id, and region name where applicable |
| `from` / `to` on overview | Not supported in v1 | Accepted but ignored; no historical dashboard metric source yet |
| Token TPS / token totals | Supported | Aggregated from `inference_token_usage` |
| Network links | Not supported in v1 | Returned as `[]`; no route/traffic source yet |
| Token `region` filter | Supported | Matches `gpu_assets.geo_city` or `geo_region` for the serving node |
| Compute node `owner` | Best-effort supported | Uses `gpu_assets.user_id` when present; falls back to `<算力区>节点池` |
| Compute node `load` | Derived fallback | Max of current CPU/memory/disk/GPU usage, not model-serving load |

## Field Authenticity Summary

| Endpoint | Real fields | Derived fields | Placeholder / not real in v1 |
|---|---|---|---|
| `/overview` | `summaryCards[onlineNodes].value`, `summaryCards[totalCompute].value`, token summary cards, `resourceUsage.totalDevices` | `resourceUsage.usedDevices`, `resourceUsage.usageRate`, `summaryCards[].displayValue`, `clusterStack[].percent` | none for current summary surface |
| `/network-map` | `cities[].nodes`, `cities[].tflops`, `cities[].gpuModel`, `cities[].onlineNodes`, `topCities[]` | `cities[].id/name/province/coord/tier`, `cities[].usedNodes`, `regions[]`, `highlightProvinces[]` | `links` |
| `/compute-nodes` | `id`, `owner` when `user_id` exists, `name` when `client_name` exists, `gpuModel`, `gpuCount`, `tokensPerSecond`, `lastSeenAt` | fallback `name`, fallback `owner`, `region`, `regionId`, `device`, `status`, `gpu`, `load`, `lastSeenText` | none for current node table surface |
| `/token-throughput` | `input`, `output`, peak values, timestamps from `inference_token_usage` buckets | empty buckets are generated as zero points for chart continuity | none for current throughput surface |

## Missing API Logic In V1

| Endpoint | Missing logic | Current behavior | Needed source/implementation |
|---|---|---|---|
| `/overview` | Historical filtering by `from`/`to` | Params are accepted but ignored | Add time-series dashboard tables or aggregate snapshots |
| `/overview` | Exact used device/resource usage | `usedDevices` and `usageRate` are best-effort values from model metadata and telemetry load | Persist currently assigned/serving workload state if exact scheduler occupancy is required |
| `/overview` | Historical token backfill | Token TPS and daily token total start from newly recorded `inference_token_usage` rows | Backfill from logs only if pre-deployment history is required |
| `/overview` | Real serving load | `clusterStack` only classifies inventory by GPU model family | Add actual serving/resource allocation metrics if frontend needs utilization composition |
| `/network-map` | City-to-city links | `links` is always `[]` | Add scheduling, request flow, bandwidth, or topology source keyed by city ids |
| `/network-map` | Exact used nodes per city | `cities[].usedNodes` is best-effort from model metadata and telemetry load | Same serving/allocation source as overview used devices if exact values are required |
| `/network-map` | Province highlight semantics | Highlights are derived from top TFLOPS/node provinces | Define product semantics if highlight means policy focus, fault area, hot traffic, or sales region |
| `/compute-nodes` | Full owner/account relation | `owner` uses `gpu_assets.user_id` when present, otherwise generated node-pool fallback | Add owner/account/pool table relation if display name/email/org is required |
| `/compute-nodes` | Real model-serving load | `load` is max of CPU/memory/disk/GPU usage | Add workload queue/runtime utilization metric if load means inference load |
| `/compute-nodes` | Historical per-node token rate | `tokensPerSecond` uses the latest 10-second window and starts after `inference_token_usage` is deployed | Backfill from logs only if historical per-node charts are required |
| `/compute-nodes` | Geo data provenance | API does not mark whether geo is real-time reported or test-backfilled | Store/report geo source and update timestamp |
| `/token-throughput` | Historical token backfill | Series starts from newly recorded rows; old requests are not reconstructed | Backfill from logs only if pre-deployment history is required |

## GET `/api/banking/admin/overview`
Get summary cards, resource usage, and coarse resource composition.

### Query Parameters
| Param | Type | Optional | v1 Notes |
|---|---:|:---:|---|
| region | string | Yes | Filters by city/region when matched |
| from | ISO8601 string | Yes | Optional start of time range; when both `from`/`to` are valid, total tokens are summed in range |
| to | ISO8601 string | Yes | Optional end of time range; must be later than `from` |

### Response `data`
| Field | Type | v1 Notes |
|---|---|---|
| summaryCards | SummaryCard[] | `onlineNodes`, `totalCompute`, and token cards are real after token usage rows are recorded |
| resourceUsage.totalDevices | number | Real filtered node count |
| resourceUsage.usedDevices | number | Best-effort count of online nodes with assigned model metadata or telemetry load >= 5% |
| resourceUsage.usageRate | number | `usedDevices / totalDevices`, rounded percentage |
| clusterStack | ClusterStackItem[] | Derived from GPU model names; unknown/CPU nodes fall into `cpu_edge` |

### Example
```bash
curl "http://<host>:18081/api/banking/admin/overview?region=beijing"
```

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "summaryCards": [
      {
        "key": "onlineNodes",
        "label": "在线节点",
        "value": 1,
        "displayValue": "1",
        "unit": "个"
      },
      {
        "key": "totalCompute",
        "label": "总算力",
        "value": 80,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "realtimeTokenThroughput",
        "label": "实时Token吞吐",
        "value": 0,
        "displayValue": "0",
        "unit": "/s",
        "caption": "当前TPS"
      },
      {
        "key": "todayTokenTotal",
        "label": "今日Token总量",
        "value": 0,
        "displayValue": "0",
        "unit": "T",
        "caption": "今日已调用"
      }
    ],
    "resourceUsage": {
      "totalDevices": 1,
      "usedDevices": 0,
      "usageRate": 0
    },
    "clusterStack": [
      {
        "key": "a100_h100",
        "label": "GPU A100/H100",
        "percent": 100
      },
      {
        "key": "a800_4090",
        "label": "GPU A800/4090",
        "percent": 0
      },
      {
        "key": "cpu_edge",
        "label": "CPU/边缘节点",
        "percent": 0
      }
    ]
  }
}
```

## GET `/api/banking/admin/network-map`
Get city-level map nodes, region groups, highlighted provinces, and top cities.

### Response `data`
| Field | Type | v1 Notes |
|---|---|---|
| cities | NetworkCity[] | Real city-level aggregation, not per-device details |
| links | NetworkLink[] | Always `[]` in v1 |
| regions | NetworkRegion[] | Static region definitions with currently active city ids |
| highlightProvinces | HighlightProvince[] | Derived from top provinces by TFLOPS/nodes |
| topCities | TopCity[] | Top 5 cities by TFLOPS then node count |

`NetworkCity`:
| Field | Type | v1 Notes |
|---|---|---|
| id | string | Stable city id |
| name | string | City display name |
| province | string | Map province name |
| coord | [number, number] | `[lng, lat]` |
| nodes | number | City node count |
| tflops | number | City total TFLOPS |
| gpuModel | string | Top GPU models joined with `/`, or `Unknown` |
| tier | string | `mega/large/medium/small` from TFLOPS/node count |
| onlineNodes | number | Count of `active`/`online` nodes |
| usedNodes | number | Best-effort count of used nodes in this city |

### Example
```bash
curl "http://<host>:18081/api/banking/admin/network-map"
```

## GET `/api/banking/admin/compute-nodes`
Get paged compute node rows for the access-compute table.

### Query Parameters
| Param | Type | Optional | v1 Notes |
|---|---:|:---:|---|
| status | string | Yes | `active/online/warning/maintenance/offline/error` |
| device | string | Yes | `linux/windows/mac`; unmatched OS is returned as `unknown` |
| region | string | Yes | Matches city id/name or region owner text |
| keyword | string | Yes | Fuzzy search over id/name/owner/region/GPU |
| page | number | Yes | Default `1` |
| pageSize | number | Yes | Default `20`, max `200` |

### Response `data`
| Field | Type | v1 Notes |
|---|---|---|
| items | ComputeNodeItem[] | Real inventory rows |
| pagination.total | number | Filtered row count |
| stats.filteredCount | number | Same filtered count |
| stats.totalCount | number | Total valid/warning inventory count before filters |

`ComputeNodeItem`:
| Field | Type | v1 Notes |
|---|---|---|
| id | string | Hex encoded `client_id` |
| name | string | `client_name` or generated fallback |
| owner | string | Uses `gpu_assets.user_id` when present; otherwise derived `<算力区>节点池` fallback |
| region | string | City name or raw geo region |
| regionId | string\|null | City id when geo city exists |
| device | string | Normalized OS |
| status | string | Normalized node status |
| gpu | string | e.g. `2 x H100`; derived from `device_info` |
| gpuModel | string\|null | Top GPU models |
| gpuCount | number\|null | Count of `device_info` rows |
| load | number | Derived fallback load, 0-100 |
| tokensPerSecond | number | Latest 10-second average token TPS for this node from `inference_token_usage` |
| lastSeenAt | RFC3339 string | Max of asset/system/device update timestamps |
| lastSeenText | string\|null | Backend formatted relative text |

### Example
```bash
curl "http://<host>:18081/api/banking/admin/compute-nodes?page=1&pageSize=20&device=linux"
```

## GET `/api/banking/admin/token-throughput`
Get the time-series payload shape for the token throughput chart.

### Query Parameters
| Param | Type | Optional | v1 Notes |
|---|---:|:---:|---|
| windowSeconds | number | Yes | Default `180`, max `3600`; controls returned point count |
| intervalSeconds | number | Yes | Default `3`, max `300` |
| region | string | Yes | Filters by city or region name when matched |

### Response `data`
Throughput values are aggregated from `inference_token_usage`. Empty buckets are
returned as `0` so charts remain continuous. Each point is normalized to TPS
for the selected interval. `region` filters by the serving node's `geo_city` or
`geo_region`.

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "latest": {
      "timestamp": "2026-06-30T10:30:00Z",
      "input": 0,
      "output": 0
    },
    "peaks": {
      "input": 0,
      "output": 0
    },
    "points": [
      {
        "timestamp": "2026-06-30T10:27:03Z",
        "input": 0,
        "output": 0
      }
    ]
  }
}
```

# Client / User APIs

## POST `/api/user/insert_client`
Create or update a client record for a user.

### Request Body (JSON)
| Field | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | length 1..32 |
| client_id | string | No | parsed as `ClientId` (expected 16 bytes) |
| client_status | string | No | e.g. `online/offline/...` |
| os_type | string | Yes | length 1..64 |
| name | string | No | length 1..32 |

### Response `ApiResponse<Vec<ClientInfoResponse>>`
Current implementation returns an empty list (`[]`).

`ClientInfoResponse`:
| Field | Type | Notes |
|---|---|---|
| client_id | string | client id string |
| authed | bool | |
| connected_at | RFC3339 string | |
| system_info | object\|null | currently internal fields |

### Example
```bash
curl -X POST "http://<host>:18081/api/user/insert_client" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "1",
    "client_id": "<client-id-32-hex>",
    "client_status": "online",
    "os_type": "linux",
    "name": "node-1"
  }'
```

---

## GET `/api/user/client_list`
Get a user’s client list.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | Yes | if provided, parsed as `ClientId` |
| status | string | Yes | client status filter |
| name | string | Yes | matched by `ILIKE` |
| valid_status | string | Yes | e.g. `valid/invalid/warning` |

### Response `ApiResponse<ClientListResponse>`
`ClientListResponse`:
| Field | Type |
|---|---|
| total | number |
| devices | ClientDeviceInfo[] |

`ClientDeviceInfo`:
| Field | Type |
|---|---|
| client_id | string |
| client_name | string |
| client_status | string |
| os_type | string |
| device_name | string |
| tflops | number |
| cpu_usage | number |
| memory_usage | number |
| storage_usage | number |
| health | number |
| last_online | RFC3339 string |
| created_at | RFC3339 string |
| uptime_days | number |
| loaded_models | object[] |

### Example
```bash
curl "http://<host>:18081/api/user/client_list?user_id=1"
```

---

## GET `/api/user/client_device_detail`
Get one client’s system and device detail.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | No | parsed as `ClientId` |
| status | string | Yes | currently unused |
| name | string | Yes | currently unused |

### Response `ApiResponse<ClientDeviceDetailResponse>`
`ClientDeviceDetailResponse`:
| Field | Type |
|---|---|
| system_info | SystemInfoDetailResponse |
| device_info | DeviceInfoResponse[] |

`SystemInfoDetailResponse`:
| Field | Type |
|---|---|
| health | number |
| cpu_usage | number |
| memory_usage | number |
| storage_usage | number |
| device_memsize | number |
| uptime_days | number |

`DeviceInfoResponse`:
| Field | Type |
|---|---|
| device_index | number |
| name | string |
| temp | number |
| usage | number |
| mem_usage | number |
| power_usage | number |

### Example
```bash
curl "http://<host>:18081/api/user/client_device_detail?user_id=1&client_id=<client-id-32-hex>"
```

---

## POST `/api/user/edit_client_info`
Edit client info fields.

### Request Body (JSON)
`EditClientRequest`:
| Field | Type | Optional |
|---|---:|:---:|
| user_id | string | No |
| client_id | string | No |
| os_type | string | Yes |
| name | string | Yes |
| client_status | string | Yes |
| valid_status | string | Yes |

### Response `ApiResponse<()>`

### Example
```bash
curl -X POST "http://<host>:18081/api/user/edit_client_info" \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "1",
    "client_id": "<client-id-32-hex>",
    "client_status": "online"
  }'
```

---

## GET `/api/user/client_status_list`
Alias of client list with status.

### Query Parameters
Same as `/api/user/client_list`.

### Response
Same as `/api/user/client_list`.

---

## GET `/api/user/client_stat`
Get overall user client statistics.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| user_id | string | No |

### Response `ApiResponse<ClientStatResponse>`
| Field | Type |
|---|---|
| systems_total_number | number |
| systems_online_number | number |
| systems_maintenance_number | number |
| systems_warnings_number | number |
| total_tflops | number |
| uptime_rate | number |

---

## GET `/api/user/client_monitor`
Get monitoring summary for user’s clients.

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | length 1..32 |
| client_id | string | Yes | **hex string**; server uses `hex::decode()` to bind to `BYTEA` |

### Response `ApiResponse<Vec<ClientMonitorInfo>>`
`ClientMonitorInfo`:
| Field | Type |
|---|---|
| client_id | string(hex) |
| client_name | string\|null |
| created_at | string\|null |
| updated_at | string\|null |
| date | string\|null |
| avg_cpu_usage | number\|null |
| avg_memory_usage | number\|null |
| avg_disk_usage | number\|null |
| total_network_in_bytes | number\|null |
| total_network_out_bytes | number\|null |
| total_heartbeats | number\|null |
| last_heartbeat | RFC3339 string\|null |
| avg_network_in_bytes | number\|null |
| avg_network_out_bytes | number\|null |

---

## GET `/api/user/client_health`
Get heartbeat records (time series).

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | |
| client_id | string | Yes | **hex string**; server decodes to `BYTEA` |
| start_date | string | Yes | passed through to SQL, recommended `YYYY-MM-DD` |
| end_date | string | Yes | passed through to SQL, recommended `YYYY-MM-DD` |

### Response `ApiResponse<Vec<ClientHeartbeatInfo>>`
`ClientHeartbeatInfo`:
| Field | Type |
|---|---|
| client_id | string(hex) |
| client_name | string\|null |
| timestamp | RFC3339 string |
| cpu_usage | number\|null |
| mem_usage | number\|null |
| disk_usage | number\|null |
| network_up | number |
| network_down | number |

---

## GET `/api/user/model_download_progress`
Get model download progress from Redis.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| client_id | string | No |

### Response `ApiResponse<ModelDownloadProgressResponse>`
| Field | Type |
|---|---|
| client_id | string |
| model_name | string\|null |
| downloaded_bytes | number\|null |
| total_bytes | number\|null |
| percentage | number\|null |
| speed_bps | number\|null |
| status | string\|null |
| error | string\|null |
| timestamp | number\|null |

---

# Points APIs

## GET `/api/user/points`
Query a user’s points list (based on materialized view `device_points_daily`).

### Query Parameters
| Param | Type | Optional | Notes |
|---|---:|:---:|---|
| user_id | string | No | joins via `gpu_assets.user_id` |
| client_id | string | Yes | client id **hex string (32 chars)**; filters by exact client |
| client_name | string | Yes | fuzzy match by `gpu_assets.client_name` using `ILIKE '%...%'` |
| device_id | number | Yes | `INT` device id |
| start_date | string | Yes | `YYYY-MM-DD` |
| end_date | string | Yes | `YYYY-MM-DD` |
| page | number | Yes | 1..100, default 1 |
| page_size | number | Yes | 1..100, default 20 |

### Response `ApiResponse<PointsListResponse>`
`PointsListResponse`:
| Field | Type |
|---|---|
| points | DevicePointsResponse[] |
| total_points | number |
| total_count | number |
| page | number |
| page_size | number |

`DevicePointsResponse`:
| Field | Type |
|---|---|
| client_id | string | hex string (`encode(bytea,'hex')`) |
| client_name | string | from `gpu_assets.client_name` |
| date | string | `YYYY-MM-DD` |
| total_heartbeats | number |
| device_name | string |
| device_id | number |
| points | number |

### Example
```bash
curl "http://<host>:18081/api/user/points?user_id=1&page=1&page_size=20"
curl "http://<host>:18081/api/user/points?user_id=1&client_id=<client-id-32-hex>"
curl "http://<host>:18081/api/user/points?user_id=1&client_name=node"
curl "http://<host>:18081/api/user/points?user_id=1&device_id=9860&start_date=2026-02-01&end_date=2026-02-03"
```

---

# Model APIs

## POST `/api/models/insert`
Create or update a model.

### Request Body (JSON)
| Field | Type | Optional |
|---|---:|:---:|
| name | string | No |
| version | string | No |
| version_code | number | No |
| engine_type | number | No |
| is_active | bool | Yes |
| min_memory_mb | number | Yes |
| min_gpu_memory_gb | number | Yes |
| download_url | string | Yes |
| checksum | string | Yes |
| expected_size | number | Yes |

### Response `ApiResponse<()>`

---

## GET `/api/models/get`
Get models list.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| is_active | bool | Yes |
| min_gpu_memory_gb | number | Yes |

### Response `ApiResponse<Vec<ModelResponse>>`
`ModelResponse`:
| Field | Type |
|---|---|
| id | number |
| name | string |
| version | string |
| version_code | number |
| is_active | bool |
| min_memory_mb | number\|null |
| min_gpu_memory_gb | number\|null |
| created_at | RFC3339 string |
| download_url | string\|null |
| checksum | string\|null |
| expected_size | number\|null |

---

# APK APIs

## POST `/api/apk/upsert`
Upsert an APK version.

### Request Body (JSON)
| Field | Type | Optional |
|---|---:|:---:|
| package_name | string | No |
| version_name | string | No |
| version_code | number | No |
| download_url | string | No |
| channel | string | Yes |
| min_os_version | string | Yes |
| sha256 | string | Yes |
| file_size_bytes | number | Yes |
| is_active | bool | Yes |
| released_at | RFC3339 string | Yes |

### Response `ApiResponse<ApkResponse>`
`ApkResponse`:
| Field | Type |
|---|---|
| id | number |
| package_name | string |
| version_name | string |
| version_code | number |
| download_url | string |
| channel | string\|null |
| min_os_version | string\|null |
| sha256 | string\|null |
| file_size_bytes | number\|null |
| is_active | bool |
| released_at | RFC3339 string\|null |
| created_at | RFC3339 string |
| updated_at | RFC3339 string |

---

## GET `/api/apk/get`
Get one APK version.

### Query Parameters
| Param | Type | Optional |
|---|---:|:---:|
| package_name | string | No |
| version_code | number | No |

### Response `ApiResponse<ApkResponse|null>`

---

## GET `/api/apk/list`
List APK versions.

### Query Parameters
| Param | Type | Optional | Default |
|---|---:|:---:|---|
| package_name | string | Yes | |
| channel | string | Yes | |
| is_active | bool | Yes | |
| limit | number | Yes | 50 (max 200) |

### Response `ApiResponse<Vec<ApkResponse>>`
