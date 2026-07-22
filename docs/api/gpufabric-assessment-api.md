# GPUFabric 评估数据 API 契约

> 接口所有者：GPUFabric `api_server`
> 版本：技术预评估 v1、技术快照 v2
> 主要调用方：asset-assessment-service、new-api
> 权限：报告创建/读取与 benchmark producer 分离

## 1. 边界

GPUFabric 只提供可采集、可验证、可追溯的技术事实：

- GPU、CPU、内存、驱动、互联和操作系统等硬件信息。
- 设备遥测、健康和稳定性摘要。
- 可验证的 benchmark 结果。
- 技术预评估报告。
- 不可变技术快照及 SHA-256。

GPUFabric 不提供：

- 权属结论。
- 市场价格和正式估值。
- 质押率、可贷额度和银行审批结论。
- 用户发票、合同、实名和支付材料。
- 正式报告签章和撤销。

## 2. 通用认证

```http
Authorization: Bearer <service-token>
X-Request-ID: req_01...
X-Correlation-ID: corr_01...
Accept: application/json
```

建议 scope：

```text
technical-report:read
technical-snapshot:read
```

assessment-service 和其他调用方必须使用不同服务身份，禁止共享 Token。报告创建可携带 `Idempotency-Key`，范围为脱敏服务主体 + 脱敏租户 + 操作；相同请求复用结果，不同请求返回 `409`。Benchmark 注册使用独立 producer Token 和 Ed25519 公钥集合。

## 3. 创建技术预评估

在线和离线创建接口分别为：

```http
POST /api/banking/provider/pre-evaluations/from-client
POST /api/banking/provider/pre-evaluations/challenge
POST /api/banking/provider/pre-evaluations/from-evidence
```

供 new-api 等内部编排服务使用的等价路径为：

```http
POST /internal/v1/technical-pre-evaluations/from-client
POST /internal/v1/technical-pre-evaluations/challenge
POST /internal/v1/technical-pre-evaluations/from-evidence
```

两组路径使用相同鉴权、请求大小、证据验证和报告生成逻辑。旧 `userId + clientId` 协议保持兼容；internal 调用可使用 `gpufUserRef + gpufClientRef + tenantRef + clientRequestId`。`clientRequestId` 与 `Idempotency-Key` 均可建立租户范围幂等，二者同时出现时必须完全一致，否则返回 `400`。

新调用方可增加 `benchmarkEvidenceIds`；非空数组严格指定证据，空数组按同一技术 `sourceRef` 自动选择每个 metric 最新且未过期的已签名证据。非空 `supplements` 一律拒绝。响应中的 `valuation` 固定为 `null`，挂牌和授信资格固定为 `false`。创建响应返回完整技术报告，调用方随后通过 internal 读取接口取得冻结 JSON、HTML 和 snapshot v2 的 ID、schema version 与 SHA-256 引用。

internal 在线请求示例：

```json
{
  "clientRequestId": "pe_01J0ONLINE001",
  "tenantRef": "tenant_hmac_v1_0123456789abcdef",
  "gpufUserRef": "gpuf-user-ref",
  "gpufClientRef": "00112233445566778899aabbccddeeff",
  "assetName": "GPU-node-A01",
  "benchmarkEvidenceIds": []
}
```

离线 collector 未启用运行采样或历史文件时，`runtime: null` 是预期结果，报告会包含
`RUNTIME_HISTORY_MISSING`。启用 `--runtime-duration-seconds` 后，或使用
`--runtime-history-file` 加载跨进程 JSONL 历史，GPUFabric 会把
`hardware.runtime_history` 的利用率、温度、功耗均值和真实 `observation_days` 归一化到
`runtime`；窗口不足 7 天时保留 `SHORT_OBSERVATION_WINDOW`，不会把短期现场采样当作
长期稳定性。

internal 离线请求示例：

```json
{
  "clientRequestId": "pe_01J0OFFLINE001",
  "tenantRef": "tenant_hmac_v1_0123456789abcdef",
  "assetName": "Offline-GPU-node-01",
  "hardwareEvidenceJson": "{...collector original JSON...}",
  "benchmarkEvidenceIds": []
}
```

## 4. 注册可信 BenchmarkEvidence

```http
POST /api/banking/provider/benchmark-evidence
```

只接受 Ed25519 签名的原始 `payloadJson`，并验证设备 `sourceRef`、参数 SHA-256、测试时间和有效期。报告只能引用已登记、未过期且与技术来源一致的证据。`scripts/run_signed_ollama_benchmark.sh` 最少执行 3 轮，并分别登记 LLM 吞吐和持续吞吐百分比两条证据。

## 5. 查询技术预评估报告

```http
GET /internal/v1/technical-pre-evaluations/{reportId}
```

响应使用原始 JSON 字节完整性信封：

```json
{
  "success": true,
  "data": {
    "reportId": "PRE-2026-07-...",
    "schemaVersion": "gpuf.pre_evaluation.v1",
    "reportSha256": "64-hex",
    "hashProfile": "gpuf.report-json-bytes.v1",
    "reportJson": "{...原始报告 JSON...}",
    "report": {
      "technicalSnapshot": {
        "snapshotId": "TAS-2026-07-...",
        "schemaVersion": "technical_asset_snapshot.v2",
        "snapshotSha256": "64-hex",
        "hashProfile": "gpuf.snapshot-json-bytes.v2"
      }
    }
  }
}
```

`technicalSnapshot`、`reportHtmlSha256` 和 `htmlHashProfile` 是 v1 新增可选字段，旧客户端可以忽略。报告不得包含正式估值、可贷额度、确权结论或银行授信结论。

冻结 HTML 可通过以下接口读取：

```http
GET /api/banking/provider/pre-evaluations/{reportId}/html
```

调用方对原始 HTML UTF-8 字节计算 SHA-256，并同时比较引用 Hash 与 `X-Content-SHA256`。

## 6. 查询不可变技术快照

```http
GET /internal/v2/technical-snapshots/{snapshotId}
```

响应示例：

```json
{
  "success": true,
  "data": {
    "snapshotId": "TAS-2026-07-...",
    "schemaVersion": "technical_asset_snapshot.v2",
    "snapshotSha256": "64-hex",
    "hashProfile": "gpuf.snapshot-json-bytes.v2",
    "snapshotJson": "{...原始快照 JSON...}",
    "snapshot": {
      "reportId": "PRE-2026-07-...",
      "capturedAt": "2026-07-16T00:00:00Z",
      "source": {},
      "asset": {},
      "assetConfiguration": {
        "schemaVersion": "gpuf.asset_configuration.v1",
        "hashProfile": "gpuf.asset-configuration-lines.v1",
        "canonicalModelId": "nvidia-a100-pcie-80gb",
        "deviceForm": "pcie_card",
        "gpuCount": 1,
        "memoryPerGpuBytes": 85899345920,
        "configurationHash": "4193527c64c8292550cd8ae250546f33daf2969ca122bcbdb8dbeea5634b70b9"
      },
      "hardware": {},
      "runtime": null,
      "theoreticalPerformance": {},
      "benchmarks": [],
      "fieldProvenance": {
        "/asset/gpuModel": {
          "sourceRef": "sha256-source-reference",
          "quality": "collected",
          "observedAt": "2026-07-16T00:00:00Z"
        }
      },
      "missingFields": ["TRUSTED_BENCHMARK_MISSING"],
      "warningCodes": [],
      "quality": {
        "completeness": 0.7,
        "confidence": 0.825
      }
    }
  }
}
```

`assetConfiguration` 只在 GPU 清单完整、同构、逐卡显存一致且型号命中服务端规格目录时出现。Hash 输入为 `gpuf.asset_configuration.v1`、`canonicalModelId`、`deviceForm`、`gpuCount`、`memoryPerGpuBytes` 的固定换行协议，末尾包含换行；不得通过 JSON 重序列化计算。完整规则见 gpuf-s API 文档。

完整 Schema 和示例：

- [`technical-asset-snapshot.v2.schema.json`](../schema/technical-asset-snapshot.v2.schema.json)
- [`technical-asset-snapshot-v2.json`](../examples/technical-asset-snapshot-v2.json)

## 7. Hash 验证规则

1. `reportJson` 与 `snapshotJson` 是数据库中不可变保存的原始 JSON 字节串。
2. 分别对 UTF-8 字节直接计算 SHA-256，不得在验证前解析、重新排序或重新序列化。
3. v1 JSON 使用 `gpuf.report-json-bytes.v1`，HTML 使用 `gpuf.report-html-bytes.v1`，v2 使用 `gpuf.snapshot-json-bytes.v2`。
4. 调用方必须同时比较请求引用 Hash、信封声明 Hash 和本地重算 Hash。
5. Hash 不一致时 asset-assessment-service 必须停止后续证据或估值流程。

旧的 canonical JSON 信封只用于存量兼容；新生成报告和快照统一使用原始 JSON 字节 Hash。

## 8. 数据最小化

技术快照不得返回：

- 用户姓名、邮箱、手机号和证件号码。
- GPUFabric 用户 Token、设备私钥和 challenge secret。
- 公网 IP、MAC、原始主机名和完整序列号。
- 进程命令行、环境变量、用户文件路径。

需要设备绑定时只返回稳定脱敏标识或由评估域生成 `assetIdentityHash`。

## 9. 状态和错误码

建议错误码：

| HTTP | 错误码 | 含义 |
|---|---|---|
| 401 | `SERVICE_UNAUTHENTICATED` | 服务身份无效 |
| 403 | `SERVICE_SCOPE_DENIED` | 缺少读取 scope |
| 404 | `TECHNICAL_REPORT_NOT_FOUND` | 报告不存在或不可见 |
| 404 | `TECHNICAL_SNAPSHOT_NOT_FOUND` | 快照不存在或不可见 |
| 409 | `TECHNICAL_REPORT_REVOKED` | 报告已撤销 |
| 410 | `TECHNICAL_REPORT_EXPIRED` | 报告已过期 |
| 422 | `TECHNICAL_SCHEMA_UNSUPPORTED` | schema 不受支持 |
| 503 | `GPUFABRIC_UNAVAILABLE` | 服务暂不可用 |

## 10. 兼容要求

- 技术预评估 API 保持 v1，新增可选字段不得破坏旧客户端。
- 技术快照使用独立 v2 schema，不要求修改旧 gpuf-c/gpuf-s 设备协议。
- 新字段必须可选并包含来源、质量或缺失说明。
- 不兼容变更必须升级 URL 或 schema version。

## 11. 调用方职责

asset-assessment-service 必须保存：

```text
reportId
reportSchemaVersion
reportSha256
snapshotId
snapshotSchemaVersion
snapshotSha256
fetchedAt
verifiedAt
verificationStatus
```

不得保存 GPUFabric 服务 Token 和超出评估目的的原始设备信息。
