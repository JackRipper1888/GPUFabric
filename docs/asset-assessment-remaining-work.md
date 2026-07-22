# 算力资产评估剩余任务执行顺序

> 基线日期：2026-07-20
> 适用仓库：GPUFabric、new-api、asset-assessment-service
> 排序原则：先解决代码归档和可部署基线，再交付用户可见预评估，然后完成正式评估、真实证据、估值和签章。

## 1. 当前基线

### 已完成

- GPUFabric 已提供在线设备与离线 challenge/evidence 预评估、主体/租户/操作范围幂等、结构化缺失码/警告码/后续动作、冻结 HTML、不可变 v1/v2、原始证据保留期清理和签名 BenchmarkEvidence。
- asset-assessment-service 已实现创建/查询、GPUFabric v1 JSON/HTML/v2 客户端与三 Hash 校验、证据要求、上传会话、可信对象事件核验、扫描/审核工作队列、状态绑定的短时私有读取授权、扫描结果、人工证据复核、PostgreSQL、审计和 Outbox。
- asset-assessment-service 的本地 Go、PostgreSQL、HTTP、Docker 和服务身份负例测试已通过。
- AS-005 回调已按 `object-storage-gateway`、`evidence-scanner`、`assessment-reviewer` 独立 subject/scope 隔离。
- 原生 S3/OSS 事件入口只接收 `eventId + provider object key`，由服务端 HEAD 获取长度、SHA-256 和 MIME；事件收据与证据状态原子提交，同事件跨对象复用冲突。Scanner/Reviewer 队列不暴露对象键，GET 授权最长 120 秒且授权审计已持久化。
- asset-assessment-service 已实现原生 S3 SigV4 和阿里云 OSS V4 私有存储适配、正式报告 PDF/签发/撤销/过期/短时下载生命周期、Signing Service/X.509 验证边界、Token 轮换和 subject-bound mTLS。
- `assessment.benchmark-policy.v1` 已接入正式评估：T1 要求有效稳定性证据，T2 额外要求有效 LLM 性能证据；双证据成功、稳定性单证据 T1 成功/T2 拒绝、幂等和 PostgreSQL 持久化跨服务回归已通过。
- GPUFabric 已生成目录绑定的 `gpuf.asset_configuration.v1` 并自动关联同设备最新有效 BenchmarkEvidence；assessment-service 独立重算配置 Hash，估值强制匹配技术配置和经 `asset.lifecycle` 审核的成色。
- GPUFabric 与 new-api 已实现稳定离线资产引用：new-api 返回 64 位 `benchmarkSourceRef`，GPUFabric 用固定 `gpuf.offline_asset_source.v1` profile 生成同一 `sourceRef`；collector `payloadSha256` 仍只绑定单次 challenge。跨语言固定向量、普通回归和 z370 真实签名 Benchmark 自动关联验收均已通过。
- assessment-service 已提供 `market:verify` 隔离的待核验市场样本队列；真实供应商适配器仍需在许可和对象访问方案确定后接入。
- 2026-07-22 本地 Docker staging 已部署当前 GPUFabric 与 assessment-service；new-api live contract 已通过在线预评估、目录配置 Hash、自动 BenchmarkEvidence、T2 正式评估和 `asset.lifecycle` 材料要求。独立离线 live contract 已通过 SSH 在 z370 的 RTX 4070 SUPER 上运行带利用率/温度/功耗序列的真实 `hw-asset-collector`，经 new-api 服务层消费一次性 challenge 并生成不可变报告/快照；同一流程通过 SSH 隧道运行 Ollama workload，自动关联 LLM 与稳定性两项 Ed25519 BenchmarkEvidence。assessment-service 回调密钥边界已与 new-api 对齐为 32 至 4096 字节，配置负例、全量测试和重部署后 live contract 均已通过。

### 尚未完成或存在缺口

- asset-assessment-service 的内部远端已建立，生命周期基线提交 `17429ae` 与技术配置/成色/估值一致性提交 `7fd0899` 均已推送 `main`。
- new-api 已实现资产绑定、在线/离线预评估、正式评估、材料会话、签名回调、状态投影和下载 API；在线与 z370 离线服务层 live contract 已通过，剩余真实 user-service/数据库部署下的浏览器链路，以及真实材料、assessment-service Outbox 到 new-api HTTP 回调和 issued 下载联合回归。
- GPUFabric T0 预评估代码闭环、collector 短期运行采样、z370 真实离线节点和签名 Benchmark 自动关联验收均已通过；剩余 collector 的正式签名发布/下载渠道、生产服务身份、可靠事件和规格目录持续扩充。
- collector 已支持 `--runtime-history-file` 跨进程追加和加载最多 90 天的 JSONL 运行历史，并在报告中给出真实 `observation_days`；该历史仍属于自报告证据。至少 7 个自然日的授信级长期稳定性仍需要设备侧周期代理提交观测，服务端按稳定 `sourceRef` 去重聚合利用率、温度、功耗、在线率和异常计数。短期窗口保留 `SHORT_OBSERVATION_WINDOW`；完全没有 runtime_history 时才保留 `RUNTIME_HISTORY_MISSING`，这不是 T1/T2 当前签名 Benchmark 门禁的替代项。
- 私有存储代码支持 HMAC gateway、原生 S3 SigV4 和阿里云 OSS V4；可信事件核验和读取授权接口已完成。真实 bucket 禁止公共访问、RAM/IAM/RRSA/STS、KMS、保留期以及云事件源到桥接入口的部署验收仍未完成，腾讯云 COS 原生适配未实现。
- 报告生命周期和依赖失败语义已有服务端代码与本地测试；Scanner/Reviewer 的服务端接入边界已经完成，但真实隔离 Scanner/OCR、Reviewer Workbench、renderer、Signing Service/HSM、可信时间戳和生产证书尚未接入。市场数据样本/核验/不可变快照、版本化估值策略、策略审批职责分离和双人正式审核已有服务端代码闭环；真实供应商许可、生产样本治理和审核人员联合认证仍是正式金额前置门禁。

## 2. 严格执行顺序

| 顺序 | 优先级 | 工作包 | 责任仓库/系统 | 依赖 | 完成标准 |
|---|---|---|---|---|---|
| 1 | P0 阻断 | 建立 asset-assessment-service 代码基线 | asset-assessment-service | 无 | 已完成：`main` 已包含并推送生命周期与数据闭环提交 `17429ae`、`7fd0899`。 |
| 2 | P0 阻断 | 部署当前 v1/v2 技术底稿链路 | GPUFabric + assessment-service | 1 | 本地 Docker 三 Hash、篡改拒绝、租户幂等、在线/离线和签名基准已通过；剩余裸金属同步部署与发布回归。 |
| 3 | P0 | new-api 用户可见预评估最小闭环 | new-api `NA-001` 至 `NA-006` | 2 | 代码已完成，在线与 z370 真实离线服务层 live contract 已通过；剩余真实 user-service/数据库环境的浏览器 HTTP 验收。 |
| 4 | P0 | 技术引用接入 new-api | new-api `NA-007`（GPUFabric 侧已完成） | 2，可与 3 并行 | new-api 技术引用持久化和在线/离线 live contract 已通过；剩余 collector 正式发布和生产节点推广回归。 |
| 5 | P0 | 预评估报告内容发布验收 | GPUFabric `GF-009` 至 `GF-011` 已完成，`GF-014` 持续扩充 | 4 | 结构化码、冻结 HTML、兼容空金融字段已通过；发布前复验目标型号目录来源，报告始终不生成无来源金额。 |
| 6 | P0 | new-api 正式评估状态骨架 | new-api `NA-008` 至 `NA-011` | 2、3 | 代码及在线 T2 live contract 已完成；剩余真实回调接收、乱序重放和浏览器状态投影联合验收。 |
| 7 | P0 | 接入真实私有对象存储 | assessment-service + Storage Gateway | 1、6 | S3/OSS 上传、服务端 HEAD 核验、事务化事件收据和短时 GET 授权代码/本地测试已完成；剩余真实 bucket/KMS/最小权限和云事件源部署验收，COS 原生适配按部署区域另行决定。 |
| 8 | P0 | 接入 Scanner、证据审核端和上传代理 | Scanner + Reviewer + new-api `NA-012` | 7 | assessment-service 队列、状态授权、结果回调和审计边界及 new-api 上传会话代理已完成；剩余真实 Scanner/OCR、Reviewer Workbench 联调，完成真实文件 upload -> event -> scan -> review -> `ready_for_valuation`。 |
| 9 | P1 | 生产 BenchmarkEvidence 接入 | GPUFabric `GF-015` + assessment-service `AS-006` | 4、8 | GPUFabric 登记/Ed25519 验签、空 ID 自动关联、LLM/稳定性双证据 runner 和 assessment T1/T2 策略已通过本地回归；剩余生产 workload 验收、阈值异常复核和密钥签发/轮换/吊销。 |
| 10 | P1 阻断估值 | 市场数据授权与治理 | 业务/合规 + assessment-service `AS-007` | 数据供应商与许可 | 代码已支持授权样本写入和核验；还需明确真实供应商、许可范围、保留期、币种、税口径、去重键和撤回机制；未完成前禁止正式金额输出。 |
| 11 | P1 | 不可变市场快照聚合 | assessment-service `AS-008` | 10 | 代码已支持待核验队列、可复算配置 Hash、已核验 MarketObservation 聚合和最少 3 样本/2 来源；还需真实样本适配器、许可/撤回、异常值策略和生产回归。 |
| 12 | P1 | 版本化估值策略 | assessment-service `AS-009` | 8、9、11 | 代码已支持 PricingPolicy 草稿/独立主体审批和 ValuationResult 可复算，同输入同策略返回同一结果；生产仍需真实市场许可和样本治理。 |
| 13 | P1 | 正式审核后台 | assessment-service `AS-013` | 8、12 | 服务端已支持任务提交、主审/复审分配、顺序审批、职责分离和不可变动作审计，并拒绝人工金额覆盖；剩余真实 Workbench、人员身份联合认证和生产权限回归。 |
| 14 | P1 | 报告冻结与生命周期 | assessment-service `AS-010` + new-api `NA-013` | 12、13 | 服务端和 new-api `NA-013` 下载代理均已实现；仍需真实 renderer/存储/签名依赖和 issued 下载联合验收。 |
| 15 | P1 | Signing Service / HSM | assessment-service `AS-012` | 14 | 已实现冻结摘要 envelope、依赖客户端、X.509 链和 detached signature 验证及失败保持 frozen；剩余真实 HSM 私钥、机构证书、可信时间戳和轮换演练。 |
| 16 | P1 发布门禁 | 生产身份与可靠事件 | GPUFabric `GF-012/013` + new-api `NA-014` | 2 至 15 可并行推进 | assessment-service 已支持 Token 轮换和 subject-bound mTLS；剩余生产 OAuth2 issuer/audience、GPUFabric/new-api 身份、事件重试/死信、审计保留和依赖降级监控联合验收。 |

## 3. 可并行工作

以下任务可以并行，但不能绕过对应验收门禁：

- 顺序 3（new-api v1 用户链）与顺序 4（技术引用适配）可以并行；GPUFabric v1/v2/HTML 已可用，new-api 通过 `NA-007` 保存引用。
- 顺序 6（new-api 正式评估骨架）与顺序 7（对象存储）可以并行；两者在顺序 8 汇合。
- 顺序 9（生产 benchmark runner 与策略接入）可以与顺序 10/11（市场数据）并行；估值必须等待两者达到所选评估等级要求。
- 顺序 16 的服务身份、事件可靠性和监控应从 P0 阶段持续建设，但它是生产发布硬门禁，不得留到上线后补做。

## 4. 近期三个里程碑

### M0：可追踪、可部署基线

包含顺序 1 至 2。

验收结果：任意开发者可从仓库检出并部署；现有 GPUFabric JSON/HTML/v2 技术底稿能被 assessment-service 真实读取并完成三 Hash 验证。

### M1：用户可见技术预评估

包含顺序 3 至 5。

验收结果：在线设备和离线 collector 均可经 new-api 生成预评估；报告包含来源、质量、缺失项和风险提示，不包含无来源金融金额。

### M2：无市场估值的正式评估骨架

包含顺序 6 至 8。

验收结果：new-api 可创建正式评估、补齐真实私有证据并接收回调，任务最终到达 `ready_for_valuation`。

市场估值和签章属于 M3，只有 M0 至 M2 稳定后才能开始正式金额和签章报告验收。

## 5. 当前不应提前开始

- 在市场数据许可、样本治理、审核人员身份联合认证和正式报告冻结完成前，不向用户或授信流程输出参考估值、质押率、可贷额度或正式授信参考。
- 在报告冻结和正式审核完成前，不接入生产 HSM 或申请正式机构证书。
- 在真实对象存储、扫描器和审核端联调前，不宣称 AS-005 生产闭环完成。
- new-api 回调验签、幂等和状态映射代码已完成；在真实回调密钥、重试和乱序联合验收前，不把 assessment-service 状态直接用于生产业务结论。
