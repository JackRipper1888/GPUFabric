# 算力资产预评估数据模型与开发路线

状态：开发基线
版本：v1.0
更新时间：2026-07
适用组件：GPUFabric、new-api、asset-assessment-service、Benchmark Runner、Signing Service

Cross-service implementation contract:
[integration and task breakdown](pre-evaluation-cross-service-integration.md).

## 1. 决策摘要

系统提供两类报告，必须在名称、数据来源、状态和免责声明上明确区分：

| 报告 | 生成方 | 数据要求 | 可自动生成 | 可包含内容 | 不得包含内容 |
|---|---|---|---|---|---|
| 技术预评估报告 | GPUFabric | 已有硬件、遥测、采集证据、规格目录、可用基准 | 是 | 技术能力、完整度、风险、缺失项、推荐用途 | 无来源的确权、估值、贷款额、银行结论 |
| 正式资产评估报告 | asset-assessment-service | 技术预评估 + 可信确权 + 市场数据 + 策略 + 审核 | 否 | 参考估值、质押率、审核结论、签章报告 | 未经验证或无法追溯的业务结果 |

核心原则：

- 尽可能使用已有数据，但不为了填满报告而猜测。
- 理论规格、实时遥测、长期观测和实测基准必须分开标注。
- 每个关键字段必须携带来源、观察时间、可信等级和版本。
- 市场原始数据、业务规则、客户材料和签章能力保持私有。
- GPUFabric 生成技术预评估；私有服务生成带估值和签章的正式报告。

## 2. 报告分级

### T0 技术预评估

输入：

- 在线 gpuf-c 遥测，或离线 hw-asset-collector 证据。
- GPU 规格目录。
- 已接入的固定基准结果，可为空。

输出：

- `gpuf.technical_asset_snapshot.v2`
- `gpuf.technical_pre_evaluation_report.v1`
- 技术评分、证据完整度、数据可信等级、适用场景、缺失项和后续建议。

限制：

- 不输出无来源的市场估值。
- 不输出预估贷款额和正式授信。
- 不把技术评分解释为信用评分。

### T1 定价预评估

输入：

- T0 技术预评估。
- 设备年龄、成色、保修、维修和运行历史。
- 规范化市场观察与市场价格快照。
- 版本化定价策略。

输出：

- 参考价值区间，而不是单一确定价格。
- 点估值、低位值、高位值、方法、策略版本和置信度。
- 可进入正式评估的条件和仍缺失的材料。

限制：

- T1 仍不是银行正式授信。
- 市场样本不足时必须降低置信度或不生成估值。

### T2 正式资产评估

输入：

- T1 定价预评估。
- 确权材料、设备身份映射、人工审核和机构政策。
- 必要时接入银行或担保机构结果。

输出：

- 冻结的正式报告 JSON。
- 签章 PDF、证书链和可信时间戳。
- 报告签发、撤销、过期和审计记录。

## 3. 系统边界

| 系统 | 数据所有权 | 主要职责 | 禁止事项 |
|---|---|---|---|
| gpuf-c / collector | 设备本地短期数据 | 采集库存、运行遥测、执行固定基准 | 决定估值、质押率、授信和报告状态 |
| GPUFabric | 技术快照、证据哈希、规格目录 | 规范化、来源校验、T0 预评估 | 保存权属原件、商业价格源和银行规则 |
| new-api | 用户、订单、任务和报告引用 | 用户入口、任务编排、状态和下载授权 | 保存原始证据、私有策略和签章私钥 |
| asset-assessment-service | 市场、确权、策略、审核和正式报告 | T1/T2 评估、定价、审核、签发 | 任意执行设备命令或信任调用方直接结果 |
| Signing Service / HSM | 密钥句柄、签名和证书状态 | PDF 签章和时间戳 | 处理用户业务、估值和设备遥测 |

系统间不得直接跨库查询。所有数据交换通过版本化 API、事件和受控对象引用完成。

### 3.1 跨系统标识规则

- 对外标识使用随机 UUID/ULID，不暴露数据库自增主键。
- 所有用户侧任务必须携带 `tenant_id`、`user_ref` 和 `asset_ref`。
- 所有写接口必须携带 `client_request_id`，并在租户范围内唯一。
- 系统间引用同时保存对象 ID、schema version 和内容 SHA-256。
- 所有查询和状态转换必须显式包含租户条件，不能只按 report ID 查询。
- `correlation_id` 贯穿 new-api、GPUFabric、评估服务、Benchmark Runner 和签章服务。

### 3.2 new-api 预评估任务

```text
pre_evaluation_task_id      UUID / ULID
tenant_id                   tenant reference
user_ref                    local user reference
asset_ref                   user-visible asset reference
client_request_id           tenant-scoped idempotency key
source_type                 gpuf_online / offline_collector
source_asset_ref            client id hash or upload task reference
gpuf_report_id              GPUFabric pre-evaluation report reference
gpuf_report_sha256          immutable report hash
status                      requested / collecting / generated / failed / expired
error_code                  optional normalized code
created_at                  timestamp
completed_at                optional timestamp
expires_at                  optional timestamp
```

new-api 不保存 collector 原文、完整硬件证据、市场观察和风控策略。

### 3.3 私有正式评估主实体

```text
assessment_id               UUID / ULID
tenant_id                   tenant reference
request_ref                 new-api task/request reference
asset_ref                   private assessment asset reference
pre_evaluation_report_id    GPUFabric report reference
pre_evaluation_sha256       immutable report hash
technical_snapshot_id       GPUFabric snapshot reference
technical_snapshot_sha256   immutable snapshot hash
requested_tier              T1 / T2
status                      formal assessment state
evidence_completeness       0..100
market_snapshot_id          optional market aggregate reference
pricing_policy_id           optional approved policy reference
valuation_id                optional valuation result reference
formal_report_id            optional issued report reference
created_by                  service/user reference
created_at                  timestamp
updated_at                  timestamp
```

正式评估服务只保存 GPUFabric 对象的受控引用和哈希，不直接修改 GPUFabric 技术快照。

## 4. 当前数据覆盖矩阵

| 数据域 | 当前来源 | 当前状态 | T0 是否需要 | T1/T2 是否需要 | 处理原则 |
|---|---|---|---|---|---|
| GPU 型号和数量 | gpuf-c / collector | 已有 | 是 | 是 | 逐卡保存，不用部分清单推算节点总量 |
| GPU 显存 | gpuf-c / collector | 部分已有 | 是 | 是 | 旧多卡协议只有总显存时不平均拆分 |
| CPU 和系统内存 | collector | 离线已有，在线不足 | 建议 | 建议 | 缺失时保留空值 |
| 温度、利用率、功耗 | gpuf-c | 已有 | 建议 | 是 | 区分实时值与 30 天历史 |
| 架构、制程、理论算力 | gpu_model_specs | 少量型号 | 建议 | 是 | 必须记录规格来源和版本 |
| PCIe / NVLink 拓扑 | collector / 新协议 | 不完整 | 可选 | 建议 | 未验证拓扑时不生成节点互联带宽 |
| LLM 吞吐、TTFT | Benchmark Runner | 未接入 | 可选 | 建议 | 固定模型、参数和结果签名 |
| 压力测试和稳定性 | Benchmark Runner | 未接入 | 可选 | 是 | 记录持续时间、错误和降频 |
| 采购时间和设备年龄 | 私有证据 | 未接入 | 否 | 是 | 不由设备端自行决定 |
| 成色、保修、维修 | 私有证据 | 未接入 | 否 | 是 | 审核后生成标准化等级 |
| 市场价格 | 私有市场服务 | 未接入 | 否 | 是 | 原始观察和聚合价格分表保存 |
| 权属材料 | 私有证据服务 | 未接入 | 否 | T2 必需 | 原件私有存储，只传引用和哈希 |
| 质押率和授信 | 私有策略/银行 | 未接入 | 否 | T2 可选 | 不进入 GPUFabric |

## 5. 通用来源元数据

所有可用于报告的字段都应关联来源元数据。可以在快照中逐字段携带，也可以通过 `source_ref` 关联证据清单。

```json
{
  "sourceRef": "src_01J...",
  "sourceType": "telemetry|collector|benchmark|catalog|market|ownership|policy",
  "producer": "gpuf-c|hw-asset-collector|benchmark-runner|provider-id",
  "observedAt": "2026-07-14T00:00:00Z",
  "ingestedAt": "2026-07-14T00:00:02Z",
  "schemaVersion": "v1",
  "payloadSha256": "64-hex",
  "signature": null,
  "signerKeyId": null,
  "confidence": 0.92,
  "retentionClass": "technical_snapshot"
}
```

约束：

- `payloadSha256` 使用上传原文或规范化签名载荷的 SHA-256。
- `observedAt` 表示数据事实发生时间，不能用入库时间代替。
- `confidence` 表示数据质量，不表示资产信用。
- 签名证据必须记录签名算法、密钥标识和验证状态。
- 外部业务数据必须记录授权或许可证引用。

## 6. 技术快照模型

GPUFabric 输出 `TechnicalAssetSnapshot`，作为预评估和正式评估共同使用的技术底稿。

```text
snapshot_id                 UUID / random identifier
schema_version              gpuf.technical_asset_snapshot.v2
source_type                 gpuf_online / offline_collector
source_ref                  evidence source reference
asset_display_name          explicit or canonical GPU model
generated_at                timestamp
observation_window          optional start/end
hardware                    normalized hardware inventory
runtime                     current and historical runtime metrics
theoretical_performance     versioned catalog specifications
benchmarks                  verified benchmark references
field_provenance            field path -> source ref / quality
missing_fields              canonical missing-field codes
warnings                    normalized warning codes
snapshot_sha256             exact immutable snapshot hash
```

每个数值字段应标识以下质量之一：

| 质量 | 含义 | 示例 |
|---|---|---|
| `measured` | 固定测试环境中的实测结果 | LLM tokens/s、TTFT、GPU-Burn |
| `observed` | 运行遥测或历史统计 | 温度、利用率、功耗、在线率 |
| `collected` | 探针读取的库存或配置 | GPU 型号、显存、驱动、PCIe |
| `catalog` | 规格目录提供的理论能力 | 理论 FP16、带宽、TDP |
| `derived` | 基于同层可信字段计算 | 节点理论总算力 |
| `unavailable` | 没有足够证据 | 估值、确权、贷款额 |

## 7. 基准证据模型

`BenchmarkEvidence` 由固定 Benchmark Runner 生成。报告请求不得携带任意命令、脚本或镜像地址。

```text
benchmark_id                UUID
technical_snapshot_id       snapshot reference
suite                       AISBench / MLPerf / GPU-Burn / approved suite
suite_version               exact version
task                        normalized task id
metric                      tokens_per_second / ttft_ms / sustained_tflops
value                       decimal
unit                        canonical unit
workload_ref                model and dataset reference
workload_sha256             immutable workload digest
runner_id                   enrolled runner identity
runner_version              exact binary/container version
runtime_config              allowlisted structured parameters
started_at                  timestamp
finished_at                 timestamp
result_sha256               canonical result hash
signature                   detached signature
signer_key_id               runner signing key
verification_status         pending / verified / rejected
rejection_reason            optional normalized code
```

验证要求：

- 固定测试套件、固定参数范围、资源限制和超时。
- 结果签名覆盖任务、环境、指标、时间和技术快照引用。
- 基准模型与数据集使用不可变摘要。
- 异常高分、时间倒退、设备不匹配和重复结果进入人工复核。

## 8. 市场数据模型

市场数据分为原始观察、聚合价格快照和估值结果三层。禁止把单条挂牌信息直接写入报告估值。

### 8.1 MarketObservation

```text
observation_id              UUID
canonical_model_id          GPU model specification reference
configuration_hash          normalized configuration digest
device_form                 pcie_card / sxm / server / appliance
gpu_count                   integer
memory_per_gpu_bytes        integer
condition                   new / like_new / good / fair / parts
manufactured_at             optional date
commissioned_at             optional date
warranty_until              optional date
region                      ISO country/market code
currency                    ISO 4217
amount_minor                integer in minor currency unit
price_basis                 closed_deal / invoice / distributor_quote / listing / msrp
tax_included                boolean / unknown
shipping_included           boolean / unknown
observed_at                 timestamp
source_provider_id          private provider reference
source_record_hash          hashed external record id
source_license_ref          data authorization reference
evidence_sha256             exact evidence hash
ingested_at                 timestamp
verification_status         pending / verified / rejected
quality_score               0..100
```

唯一性建议：

```text
(source_provider_id, source_record_hash, observed_at)
```

金额统一使用最小货币单位整数，禁止使用浮点数保存货币。

### 8.2 MarketPriceSnapshot

```text
market_snapshot_id          UUID
canonical_model_id          GPU model reference
configuration_hash          normalized configuration digest
condition                   normalized condition
region                      market region
base_currency               normalized currency
window_start                aggregation window start
window_end                  aggregation window end
sample_count                total accepted samples
source_count                distinct accepted providers
closed_deal_count           accepted transaction samples
listing_count               accepted listing samples
minimum_minor               minimum accepted price
p25_minor                   25th percentile
median_minor                median
p75_minor                   75th percentile
maximum_minor               maximum accepted price
weighted_median_minor       source-quality weighted median
liquidity_days_median       optional time-to-sale metric
freshness_score             0..100
sample_score                0..100
source_diversity_score      0..100
configuration_match_score   0..100
confidence                  0..1
aggregation_policy_version  immutable policy version
snapshot_sha256             immutable aggregate hash
generated_at                timestamp
```

### 8.3 市场来源优先级

| 优先级 | 来源 | 用途 | 风险 |
|---|---|---|---|
| 1 | 已验证实际成交 | 核心可比价格 | 样本少、合同口径差异 |
| 2 | 已验证采购发票 | 历史成本和折旧起点 | 不代表当前市场价值 |
| 3 | 授权经销商报价 | 当前供给参考 | 报价未必成交 |
| 4 | 规范化挂牌信息 | 供给和流动性辅助 | 重复、虚高、长期未成交 |
| 5 | 厂商 MSRP | 上限或新品基准 | 与实际成交可能明显偏离 |

初始聚合策略建议：

- 实际成交优先于报价和挂牌。
- 配置不一致的样本必须调整或剔除。
- 地区、币种、税费和运输口径统一后再聚合。
- 使用中位数和分位区间，不直接使用算术平均。
- 异常值处理规则必须版本化，并保留剔除原因。
- 样本少于策略阈值时不输出点估值，只输出宽区间或“数据不足”。
- 数据新鲜度窗口由策略配置，不能硬编码在报告模板中。

初始置信度可由以下维度组成，权重必须通过历史回测确定：

```text
confidence = source_quality
           + freshness
           + sample_coverage
           + source_diversity
           + configuration_match
```

该置信度只描述市场估值的数据质量，不代表银行信用或违约概率。

## 9. 生命周期与健康模型

`AssetLifecycleEvidence` 用于 T1 调整和 T2 审核。

```text
asset_lifecycle_id          UUID
asset_identity_ref          private hashed identity reference
manufactured_at             optional date
purchased_at                optional date
commissioned_at             optional date
total_power_on_hours        optional integer
total_workload_hours        optional integer
warranty_until              optional date
repair_count                integer
major_component_changes     structured references
ecc_corrected_count         optional integer
ecc_uncorrected_count       optional integer
thermal_throttle_events     optional integer
stress_test_status          not_run / passed / failed
stress_test_ref             optional benchmark evidence
condition_grade             unverified / like_new / good / fair / poor
verified_at                 optional timestamp
verified_by                 optional reviewer/provider reference
evidence_refs               private evidence references
```

设备端数据可作为线索，但 `condition_grade`、采购时间和维修结论必须由私有证据或审核确认。

## 10. 权属证据模型

原始发票、合同和序列号不得进入 GPUFabric 或 new-api。

```text
ownership_evidence_id       UUID
assessment_id               formal assessment reference
evidence_type               invoice / contract / payment / inventory / registry
holder_ref                  private customer/organization reference
asset_identity_hash         private salted identity hash
document_object_ref         private object storage reference
document_sha256             exact document hash
issuer_ref                  document issuer reference
issued_at                   optional date
verification_status         pending / verified / rejected / expired
verified_by                 reviewer/provider reference
verified_at                 timestamp
rejection_reason            optional normalized code
retention_policy            policy reference
```

## 11. 运营收益与成本模型

该模型仅在采用收益法或出租能力分析时使用，不是 T0/T1 的必需条件。

```text
economics_snapshot_id       UUID
asset_ref                   assessment asset reference
window_start                period start
window_end                  period end
lease_utilization_percent   decimal
billable_compute_hours      decimal
token_or_job_revenue_minor  integer
energy_kwh                  decimal
energy_cost_minor           integer
hosting_cost_minor          integer
network_cost_minor          integer
maintenance_cost_minor      integer
downtime_hours              decimal
currency                    ISO 4217
source_refs                 billing and meter references
verification_status         pending / verified / rejected
```

收益预测必须区分历史事实、合同承诺和模型预测，不得混为同一字段。

## 12. 定价策略与估值结果

### PricingPolicy

```text
policy_id                   UUID
policy_version              immutable semantic version
status                      draft / approved / retired
effective_from              timestamp
effective_until             optional timestamp
supported_regions           region list
supported_asset_classes     asset class list
algorithm_digest            code/config digest
market_aggregation_version  market policy reference
depreciation_curve_version  curve reference
condition_adjustments       versioned configuration
warranty_adjustments        versioned configuration
liquidity_adjustments       versioned configuration
minimum_confidence          decimal
approved_by                 reviewer reference
approved_at                 timestamp
```

### ValuationResult

```text
valuation_id                UUID
assessment_id               assessment reference
technical_snapshot_id       GPUFabric snapshot reference
market_snapshot_id          market aggregate reference
policy_id                   pricing policy reference
method                      comparable / cost / income / blended
point_value_minor           optional integer
low_value_minor             integer
high_value_minor            integer
currency                    ISO 4217
confidence                  0..1
adjustment_factors          structured auditable factors
missing_evidence            normalized missing codes
calculated_at               timestamp
valuation_sha256            immutable result hash
```

估值模型不得继续采用简单的 `FP16 × 参考单价`。至少需要考虑：

- 精确型号、显存、设备形态和 GPU 数量。
- 新旧程度、设备年龄、保修和维修状态。
- 实测性能相对同型号基线的偏差。
- 地区、币种、税费和运输口径。
- 样本数量、价格区间、来源质量和流动性。

## 13. 技术预评估报告模型

`TechnicalPreEvaluationReport` 由 GPUFabric 自动生成。

```text
report_id                   PRE-YYYY-MM-<random>
schema_version              gpuf.technical_pre_evaluation_report.v1
report_status               generated / stale / expired
technical_snapshot_id       snapshot reference
generated_at                timestamp
valid_until                 timestamp
asset_summary               normalized non-sensitive summary
hardware_inventory          technical snapshot projection
runtime_summary             available telemetry and history
performance_summary         theoretical and measured values separated
technical_score             evidence/technical score
technical_grade             technical grade only
data_completeness           0..100
recommended_workloads       conditional recommendations
field_provenance            source metadata references
missing_evidence            normalized missing codes
warnings                    normalized warning codes
next_actions                collection/formal-assessment actions
valuation                   null unless trusted T1 result is attached externally
disclaimer                  mandatory non-credit disclaimer
report_sha256               immutable report hash
```

报告章节：

1. 基础备案与来源说明。
2. 硬件台账。
3. 运行状态与观测窗口。
4. 理论规格与实测性能。
5. 技术评分与完整度。
6. 推荐用途。
7. 缺失证据、风险和下一步动作。
8. 技术预评估结论与免责声明。

## 14. API 契约

### GPUFabric

当前兼容接口：

```text
POST /api/banking/provider/pre-evaluations/from-client
POST /api/banking/provider/pre-evaluations/challenge
POST /api/banking/provider/pre-evaluations/from-evidence
GET  /api/banking/provider/pre-evaluations/{reportId}
DELETE /api/banking/provider/pre-evaluations/{reportId}/evidence
```

目标 v2 接口：

```text
POST /internal/v1/technical-pre-evaluations/from-client
POST /internal/v1/technical-pre-evaluations/challenge
POST /internal/v1/technical-pre-evaluations/from-evidence
GET  /internal/v1/technical-pre-evaluations/{reportId}
GET  /internal/v2/technical-snapshots/{snapshotId}
```

所有创建接口要求：

- `clientRequestId` 幂等键。
- `tenantRef` 由受信任业务入口提供；GPUFabric 使用服务身份与 `tenantRef` 派生内部 request scope。
- 服务身份和 scope。
- 请求与响应 schema version。
- 可追踪 request id。
- 不在错误信息或日志中返回原始证据。

### asset-assessment-service

```text
POST /internal/v1/asset-assessments
GET  /internal/v1/asset-assessments/{assessmentId}
POST /internal/v1/asset-assessments/{assessmentId}/evidence
POST /internal/v1/asset-assessments/{assessmentId}/valuation
POST /internal/v1/asset-assessments/{assessmentId}/submit-review
POST /internal/v1/asset-assessments/{assessmentId}/issue
POST /internal/v1/asset-assessments/{assessmentId}/revoke
GET  /internal/v1/asset-assessments/{assessmentId}/report
```

### new-api

```text
POST /api/pre-evaluations
GET  /api/pre-evaluations/{reportId}
POST /api/asset-assessments
GET  /api/asset-assessments/{assessmentId}
GET  /api/asset-assessments/{assessmentId}/download
```

new-api 只保存用户映射、任务状态、摘要和报告引用。

## 15. 事件模型

建议事件：

```text
technical_snapshot.created
technical_pre_evaluation.generated
technical_pre_evaluation.expired
benchmark.verified
market_snapshot.generated
asset_assessment.evidence_completed
asset_assessment.review_requested
asset_assessment.issued
asset_assessment.revoked
```

事件公共字段：

```text
event_id
event_type
schema_version
occurred_at
producer
subject_id
correlation_id
payload_ref or minimal payload
payload_sha256
```

事件不得携带权属文件原文、客户身份材料或签章私钥信息。

## 16. 状态机

### 技术预评估

```text
requested -> collecting -> generated -> stale -> expired
                    \-> failed
```

- `generated`：已生成可查看报告，允许缺项。
- `stale`：底层遥测、规格或基准已更新，需要重新生成。
- `expired`：超过报告有效期。
- 技术预评估不进入“已签发”状态。

### 正式评估

```text
draft -> evidence_pending -> ready_for_review -> reviewing -> issued -> expired
                         \-> rejected                 \-> revoked
```

- 状态转换由私有服务控制并写入审计日志。
- `issued` 后报告内容不可变；修订必须生成新报告版本。
- `revoked` 不删除原报告，但禁止继续作为有效报告使用。

## 17. 隐私与安全要求

- GPUFabric 默认不保存离线原始证据，只保存 SHA-256。
- 序列号、UUID、WWN、合同、发票和客户身份不进入开源报告路径。
- 设备身份在私有服务中使用带租户隔离的加盐哈希映射。
- 市场提供方记录、授权协议和外部记录 ID 保持私有。
- 服务间使用 mTLS 或 OAuth2 client credentials，不只依赖 IP 白名单。
- 报告下载使用短时签名 URL，并记录用户、报告、时间和结果。
- 生产日志不得记录原始证据、Authorization Header 和完整客户标识。
- 数据库备份、只读副本和对象存储使用同一保留与删除政策。
- 签章私钥只存在 HSM 或独立签章服务中。

## 18. 开发路线

### Phase 0：边界冻结与兼容基线

目标：保持现有客户端协议和 v1 预评估接口可用。

- 固化当前 v1 API、示例和回归测试。
- 提交当前 `feature/pre-evaluation-report` 分支。
- 明确 v1 中估值、授信字段只作为兼容空字段，不接受调用方直接结果。
- 建立 OpenAPI/JSON Schema 基线和兼容策略。

验收：

- 在线和离线 v1 报告继续生成。
- 原始证据默认不落库。
- 旧 gpuf-c/common 协议不变。

### Phase 1：T0 技术快照 v2

目标：把技术快照与报告展示结构解耦。

- 实现 `TechnicalAssetSnapshot v2`。
- 为关键字段增加质量等级和 `source_ref`。
- 增加 `clientRequestId` 幂等处理。
- 扩充规范化缺失码和警告码。
- 建立技术快照不可变存储和版本关系。

验收：

- 每个报告关键字段可追溯来源。
- 同一幂等请求不会创建重复快照。
- 理论、观测和实测值不会混用。

### Phase 2：技术预评估报告与渲染

目标：自动生成可交付的 T0 报告。

- 实现 `TechnicalPreEvaluationReport v1`。
- 增加 HTML 渲染。
- 增加可选 PDF 渲染器，不包含机构签章。
- 增加报告过期、重新生成和旧版本查看。
- new-api 增加预评估申请、详情和报告展示。

验收：

- 数据不足的设备仍能生成报告。
- 缺失字段有明确解释和补证建议。
- HTML/PDF 与 JSON 快照哈希可关联。

### Phase 3：可信基准

目标：接入可验证的性能数据。

- 实现 Benchmark Runner 固定任务协议。
- 建立 runner 身份、签名和结果验证。
- 接入至少一个 LLM 基准和一个稳定性基准。
- 建立异常结果复核规则。

验收：

- 篡改、重放、设备不匹配的结果被拒绝。
- 报告明确区分理论能力与实测结果。

### Phase 4：市场数据与 T1 定价

目标：生成有来源、有区间和有置信度的参考估值。

- 新建私有 asset-assessment-service。
- 实现 MarketObservation 和 MarketPriceSnapshot。
- 接入第一批授权市场数据源。
- 实现币种、税费、地区、配置和成色归一化。
- 实现版本化 PricingPolicy 和 ValuationResult。
- 使用历史数据回测置信度和调整因子。

验收：

- 每个估值可追溯到市场快照和策略版本。
- 样本不足时不输出虚假精确值。
- 同一输入和策略版本可复算出同一结果。

### Phase 5：T2 确权、审核与签发

目标：生成正式评估报告。

- 实现权属证据登记和审核工作台。
- 实现正式评估状态机与审计日志。
- 接入 Signing Service / HSM。
- 实现签发、撤销、过期和新版本关系。
- new-api 增加正式评估任务、进度和下载授权。

验收：

- 正式报告签发后不可修改。
- 签章 PDF、报告 JSON、证据和策略版本可相互校验。
- 撤销和过期状态能阻止报告继续作为有效文件使用。

## 19. 第一开发迭代

第一迭代建议只实现 Phase 1，不同时启动市场和签章功能。

GPUFabric：

1. 定义 `technical_asset_snapshot.v2` JSON Schema。
2. 将当前在线和离线规范化结果映射到统一快照。
3. 为硬件、运行、规格和基准字段增加质量等级与来源引用。
4. 增加创建幂等键和快照版本关系。
5. 保留现有预评估 v1 API 兼容测试。

new-api：

1. 定义 `pre_evaluation_tasks`，不复用战略报告 `report_records`。
2. 保存用户、资产引用、GPUFabric 报告 ID、状态和错误码。
3. 使用服务身份调用 GPUFabric，不传递浏览器管理 Token。
4. 提供创建、状态和详情接口。

asset-assessment-service：

1. 只创建私有仓库和服务骨架。
2. 定义 assessment、evidence、market、pricing、audit 模块边界。
3. 建立 PostgreSQL 迁移和服务身份，不实现正式估值。

## 20. 开发前待确认事项

| 事项 | 决策责任方 | 阻塞阶段 |
|---|---|---|
| 第一批 GPU 型号规格范围 | GPUFabric 团队 | Phase 1 |
| 在线协议新增字段还是独立采集通道 | GPUFabric 团队 | Phase 1/3 |
| HTML/PDF 模板所有权 | 产品与技术 | Phase 2 |
| 第一套 LLM 和稳定性基准 | 性能团队 | Phase 3 |
| 市场数据提供方及授权范围 | 商务与法务 | Phase 4 |
| 成色分级和折旧政策 | 评估业务团队 | Phase 4 |
| 基准货币、税费和地区口径 | 财务与业务 | Phase 4 |
| 正式评估审核角色和权限 | 风控与合规 | Phase 5 |
| 签章服务或 HSM 供应方 | 安全与合规 | Phase 5 |

## 21. 开发完成定义

一个阶段只有同时满足以下条件才视为完成：

- Schema、迁移、API 文档和示例同步更新。
- 单元测试、数据库测试和端到端测试通过。
- 旧客户端兼容行为有自动化测试。
- 敏感数据不进入日志、仓库和错误响应。
- 每个报告关键结论可追溯到证据和策略版本。
- 失败、重试、幂等、过期和删除行为有测试。
- 本地部署链路和测试环境部署链路均通过。
