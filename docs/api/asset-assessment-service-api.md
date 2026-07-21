# asset-assessment-service API 契约

> 接口所有者：`asset-assessment-service`
> 版本：v1
> 网络：私有服务网络
> 主要调用方：new-api、评估服务内部工作流、授权审核后台

## 1. 边界

本文只定义由 `asset-assessment-service` 提供的接口。

本文不定义：

- new-api 面向浏览器提供的 `/api/...` 业务接口。
- GPUFabric 提供的技术报告和技术快照接口。
- 私有对象存储自身的 PUT/GET 协议。
- 银行贷款、支付、交易挂牌和用户登录接口。

## 2. 通用协议

### 2.1 请求头

```http
Authorization: Bearer <service-token>
Content-Type: application/json
X-Request-ID: req_01...
X-Correlation-ID: corr_01...
Idempotency-Key: as_01...
```

生产环境必须使用 TLS，推荐 mTLS 加 OAuth2 client credentials。禁止仅依赖 IP 白名单。

### 2.2 成功响应

```json
{
  "success": true,
  "data": {},
  "requestId": "req_01..."
}
```

### 2.3 错误响应

```json
{
  "success": false,
  "error": {
    "code": "ASSESSMENT_NOT_FOUND",
    "message": "assessment does not exist",
    "retryable": false,
    "details": {}
  },
  "requestId": "req_01..."
}
```

## 3. new-api 可调用接口

### 3.1 创建正式评估

```http
POST /internal/v1/asset-assessments
```

所需 scope：`assessment:create`

```json
{
  "clientRequestId": "as_01J0FORMAL001",
  "correlationId": "corr_01J0...",
  "tenantRef": "tenant_hmac_v1_...",
  "userRef": "user_hmac_v1_...",
  "assetRef": "asset_01J0GPU001",
  "requestedTier": "T1",
  "purpose": ["trading", "financing_pre_review"],
  "preEvaluation": {
    "provider": "gpufabric",
    "reportId": "PRE-2026-07-...",
    "reportSha256": "64-hex",
    "reportHtmlSha256": "64-hex",
    "schemaVersion": "gpuf.pre_evaluation.v1",
    "technicalSnapshotId": "TAS-2026-07-...",
    "technicalSnapshotSha256": "64-hex",
    "technicalSnapshotSchemaVersion": "technical_asset_snapshot.v2"
  },
  "callback": {
    "urlRef": "new-api-primary"
  }
}
```

响应 HTTP `202`：

```json
{
  "success": true,
  "data": {
    "assessmentId": "ASMT-2026-07-...",
    "status": "evidence_pending",
    "requestedTier": "T1",
    "requiredEvidence": [
      {
        "code": "ownership.invoice",
        "required": true,
        "reason": "OWNERSHIP_NOT_VERIFIED"
      }
    ],
    "technicalVerification": {
      "status": "verified",
      "reportId": "PRE-2026-07-...",
      "snapshotId": "TAS-2026-07-..."
    }
  }
}
```

幂等范围：`serviceSubject + tenantRef + clientRequestId`。

### 3.2 查询评估

```http
GET /internal/v1/asset-assessments/{assessmentId}
```

所需 scope：`assessment:read`

返回评估状态、进度、证据摘要、估值摘要、报告摘要、缺失码和下一步动作。不得返回原始证据 URL、市场样本明细和完整审核意见。

### 3.3 查询补证要求

```http
GET /internal/v1/asset-assessments/{assessmentId}/evidence-requirements
```

所需 scope：`assessment:read`

```json
{
  "success": true,
  "data": {
    "items": [
      {
        "code": "ownership.invoice",
        "required": true,
        "status": "missing",
        "allowedContentTypes": ["application/pdf", "image/jpeg", "image/png"],
        "maximumBytes": 10485760,
        "reason": "OWNERSHIP_NOT_VERIFIED"
      }
    ]
  }
}
```

### 3.4 创建证据上传会话

```http
POST /internal/v1/asset-assessments/{assessmentId}/evidence-sessions
```

所需 scope：`assessment:evidence`

```json
{
  "clientRequestId": "evs_01...",
  "evidenceType": "ownership.invoice",
  "contentType": "application/pdf",
  "contentLength": 123456,
  "fileName": "invoice.pdf",
  "sha256": "optional-64-hex"
}
```

```json
{
  "success": true,
  "data": {
    "evidenceId": "EVD-...",
    "uploadMethod": "PUT",
    "uploadUrl": "https://private-object-storage/...",
    "expiresAt": "2026-07-15T00:10:00Z",
    "requiredHeaders": {
      "Content-Type": "application/pdf"
    },
    "maximumBytes": 10485760
  }
}
```

`uploadUrl` 是临时凭证，不得持久化到 new-api 数据库或写入日志。

### 3.5 创建报告下载授权

```http
POST /internal/v1/asset-assessments/{assessmentId}/report-downloads
```

所需 scope：`assessment:download`

```json
{
  "clientRequestId": "dl_01...",
  "requesterRef": "user_hmac_v1_...",
  "purpose": "user_download",
  "expiresInSeconds": 120
}
```

只有 `issued` 且未撤销、未过期的报告可以生成下载授权。

## 4. 仅评估内部可调用接口

这些接口不向 new-api 普通服务身份开放。

### 4.1 对象上传完成事件

```http
POST /internal/v1/asset-assessments/{assessmentId}/evidence/{evidenceId}/upload-completions
```

调用主体必须绑定为 `object-storage-gateway`，所需 scope 为 `assessment:evidence:complete`。请求中的 `eventId` 必须与 `Idempotency-Key` 相同。

```json
{
  "eventId": "upload_evt_01...",
  "contentLength": 123456,
  "sha256": "64-hex"
}
```

服务重新比较上传会话中的长度和可选预声明 Hash。匹配后证据进入 `scanning`；长度、Hash 或有效期不匹配时持久化为 `rejected` 或 `expired`，不会进入审核。

### 4.2 恶意文件扫描结果

```http
POST /internal/v1/asset-assessments/{assessmentId}/evidence/{evidenceId}/scan-results
```

调用主体必须绑定为 `evidence-scanner`，所需 scope 为 `assessment:evidence:scan`。

```json
{
  "eventId": "scan_evt_01...",
  "status": "clean",
  "detectedContentType": "application/pdf",
  "sha256": "64-hex",
  "reasonCode": "SCAN_CLEAN"
}
```

`status` 只允许 `clean` 或 `infected`。服务必须再次比较对象 Hash，并按证据类型校验实际 MIME。感染、Hash 不一致或 MIME 不匹配均进入 `rejected`。

### 4.3 人工证据审核

```http
POST /internal/v1/asset-assessments/{assessmentId}/evidence/{evidenceId}/review-actions
X-Reviewer-Ref: reviewer_hmac_v1_...
```

调用主体必须绑定为 `assessment-reviewer`，所需 scope 为 `assessment:review`。

```json
{
  "clientRequestId": "evidence_review_01...",
  "action": "verify",
  "reasonCode": "MANUAL_REVIEW_VERIFIED"
}
```

`action` 只允许 `verify` 或 `reject`。完整审核意见不得通过该接口、状态回调或通用审计表传输。全部必需证据验证后，评估状态原子转换为 `ready_for_valuation`。

验证 `asset.lifecycle` 时必须额外提交从材料中提取的结构化事实，其他证据类型和拒绝动作禁止携带该字段：

```json
{
  "clientRequestId": "evidence_review_lifecycle_01...",
  "action": "verify",
  "lifecycleFacts": {
    "condition": "good",
    "manufacturedAt": "2024-03-01T00:00:00Z",
    "commissionedAt": "2024-04-15T00:00:00Z",
    "warrantyUntil": "2027-04-15T00:00:00Z"
  }
}
```

`manufacturedAt` / `commissionedAt` 至少一项存在。服务保存 `sourceEvidenceId` 和审核时间，不把用户自报成色当成正式事实。

### 4.4 写入市场样本

```http
POST /internal/v1/market-observations
```

调用主体必须绑定为 `market-data-provider`，所需 scope 为 `market:write`。

该接口只接收授权来源的规范化最小价格事实：配置 Hash、型号、地区、币种、成色、成交价或挂牌价、来源记录 Hash、证据 SHA-256 和私有对象引用。`configurationHash` 必须能按 `gpuf.asset-configuration-lines.v1` 从型号、形态、GPU 数量和逐卡显存重算。普通读取接口不会返回 `rawObjectRef`，也不接受调用方直接提交市场快照或估值金额。

### 4.5 获取待核验市场样本

```http
GET /internal/v1/market-observation-verification-jobs?limit=50
```

调用主体必须绑定为 `market-data-verifier`，所需 scope 为 `market:verify`。该隔离队列返回待处理价格事实、来源/证据 Hash 和私有 `rawObjectRef`；市场提供方与普通 `market:read` 身份不能访问。

### 4.6 核验市场样本

```http
POST /internal/v1/market-observations/{observationId}/verification-actions
```

调用主体必须绑定为 `market-data-verifier`，所需 scope 为 `market:verify`。只有 `verified` 样本可以进入市场快照；`reject` 必须给出 `reasonCode`。

### 4.7 生成不可变市场快照

```http
POST /internal/v1/market-price-snapshots
```

调用主体必须绑定为 `market-snapshot-worker`，所需 scope 为 `market:snapshot`。服务按同配置、地区、币种、成色和时间窗聚合已核验样本；少于 3 条样本或少于 2 个来源时返回 `MARKET_DATA_INSUFFICIENT`。快照包含样本量、来源数、分位数、中位数、置信度和 `snapshotSha256`。

### 4.8 查询市场快照

```http
GET /internal/v1/market-price-snapshots/{snapshotId}
```

所需 scope：`market:read`

### 4.9 创建估值策略

```http
POST /internal/v1/pricing-policies
```

调用主体必须绑定为 `pricing-policy-author`，所需 scope 为 `pricing:write`。服务将该认证主体持久化为 `authoredBy`；新策略只以 `draft` 状态落库，不能被估值执行使用。

```json
{
  "policyVersion": "gpu-comparable-us-v1",
  "effectiveFrom": "2026-07-17T00:00:00Z",
  "supportedRegions": ["US"],
  "supportedAssetClasses": ["gpu", "pcie_card"],
  "algorithmDigest": "64-hex",
  "marketAggregationVersion": "market_aggregation.v1",
  "depreciationCurveVersion": "depreciation.none.v1",
  "conditionAdjustments": {"good": 0.9},
  "warrantyAdjustments": {"default": 1},
  "liquidityAdjustments": {"US": 1},
  "minimumConfidence": 0.5
}
```

调整因子当前只允许 `0 < factor <= 1`，用于保守下调，不允许通过策略把快照价格向上抬高。

### 4.10 审批估值策略

```http
POST /internal/v1/pricing-policies/{policyId}/approval-actions
```

调用主体必须绑定为 `pricing-policy-approver`，所需 scope 为 `pricing:approve`。请求中的 `eventId` 必须与 `Idempotency-Key` 相同。服务强制审批服务主体与策略 `authoredBy` 不同，即使错误地给同一凭证同时配置两个 scope 也会拒绝。只有 `approved` 且处于有效期内的策略可以用于估值。

```json
{
  "eventId": "pricing_approval_evt_01...",
  "approverRef": "pricing_committee_hmac_v1_..."
}
```

### 4.11 执行估值

```http
POST /internal/v1/asset-assessments/{assessmentId}/valuation
```

所需 scope：`assessment:valuation:execute`

调用主体必须绑定为 `valuation-worker`。服务只接受 `technicalSnapshotId`、`marketSnapshotId`、`policyVersion` 和 `method`，不会接受调用方提交价格、质押率或可贷额度。估值必须同时满足：

- 评估状态为 `ready_for_valuation`，且技术快照 ID 与已验证的 GPUFabric 技术底稿一致。
- 技术快照中的 `assetConfiguration` Hash 已独立重算，所有配置字段与市场快照完全相同。
- 市场快照成色与已审核 `asset.lifecycle` 的 `assetCondition` 相同；保修日期只通过已审批策略的 `covered/expired/unknown` 调整项生效。
- 市场快照已由服务端从已核验样本生成，且快照置信度不低于策略阈值。
- 策略版本已审批、生效，且市场聚合版本、地区和资产类别匹配。

```json
{
  "technicalSnapshotId": "TAS-2026-07-...",
  "marketSnapshotId": "MKTS-20260717-...",
  "policyVersion": "gpu-comparable-us-v1",
  "method": "comparable"
}
```

重复执行同一输入会按 `valuationSha256` 返回同一个结果。

### 4.12 查询估值结果

```http
GET /internal/v1/valuations/{valuationId}
```

所需 scope：`assessment:valuation:read`

### 4.12 提交正式评估审核

```http
POST /internal/v1/asset-assessments/{assessmentId}/submit-review
```

所需 scope：`assessment:formal-review:submit`，调用主体绑定为 `valuation-worker`。请求只引用服务内已持久化且属于当前评估的估值结果：

```json
{
  "clientRequestId": "formal_review_submit_01...",
  "valuationId": "VAL-20260717-..."
}
```

服务重新校验租户、评估状态、`assessmentId` 与 `valuationSha256`，通过后原子进入 `review_pending`。

### 4.13 分配主审和复审

```http
POST /internal/v1/asset-assessments/{assessmentId}/review-assignments
```

所需 scope：`assessment:formal-review:assign`

```json
{
  "clientRequestId": "review_assign_01...",
  "role": "primary",
  "reviewerRef": "reviewer_hmac_v1_..."
}
```

`role` 只允许 `primary` 或 `secondary`，同一 `reviewerRef` 不得同时占用两个角色。审核后台必须从已认证人员身份生成 `reviewerRef`，不能直接信任浏览器自填请求头。

### 4.14 正式评估审核动作

```http
POST /internal/v1/asset-assessments/{assessmentId}/review-actions
```

所需 scope：`assessment:formal-review:action`，并携带由审核后台认证会话绑定的 `X-Reviewer-Ref`。

允许动作：

```text
start_review
approve
request_changes
reject
```

主审必须先执行 `start_review` 和 `approve`，之后不同的复审人才能执行最终 `approve` 并把评估转为 `approved`。`request_changes` 和 `reject` 必须带 `reasonCode` 与私密 `opinion`；完整意见只保存在审核动作表和授权读取接口，不进入 assessment 查询摘要或回调。

审核请求模型不包含估值金额、区间、LTV、可贷额度、市场快照替换或策略替换字段；出现这些未知字段时返回 `INVALID_ARGUMENT`。需要调整时只能 `request_changes`，并通过证据、市场快照和已审批策略重新计算。

```http
GET /internal/v1/asset-assessments/{assessmentId}/review
```

所需 scope：`assessment:formal-review:read`。返回最新审核 case 及按时间排序的不可变动作记录。

### 4.15 冻结内部报告

```http
POST /internal/v1/asset-assessments/{assessmentId}/report-freezes
```

所需 scope：`assessment:report:freeze`，调用主体绑定为 `report-freeze-worker`。默认 `ASSESSMENT_ENABLE_FORMAL_REPORTS=false`；即使开启，也只有 assessment 为 `approved`、FormalReview 为 `approved`、技术/市场/策略/估值引用一致且全部必需证据为 `verified` 时才允许冻结。

请求只包含幂等键，不接受金额、市场快照、估值策略、证据、PDF 或签章字段：

```json
{
  "clientRequestId": "freeze_01..."
}
```

冻结结果的 `reportStatus` 固定为 `frozen`，只保存 canonical JSON、确定性 HTML 和对应 SHA-256。当前不生成 PDF、不进入 `issued`、不调用 HSM、不创建下载授权，也不向 `new-api` 暴露内容。

```http
GET /internal/v1/reports/{reportId}
GET /internal/v1/reports/{reportId}/frozen-content
```

读取分别需要 `assessment:report:read` 和 `assessment:report:content`，均按 `X-Tenant-Ref` 隔离。内容读取会重新校验 JSON/HTML 字节 Hash，审核 opinion 和原始对象键不会进入冻结文档。

### 4.16 签发报告

```http
POST /internal/v1/asset-assessments/{assessmentId}/issue
```

所需 scope：`assessment:issue`

调用主体必须绑定为 `report-signing-worker` 或等价的内部签发身份。`new-api`、普通审核身份和用户 Token 不得调用该接口。

报告冻结由 assessment-service 完成，随后由 Signing Service 调用 HSM。HSM 只接收冻结报告摘要、签名配置和受控密钥引用，不接收原始证据、市场样本、估值规则或用户业务数据；私钥和可导出密钥材料不得离开 HSM。

当前状态必须为 `approved`，并且证据、市场快照、定价策略和估值结果已经冻结。

### 4.17 撤销报告

```http
POST /internal/v1/asset-assessments/{assessmentId}/revoke
```

所需 scope：`assessment:revoke`

撤销后必须停止签发下载授权，并向 new-api 发送撤销事件。

## 5. 健康检查

```http
GET /healthz
GET /readyz
```

健康检查接口不返回数据库 DSN、依赖地址、密钥状态详情和内部版本指纹。

## 6. 本服务主动调用的外部接口

### 6.1 调用 GPUFabric

接口定义见 [GPUFabric 评估数据 API](gpufabric-assessment-api.md)。

```text
GET /internal/v1/technical-pre-evaluations/{reportId}
GET /api/banking/provider/pre-evaluations/{reportId}/html
GET /internal/v2/technical-snapshots/{snapshotId}
```

### 6.2 回调 new-api

接口定义见 [new-api 资产评估 API](new-api-asset-assessment-api.md)。

```text
POST /api/banking/callback/assessment
```

评估服务是该回调的调用方，不是接口所有者。

`reportHtmlSha256` 为兼容旧调用方的可选字段；提供后，评估服务必须同时验证 HTML 响应字节和 `X-Content-SHA256`。

## 7. 状态

```text
created
technical_fetching
technical_verified
technical_rejected
evidence_pending
evidence_reviewing
ready_for_valuation
valuating
valuation_ready
review_pending
reviewing
changes_requested
approved
issuing
issued
rejected
revoked
expired
failed
```

## 8. 主要错误码

```text
ASSESSMENT_NOT_FOUND
IDEMPOTENCY_CONFLICT
INVALID_STATE_TRANSITION
STATE_VERSION_CONFLICT
TECHNICAL_HASH_MISMATCH
TECHNICAL_SCHEMA_UNSUPPORTED
EVIDENCE_REQUIRED
EVIDENCE_REJECTED
MARKET_DATA_INSUFFICIENT
PRICING_POLICY_NOT_APPROVED
SIGNING_SERVICE_UNAVAILABLE
REPORT_REVOKED
REPORT_EXPIRED
```

完整业务、安全和状态机要求见
[asset-assessment-service 对接与开发规范](../asset-assessment-service-integration.md)。
