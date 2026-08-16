# 正式资产评估报告生产上线与无 Mock 运行手册

文档状态：生产上线评审稿
适用范围：GPUFabric、gpuf-s/gpuf-c、asset-assessment-service、new-api 接入、证据存储与扫描、市场与定价、正式审核、报告签名与时间戳
更新时间：2026-08-16

## 1. 上线原则

生产签发的正式资产评估报告必须全部使用可追溯的真实数据。任何 Mock、测试夹具、合成市场、自动测试审核、SoftHSM 测试签名、本地非可信时间戳或 `test_only` 来源都不得进入生产报告。

以下规则没有降级例外：

1. 真实依赖缺失时，流程只能停在 `evidence_pending`、`ready_for_valuation`、`review_pending` 或 `frozen`，不得以测试数据补齐后签发。
2. Signer、TSA、IAM、市场源或对象存储故障时必须 fail closed，不得切换到 SoftHSM、本地 TSA、固定 reviewer 或合成市场。
3. 测试环境生成的报告即使字段完整，也只能保持 `testOnly=true`、`productionEligible=false`，不得改名、复制或回填为生产正式报告。
4. 已签发报告不可修改。数据或信任来源发生变化时必须创建新的评估链路和新报告。
5. 浏览器、new-api 或人工操作员不得直接提交 Benchmark 数值、估值金额、报告 Hash、trust level、签名或时间戳 token。

当前目录 `/home/jack/桌面/e5dd-正式报告依赖数据-测试环境-20260816` 及其中三份 `Local E2E Evidence` PDF 只用于测试联调，明确禁止进入生产。

## 2. 当前基线和上线结论

当前 `ssh test` 已验证完整技术链路，但仍是测试环境：使用测试材料、合成市场、测试定价策略、测试审核身份、SoftHSM、非可信本地时间戳和测试对象存储。因此当前系统可以生成“测试环境正式结构报告”，不能直接对外宣称为具备机构或法律效力的生产正式报告。

在企业实名审核、机构 HSM/KMS、正式 RFC 3161 TSA、生产 S3/OSS/KMS/Scanner 尚未齐备时，全链路 Beta 可以在隔离白名单内使用固定 reviewer、合成市场、Beta MinIO、SoftHSM 和本地 TSA 完成技术联调，但必须强制 `testOnly=true`、`productionEligible=false`，不得对外称为正式报告。详细阶段、Go/No-Go、监控和回滚见 `docs/asset-assessment-beta-production-rollout-plan-cn.md`。实名人工审核另从干净基线创建 `feature/production-human-review-identity` 实现，禁止混入当前测试发布分支。

以下当前测试值必须加入生产发布禁止清单，不能复制、改名或跨环境复用：

| 测试项 | 当前禁止值/特征 |
|---|---|
| Evidence | 三份 `Local E2E Evidence` PDF 及其现有 Hash |
| Benchmark key | `gpuf-online-test-2026-08` |
| 市场快照 | `MKTS-20260815-a3292efcf9133d72` |
| 定价策略 | `local-e2e-pricing-20260815T030657-84381ab9` |
| 审核身份 | `test-evidence-reviewer-e5dd`、`test-primary-reviewer-e5dd`、`test-secondary-reviewer-e5dd` |
| 签名 key | `local-e2e-p256-v1` |
| 时间戳 | `local-e2e-untrusted-timestamp` |
| 存储 | 当前测试 MinIO bucket、凭据和对象版本 |
| 已有正式结构报告 | `AER-20260815-11a179ef1deece57` 及其 assessment/evidence/review/valuation ID |

同一真实设备未来可以重新入驻生产，但必须在生产租户中重新绑定、重新采集、重新取得 Benchmark、上传真实材料并生成全新的市场、估值、审核和报告 ID。

生产代码已经具备以下 fail-closed 边界：

- 生产 workflow 与测试自动化不能同时启用；
- 生产市场快照、定价策略、issuer 和 revoker 标识拒绝包含 `test`、`mock`、`synthetic`、`fixture` 或 `demo`；
- 生产 workflow 只自动执行估值并提交人工审核，不会自动分配 reviewer、批准、冻结或签发；
- 生产签发要求真实企业身份、MFA、独立双审、签发授权策略、production Signer 和 production TSA；
- Signer/TSA capability 不满足时报告保持 `frozen`；
- 新冻结报告固化 `collected/measured/catalog/reviewer/derived` 来源和 trust level。

标识过滤只能阻止明显测试配置，不能证明 PDF、市场成交或人工意见真实。外部数据真实性仍必须通过受控入驻、原始凭证、独立复核和审计制度保证。

## 3. 正式报告无 Mock 数据门禁

| 数据域 | 生产允许来源 | 必须验证 | 禁止项 | 失败状态 |
|---|---|---|---|---|
| 设备身份与资产绑定 | 实际 gpuf-c、实名资产所有人、服务端 OwnerScope | client/asset/user/tenant 绑定、设备状态、请求幂等 | 测试 client 映射、浏览器覆盖资产身份 | 拒绝创建 |
| 硬件与健康观测 | gpuf-c/gpuf-s 实际采集 | 原始采集 Hash、采样覆盖、设备来源、时间窗口 | 手填 GPU、温度、功耗、ECC 或把“不支持”改为 0 | `technical_rejected` |
| 规格目录 | 经批准的厂商/目录版本 | 型号匹配、目录版本、来源和更新时间 | 无来源理论性能、临时表格 | `technical_rejected` 或不出具该字段 |
| Benchmark | 新版 gpuf-c 内置受控 workload 或获批等价 Runner | challenge、sourceRef、参数 Hash、生产密钥、testedAt、expiresAt、T1/T2 分类 | 手填结果、测试 key、过期 evidence | `technical_rejected` |
| 所有权材料 | 资产所有人真实发票/合同 | 原文件 Hash、对象版本、病毒扫描、实名 reviewer 核验 | E2E PDF、模板发票、自动 test verify | `changes_requested`/`rejected` |
| 生命周期材料 | 真实采购、制造、启用、保修、维修或检验记录 | 材料 Hash、OCR 对照、condition 与日期事实、reviewer 审计 | 固定 lifecycleFacts、无凭证成色 | `changes_requested`/`rejected` |
| 市场数据 | 已签约授权的成交或发票记录 | provider、原始对象、sourceRecordHash、独立 verifier、保留许可 | 合成成交、挂牌价代替成交价、测试快照 | 保持 `ready_for_valuation` |
| 定价策略 | 评估机构批准的版本化策略 | draft/approve 分离、生效范围、算法摘要、版本 | 测试策略、未审批参数、浏览器提交价格 | 不执行估值 |
| 估值 | 服务端使用已存市场快照和已批准策略推导 | 可复算 Hash、配置/成色/区域/币种一致性 | 人工直接填金额或覆盖区间 | 不提交正式审核 |
| 正式审核 | 企业 IAM 中两个不同实名审核人 | active、MFA、RBAC、授权快照、primary/secondary 顺序 | `test-*` reviewer、同人双审、请求头冒充 | 保持 `review_pending` |
| 报告签名 | 机构 HSM/KMS/PKCS-11 | production capability、不可导出密钥、证书链、key version、审计引用 | SoftHSM、导出私钥、测试证书 | 保持 `frozen` |
| 可信时间戳 | 正式 RFC 3161 TSA | production、policy OID、nonce、摘要、CMS/证书链、genTime、审计引用 | 本地时间、合成 token、测试 TSA | 保持 `frozen` |
| 对象存储 | 生产私有 S3/OSS、KMS 和对象事件 | bucket/prefix/tenant/version/length/MIME/SHA-256、加密和事件认证 | 公共 bucket、测试 MinIO、HMAC-only 完成回调作为生产主路径 | 上传或签发不推进 |

每份生产报告在冻结前必须生成“生产来源清单”，至少记录：source kind、source ref、SHA-256、提供方/审核主体、verifiedAt、expiresAt、授权或审计引用和 trust level。清单中出现 `test_only` 或来源不完整时禁止冻结。

## 4. 需要人工入驻和录入的内容

人工录入必须通过受控内部 API 或审核工作台完成，禁止直接修改数据库。

| 入驻对象 | 人工提供或录入 | 审批要求 | 系统自动产生 |
|---|---|---|---|
| 资产所有人/运营方 | 企业身份、资产权属、发票、合同、生命周期与检验材料 | OwnerScope 和材料审核 | evidenceId、对象 key、Hash 校验、状态 |
| gpuf-c 设备运营方 | 安装并运行新版 gpuf-c，绑定真实 client | 设备与资产归属确认 | 硬件、健康观测、BenchmarkEvidence |
| 证据审核员 | 材料 verify/reject、拒绝/补件原因、生命周期事实 | 实名、MFA、最小权限 | 审计、状态推进、来源投影 |
| 市场数据提供方 | 成交/发票记录、原始凭证、授权与保留期限 | 数据合同和 provider 身份 | observationId、Hash、规范化字段 |
| 市场独立核验员 | 对市场样本 verify/reject | 与提供方职责分离 | 可进入快照的状态 |
| 定价策略管理员 | 策略参数、适用区域/资产类别、算法与版本 | 独立策略审批人批准 | policyId、版本和摘要 |
| Primary reviewer | 正式评估第一次独立审核 | 企业 IAM、MFA、角色授权 | 审核动作 Hash 和授权快照 |
| Secondary reviewer | 第二次独立审核 | 必须与 primary 不同且后执行 | 最终 approved 状态 |
| 报告签发人 | 签发授权动作 | `formal_report_issuer` 和签发策略 | PDF、签名、TSA、issued 状态 |
| 紧急撤销人 | reasonCode、授权编号、事件编号 | `formal_report_revoker` | revoked 状态和审计事件 |
| 安全/CA 团队 | HSM/KMS、证书链、轮换/吊销策略、TSA roots/OID | 安全评审与演练 | capability 和签名验证结果 |
| 云平台/SRE | 私有 bucket、KMS、事件桥、告警、备份/恢复 | 平台与安全验收 | 短时 PUT/GET grant、版本事件 |

市场数据可以人工录入，但每条记录必须附真实原始凭证、provider 身份、sourceRecordHash 和保留许可，并由另一个身份核验。仅手工填写“某设备价格为 X”不构成正式市场证据。

以下字段绝对不能人工录入：Benchmark 数值、GPU 观测结果、assetConfiguration Hash、估值点值/区间、reportId、报告 Hash、fact provenance、trust level、签名、证书摘要、TSA token。

## 5. 生产外部依赖入驻门禁

上线评审必须为每项依赖保存负责人、生产地址、凭据引用、证书/策略版本、健康检查、轮换日期、故障联系人和验收证据。

1. 企业 IAM：SSO/MFA/RBAC/SCIM 或等价服务，包含 evidence reviewer、market verifier、pricing admin/approver、formal review admin/reviewer、issuer、revoker。
2. 市场源：至少两个独立授权 provider；市场快照仅聚合精确匹配配置、成色、区域和币种的合格成交/发票记录。
3. 定价治理：正式策略必须先 draft 后独立 approve，记录生效时间、适用范围和算法摘要。
4. 对象存储：证据与报告使用独立私有 bucket/prefix 和身份；启用 versioning、KMS/SSE、事件桥、备份/复制和生命周期策略。
5. Scanner/OCR：ClamAV 签名更新、OCR 数据处理、隔离和短时下载授权通过安全验收。
6. 机构 Signer：`institutional-http` 对接真实 HSM/KMS/PKCS-11，私钥不可导出，证书链正式受信。
7. RFC 3161 TSA：正式 HTTPS endpoint、policy OID、根链、时间戳证书和审计合同。
8. 监控告警：对象事件积压/死信、审核 SLA、workflow 重试、回调 outbox、Signer/TSA 健康、证书和 Benchmark 到期。

任何一项未入驻时，生产签发开关不得开启。

## 6. 生产配置硬门禁

生产配置至少满足：

```dotenv
ASSESSMENT_ENABLE_TEST_AUTOMATION=false
ASSESSMENT_ENABLE_PRODUCTION_WORKFLOW=false
ASSESSMENT_ENABLE_FORMAL_REPORTS=false
ASSESSMENT_ENABLE_REPORT_LIFECYCLE=false

ASSESSMENT_EVIDENCE_STORAGE_PROVIDER=oss
ASSESSMENT_ENABLE_EVIDENCE_OBJECT_EVENTS=true
ASSESSMENT_REVIEWER_IDENTITY_URL=https://enterprise-identity.internal
ASSESSMENT_REPORT_ISSUANCE_POLICY_VERSION=<approved-production-policy>
ASSESSMENT_REPORT_AUTHORIZED_ISSUER_SUBJECTS=<approved-issuer-subjects>
ASSESSMENT_REPORT_AUTHORIZED_REVOKER_SUBJECTS=<approved-revoker-subjects>
ASSESSMENT_SIGNING_SERVICE_URL=https://institutional-signing.internal
ASSESSMENT_SIGNING_TRUST_ROOTS_PATH=/run/secrets/production-report-roots.pem
ASSESSMENT_REPORT_STORAGE_PROVIDER=oss
```

示例选择原生 `oss`；使用经过验收的 S3 时，两处 provider 均改为精确值 `s3`。凭据必须通过 secret manager/workload identity 注入，不能写入 Compose、镜像、Git、终端历史或发布清单。

Report Support 必须使用 `REPORT_SUPPORT_SIGNER_MODE=institutional-http`。其 health/capability 必须返回 production signer、production TSA、active/non-exportable key 和受信证书链。生产不得暴露或调用 `/internal/v1/test-trust`。

开关顺序：

1. 首次部署保持四个开关均为 `false`，只验证健康、只读接口和依赖连接。
2. 真实市场快照、正式策略和 IAM 完成后，先对白名单真实资产配置 `ASSESSMENT_PRODUCTION_WORKFLOW_ASSET_REF`，再开启 production workflow。
3. 完成人工双审、Signer/TSA 和存储验收后，才同时开启 formal reports 与 report lifecycle。
4. 首份真实报告独立验签并完成业务签字后，才逐步扩大资产范围。

## 7. 上线前备份

### 7.1 必备备份范围

- GPUFabric 数据库：设备、健康观测、BenchmarkEvidence、预评估报告、技术快照和 keyring 元数据；
- asset-assessment 数据库：assessment、evidence、object event、market、policy、valuation、review、report、provenance、audit、outbox、workflow jobs 和 migration checksums；
- new-api 数据库：资产绑定、预评估/正式评估任务及报告投影；
- 证据和报告对象存储：对象版本、Hash、加密 key 引用、retention 和 inventory；
- 服务配置、Compose/Kubernetes 清单、镜像 digest、迁移文件 Hash、证书公钥链和 secret version 引用；
- IAM/策略/HSM/TSA 配置快照和审批记录。HSM 私钥不得导出到备份。

### 7.2 数据库备份模板

以下为模板，生产执行前必须替换容器、角色、数据库和受控目录：

```bash
release_id="$(date -u +%Y%m%dT%H%M%SZ)-asset-assessment"
backup_root="/var/backups/gpunexus-asset-assessment/${release_id}"
pg_container="<production-postgres-container>"
pg_role="<production-backup-role>"
gpufabric_db="<production-gpufabric-db>"
assessment_db="<production-assessment-db>"
new_api_db="<production-new-api-db>"

install -d -m 0700 "${backup_root}/database" "${backup_root}/services"
docker exec "${pg_container}" pg_dumpall -U "${pg_role}" --globals-only \
  > "${backup_root}/database/postgres-globals.sql"
docker exec "${pg_container}" pg_dump -U "${pg_role}" -d "${gpufabric_db}" \
  --format=custom --compress=6 --no-owner --no-acl \
  > "${backup_root}/database/gpufabric.dump"
docker exec "${pg_container}" pg_dump -U "${pg_role}" -d "${assessment_db}" \
  --format=custom --compress=6 --no-owner --no-acl \
  > "${backup_root}/database/asset-assessment.dump"
docker exec "${pg_container}" pg_dump -U "${pg_role}" -d "${new_api_db}" \
  --format=custom --compress=6 --no-owner --no-acl \
  > "${backup_root}/database/new-api.dump"

pg_restore --list "${backup_root}/database/gpufabric.dump" >/dev/null
pg_restore --list "${backup_root}/database/asset-assessment.dump" >/dev/null
pg_restore --list "${backup_root}/database/new-api.dump" >/dev/null
sha256sum "${backup_root}"/database/* > "${backup_root}/database/SHA256SUMS"
sha256sum --check "${backup_root}/database/SHA256SUMS"
```

备份必须同时保存在主机外的受控位置并再次校验。仅存在生产主机上的备份不满足上线门禁。

### 7.3 对象存储备份

1. 上线前确认 evidence/report bucket 均开启 versioning、禁止公共访问和 KMS/SSE。
2. 导出 bucket policy、KMS key ID、lifecycle、replication、event notification 和对象 inventory。
3. 对冻结/已签发报告保存 object version ID、内容长度、MIME 和 SHA-256 清单。
4. 启用跨账号或跨区域复制；复制端不得使用与生产写入身份相同的删除权限。
5. 至少完成一次从副本恢复到隔离 bucket 的演练并重新验证报告 Hash。

### 7.4 建议恢复目标

建议值需由业务和 SRE 正式批准：数据库/事件队列 RPO 不超过 15 分钟，应用回滚 RTO 不超过 30 分钟，协调数据恢复 RTO 不超过 2 小时。已签发报告对象要求零逻辑丢失，应通过 versioning、复制和不可变保留实现。

## 8. 生产发布步骤

### 8.1 发布前停止条件

出现以下任一情况立即停止：

- 任何真实依赖尚未入驻；
- 任一备份为空、无法 `pg_restore --list` 或 checksum 不一致；
- 目标镜像、二进制或迁移没有固定 digest/SHA-256；
- 配置出现 test/mock/synthetic/fixture/demo 标识；
- 测试自动化仍开启；
- Signer/TSA capability 不是 production/ready；
- reviewer 身份不能验证 active/MFA/RBAC；
- 对象存储未启用私有访问、加密、versioning 或可靠事件；
- 无真实且经授权的市场数据和已批准策略；
- 没有一项可执行的回滚和已验证异地备份。

### 8.2 分阶段上线

1. **构建与制品**：尽可能本地从干净提交构建，执行单元、集成、迁移、兼容和安全测试；记录 commit、镜像 digest、SBOM、迁移 Hash。
2. **暗部署**：使用生产配置但保持 workflow/formal/lifecycle 关闭；只开放健康检查和受控内部只读验证。
3. **迁移**：按版本顺序应用加法迁移；验证 migration checksum、索引、约束和不可变触发器；禁止直接执行回滚 SQL。
4. **对象链路**：用真实但非正式签发的受控文件验证 PUT、对象事件、version、Hash/MIME/length、Scanner、DLQ 和 polling 恢复。
5. **真实资产 Canary**：必须使用获得授权的真实内部资产和真实材料。若没有真实数据，只能在测试环境演练，不得在生产生成假正式报告。
6. **生产 workflow**：先限制 `ASSESSMENT_PRODUCTION_WORKFLOW_ASSET_REF`，验证只执行估值并停在人工审核队列。
7. **人工双审**：两个不同实名 reviewer 完成审核，核对生命周期事实、市场快照和估值复算结果。
8. **签发开关**：确认 HSM/TSA/证书/issuer 门禁后开启正式报告生命周期，签发首份真实报告。
9. **独立验签**：在服务外验证 PDF Hash、detached signature、证书链、RFC 3161 token、reportId、issuedAt 和 validUntil。
10. **逐步放量**：观察至少一个完整业务周期后再扩大资产范围；不得一次性取消所有白名单。

### 8.3 首份报告验收

- report JSON/HTML/PDF 三类 Hash 与服务端记录一致；
- `factProvenance` 不含 `test_only`，所有事实有来源；
- T1/T2 Benchmark policy 为 `satisfied` 且冻结时未过期；
- 三类 T2 材料来自真实对象版本并已扫描、实名审核；
- 市场快照满足至少 3 笔合格成交、2 个 provider，且配置/成色/区域/币种精确匹配；
- 定价策略为已批准且在生效期内；
- primary/secondary 身份不同、MFA 和授权快照有效；
- Signer capability 为 production，key 不可导出且证书状态正常；
- RFC 3161 token 可独立验证并绑定实际签名字节摘要；
- 报告存储私有，下载 URL 不超过 120 秒且响应 `no-store`；
- 报告明确标明数据来源、有效期和限制，不含测试水印或测试免责声明。

## 9. 监控和告警

必须监控：

- API health/readiness、错误率和延迟；
- 对象事件 backlog、oldest age、重试次数、死信和 polling 恢复数量；
- Scanner/OCR/ClamAV 可用性及病毒库更新时间；
- workflow queued/running/retry_wait/manual_intervention 数量和租约超时；
- evidence review 和 formal review SLA、无人认领和权限失败；
- 市场样本不足、provider 数不足、快照到期和策略即将失效；
- Benchmark 即将到期、缺少稳定性/LLM 分类；
- frozen 报告积压、issuance_failed、Signer/TSA capability age、证书到期；
- callback/outbox 重试与死信；
- 数据库容量、复制延迟、备份结果和对象存储复制失败；
- 撤销、异常下载、跨租户拒绝和权限变化审计。

告警必须指向明确值班人和操作手册。Signer/TSA/IAM 故障只允许阻止签发，不允许启用测试替代项。

## 10. 回滚方案

### 10.1 自动回滚触发

- 服务 60 秒内未恢复健康或连续健康检查失败；
- 鉴权绕过、跨租户访问、Hash/签名/TSA 校验异常；
- 生产报告出现 `test_only` 或任何测试来源；
- migration checksum、不可变约束或关键行数异常；
- 重复事件导致重复估值、重复审核或重复签发；
- 对象事件持续积压、DLQ 快速增长或证据状态错误推进；
- Signer/TSA/IAM capability 与发布前快照漂移；
- 数据损坏、OOM、磁盘/KMS/数据库故障达到批准阈值。

### 10.2 普通应用回滚

1. 停止新增正式评估入口或将流量切回上一版本。
2. 关闭 production workflow 和新的签发请求；已经运行的任务依赖持久化状态在恢复后重试。
3. 保留当前数据库和对象存储，不删除评估、事件、报告或审计记录。
4. 按发布清单恢复上一镜像 digest、配置版本和证书引用，禁止使用 `latest`。
5. 不执行数据库 down migration。当前迁移为加法迁移，旧二进制无法读取新增字段时应通过兼容测试提前发现。
6. 验证健康、鉴权、租户隔离、旧报告读取/下载和事件幂等后再恢复流量。
7. 保存失败版本日志、配置摘要、数据库快照和队列状态用于复盘。

签名或 TSA 故障时只回滚 Signer/TSA 服务或保持报告 `frozen`，绝对不能降级到 SoftHSM/本地时间戳。

### 10.3 已签发报告处理

- 应用回滚不修改或删除已签发报告；
- 如果报告内容真实且签名信任仍有效，报告保持 issued；
- 如果发现数据、审核、密钥或时间戳信任事故，使用实名 revoker、授权编号和事件编号执行撤销；
- 撤销不依赖 renderer、Signer 或 TSA 在线；
- 禁止通过删除对象代替正式撤销。

### 10.4 数据损坏恢复

数据恢复属于事故操作，不是普通发布回滚：

1. 停止所有相关 writer 和对象事件 consumer；
2. 对失败状态再做一份只读快照；
3. 记录恢复点并取得业务、数据库和安全负责人批准；
4. 先恢复到新数据库/隔离 bucket；
5. 对比 schema、关键行数、Hash、审计/outbox 连续性和报告验签；
6. 通过验证后切换连接；原库保持只读以供审计；
7. 不允许无批准的原地覆盖恢复。

## 11. 兼容策略

### 11.1 gpuf-c/gpuf-s

- 新 Benchmark 命令为协议 v3 的附加能力；旧客户端仍可连接和生成预评估。
- 旧客户端没有内置 Benchmark 时，不伪造或默认填充数值；正式 T1/T2 会因 policy 不满足而 `technical_rejected`。升级客户端或通过获批等价受控 Runner 取得真实 evidence 后重新评估。
- 新字段必须保持附加和可选；不兼容变更必须升级协议或 schema version。

### 11.2 GPUFabric 报告

- 兼容读取旧 canonical JSON Hash 信封；新报告统一使用原始 JSON 字节 Hash。
- `reportHtmlSha256` 和技术快照引用为兼容可选字段，但提供后必须完整校验。
- 预评估、Benchmark 和快照过期后不得复用到新的正式报告。

### 11.3 new-api/前端

- 浏览器只调用 new-api，不持有内部服务 Token、市场权限、reviewer 权限或签名凭据。
- 新响应字段保持 additive；旧前端可忽略未知字段，但不得把缺失字段展示为 0。
- `clientRequestId` 与 `Idempotency-Key` 必须唯一且一致；旧 assessment/evidence/report ID 和短时 URL 不得复用。
- 回调按 event ID 去重，未知附加字段可忽略，删除或改变语义必须升级事件版本。

### 11.4 数据库和对象事件

- 迁移保持向前兼容和加法；二进制回滚保留新增表/列。
- 正式对象事件为可靠主路径；旧同步完成接口和有界 polling 仅作兼容/恢复，且必须幂等。
- 旧报告不可变，不回填新 provenance、签名或 TSA 字段；新报告使用当前 schema。

## 12. 正式报告日常生成流程

1. 用户实名登录并选择自己拥有的真实资产。
2. 系统读取有效预评估和真实 BenchmarkEvidence；不满足直接拒绝。
3. 用户上传真实材料，服务端校验对象版本、长度、MIME、Hash 和病毒扫描。
4. 实名证据审核员查看原始材料并录入 lifecycleFacts；补件或拒绝必须有原因。
5. 系统选择已核验真实市场快照和已批准正式策略，自动计算估值。
6. 任务进入人工审核队列，由不同 primary/secondary reviewer 独立批准。
7. 服务端固化 provenance，重验全部门禁并冻结 JSON/HTML。
8. 实名 issuer 触发签发，服务端验证 production Signer/TSA capability。
9. HSM/KMS/PKCS-11 签名并取得 RFC 3161 token，服务端独立验签。
10. PDF 保存到生产私有对象存储，事务提交 issued、审计和 outbox。
11. 用户通过最长 120 秒的 no-store 授权下载；到期或撤销后拒绝新授权。

任何步骤失败均停留在对应状态等待修复，不使用 Mock 数据补偿。

## 13. 上线签字清单

| 项目 | 负责人 | 证据/版本 | 结果 |
|---|---|---|---|
| 真实资产和材料入驻 | 业务/运营 |  |  |
| 市场 provider、合同和原始样本 | 市场/法务 |  |  |
| 定价策略与独立审批 | 风控/评估机构 |  |  |
| 企业 IAM/MFA/RBAC/SCIM | IAM/安全 |  |  |
| Primary/Secondary 审核制度 | 审核组织 |  |  |
| HSM/KMS/PKCS-11 与正式证书 | 安全/CA |  |  |
| RFC 3161 TSA/OID/根链 | 安全/法务 |  |  |
| S3/OSS/KMS/事件/复制 | 云平台/SRE |  |  |
| 数据库和对象恢复演练 | DBA/SRE |  |  |
| 监控告警和值班手册 | SRE |  |  |
| 前后端兼容与回调幂等 | 开发/测试 |  |  |
| 首份真实报告独立验签 | 安全/业务 |  |  |
| 无 Mock 最终复核 | 发布负责人 |  |  |

所有项目签字通过前，`ASSESSMENT_ENABLE_FORMAL_REPORTS` 和 `ASSESSMENT_ENABLE_REPORT_LIFECYCLE` 必须保持关闭。正式签发后发现任何 Mock 或来源无法证明，应立即停止新签发、冻结相关流程、保全审计证据并按正式撤销政策处理。
