# 资产预评估 Beta 生产上线计划

文档状态：上线评审草案
更新时间：2026-08-16
适用版本：在机构 HSM/KMS、正式 RFC 3161 TSA、生产 S3/OSS/KMS/Scanner 和实名审核尚未全部入驻前的最小生产版本

## 1. 上线结论

Beta 分为两个明确隔离的运行档位：

1. **预评估 Beta**：只使用真实设备采集、Benchmark 和规格目录，不启用正式评估链路。
2. **全链路 Beta（当前目标）**：为了在生产部署形态中完成材料、估值、双审、冻结、PDF 和签发联调，允许使用固定 Beta reviewer、SoftHSM、非可信本地 TSA、隔离 Beta MinIO 和明确标记的合成市场。

全链路 Beta 使用的是测试替代项，不等于生产可信正式报告。所有产物必须强制 `testOnly=true`、`productionEligible=false`，事实来源为 `test_only`，PDF 和界面显示“Beta 测试签发/非正式评估”。即使数据库生命周期状态为 `issued`，也只代表 Beta 技术链路已完成，不代表机构正式签发。

不得把全链路 Beta 报告改名、去水印、改 trust level 或复制记录后作为《正式资产评估报告》。

## 2. 当前依赖缺口及影响

| 依赖 | 当前情况 | Beta 决策 |
|---|---|---|
| 企业身份、MFA、RBAC | 测试环境只有固定 reviewer 自动审批 | 全链路 Beta 使用固定且互异的 `beta-test-*` reviewer；强制 test-only；后续独立分支替换 |
| 机构 HSM/KMS + RFC 3161 TSA | 暂不能提供 | 全链路 Beta 使用独立 SoftHSM key/cert 和非可信本地时间戳；不得标为 production |
| 生产 S3/OSS、KMS、Scanner | 暂不能提供 | 全链路 Beta 使用独立 MinIO bucket/凭据和本地测试 Scanner/ClamAV/OCR；仅放测试或获批准的非敏感材料，不宣称生产托管/扫描 |
| 真实市场源 | 暂不能提供完整生产成交源 | 全链路 Beta 使用明确标记 synthetic/test-only 的市场和策略，不形成真实估值结论 |
| 真实设备采集 | gpuf-c/gpuf-s 已具备 | 可进入 Beta，要求真实 client/asset/tenant 绑定 |
| Benchmark/健康观测 | 新版 gpuf-c 已具备 | 可进入 Beta；缺失能力必须显示“不支持/未采集”，不能填 0 |
| 规格目录 | 已有服务端目录基础 | 可进入 Beta，必须保留来源和目录版本；未知型号不得虚构 |

## 3. 环境和数据隔离

1. 当前 `e5dd57907588424abb886eff4bcfd378` 保持连接 `ssh test`，只作为测试环境 Mac 验证设备。
2. 生产 Beta 必须重新执行生产租户、资产和设备绑定；推荐签发新的生产 client ID。不得复制测试数据库中的 client/asset/assessment/report 记录。
3. 全链路 Beta 可以部署同类测试组件，但必须重新创建 Beta 专用 bucket、凭据、SoftHSM key/cert、reviewer 名称、市场快照和策略；不得直接复用 `ssh test` 的 secret、对象或数据库记录。
4. 生产和测试使用不同数据库、凭据、证书、域名、端口/网络、对象前缀和监控标签。
5. 生产日志不得记录 access token、数据库口令、MFA secret、对象签名 URL 或私钥材料。

## 4. Beta 功能范围

### 4.1 允许

- 生产注册/白名单内的真实 gpuf-c 连接；
- 系统硬件、Metal/GPU、健康观测和采样覆盖率上报；
- 客户端内置受控 Benchmark 的真实执行与上报；
- 服务端规格目录匹配和来源展示；
- 预评估 JSON/HTML/PDF 生成、查询和按现有预评估策略下载；
- 使用固定 Beta reviewer、合成市场、隔离 MinIO、本地测试 Scanner、SoftHSM 和本地 TSA 跑通正式结构报告的全链路；
- 全链路 Beta 报告在内部完成冻结、测试签发、下载和撤销演练；
- 缺失、不支持、过期和低可信数据的明确展示；
- 只读运营监控、设备在线状态、错误和兼容性统计。

### 4.2 禁止

- 隐藏固定 reviewer、合成市场、SoftHSM、本地 TSA 或 MinIO 的测试属性；
- 将 Beta 技术 `issued` 状态展示为机构正式签发；
- 把真实敏感正式材料放入尚无 KMS/Scanner 验收的 Beta MinIO；
- 复用测试环境 secret、key、bucket、对象、assessment 或 report 记录；
- 人工填写 Benchmark、GPU 观测、估值金额、报告 Hash、trust level、签名或 TSA token；
- 将测试报告、测试材料或测试数据库记录迁移为生产记录。

## 5. 强制配置基线

预评估 Beta 保持四个开关关闭。全链路 Beta 的受限白名单环境使用：

```dotenv
ASSESSMENT_ENABLE_TEST_AUTOMATION=true
ASSESSMENT_ENABLE_PRODUCTION_WORKFLOW=false
ASSESSMENT_ENABLE_FORMAL_REPORTS=true
ASSESSMENT_ENABLE_REPORT_LIFECYCLE=true
ASSESSMENT_EVIDENCE_STORAGE_PROVIDER=s3
ASSESSMENT_REPORT_STORAGE_PROVIDER=s3
REPORT_SUPPORT_SIGNER_MODE=softhsm-test
```

其中两个 S3 endpoint 只能指向隔离的 Beta MinIO；SoftHSM key/cert 和本地 timestamp 必须在 capability、报告元数据和水印中保持 non-production。测试自动化与 production workflow 不得同时开启。

全链路 Beta 必须限制单个 tenant/asset/client allowlist。服务端、new-api 和前端不得移除 `testOnly`、`productionEligible=false` 或测试签发说明。

测试自动化的 asset、market snapshot、pricing policy、evidence reviewer、primary reviewer、secondary reviewer 和 lifecycleFacts 必须使用新建的 `beta-test-*` 标识，primary/secondary 必须不同。

gpuf-s/GPUFabric 使用真实设备、健康观测和 Benchmark；不得复用 `ssh test` 现有 Benchmark key。应创建 Beta 专用 key，并在报告来源中明确标为 test-only，不得获得 production trust。

## 6. 上线阶段

### 阶段 A：发布冻结和本地制品

1. 从干净提交本地构建 gpuf-s/gpuf-c/API/必要服务，禁止在生产服务器临时编译。
2. 执行单元、协议兼容、Mac Metal、Linux GPU、数据库迁移和预评估回归测试。
3. 固定 commit、二进制 SHA-256、镜像 digest、迁移 SHA-256 和 SBOM。
4. 生成测试替代项清单，确认每个 fixed reviewer、synthetic market、Beta MinIO、SoftHSM、本地 TSA 和 Beta key 都显式标记 test-only，且没有来源不明或未批准的数据。
5. 生成发布清单，明确前一版本 digest、负责人、维护窗口和回滚命令。

### 阶段 B：备份和暗部署

1. 备份生产数据库 globals、schema 和 custom-format dump，并在另一主机验证 checksum 与 `pg_restore --list`。
2. 保存当前服务/容器 inspect、环境变量名称清单、证书版本和前一二进制 Hash，不在清单中保存 secret 值。
3. 只执行加法迁移；禁止把 down migration 作为普通应用回滚。
4. 暗部署候选版本时先关闭所有 workflow/formal/lifecycle 开关，仅检查 health/readiness、鉴权和只读接口；验证后才按全链路 Beta 配置启用测试自动化和生命周期。
5. 验证旧客户端仍可连接；没有新 Benchmark 模块的客户端显示能力缺失，不阻塞普通预评估。

### 阶段 C：白名单 Canary

1. 在生产重新入驻一台获授权的内部真实设备，不复用测试绑定记录。
2. 先只启用设备注册和心跳，确认 client/tenant/asset 对应关系、TLS 和时间同步。
3. 启用健康采集，再启用 Benchmark，验证 challenge、参数 Hash、testedAt/expiresAt 和证据来源。
4. 生成首份“全链路 Beta 评估报告”，逐项确认材料、合成市场、固定 reviewer、SoftHSM 和本地 TSA 均显示 test-only，且不存在乱码、缺失值变 0 或无来源规格。
5. 验证技术链路可以冻结和测试签发，但报告明确标识 Beta/非正式，`productionEligible=false`。
6. 观察至少 24 小时，覆盖重连、采样窗口、Benchmark 周期、报告重建和旧客户端兼容后再扩大白名单。

### 阶段 D：有限放量

1. 按 tenant 和 client allowlist 小批量放量，不一次性开放全量注册。
2. 每批检查在线率、心跳延迟、采样缺口、Benchmark 失败率、报告生成错误率、数据库增长和回调积压。
3. 生产可信 workflow 和 production eligibility 持续关闭；全链路 Beta 仅在白名单内运行，产品、客服和合同不得把它描述为正式评估。
4. 只有实名审核、生产存储/Scanner、机构 Signer/TSA 全部验收后，才进入正式评估上线手册的后续阶段。

## 7. Go/No-Go 清单

| 检查项 | Go 条件 | 当前结果 |
|---|---|---|
| 生产/测试隔离 | 独立生产绑定、数据库、凭据和证书 | 待准备 |
| 本地构建制品 | 干净 commit、测试通过、Hash/digest 已记录 | 待准备 |
| 数据库备份 | 异机保存、checksum 和恢复清单验证通过 | 待准备 |
| 测试自动化 | 全链路 Beta 为 `true`，仅限白名单；production workflow 为 `false` | 待配置 |
| 正式结构链路 | formal/lifecycle 开启，但所有产物强制 test-only | 待配置 |
| 测试替代组件 | 独立 Beta reviewer/MinIO/Scanner/SoftHSM/TSA/synthetic market，禁止复用测试 secret | 待入驻 |
| 真实 Canary | 获授权真实内部设备和真实生产绑定 | 待入驻 |
| 测试来源披露 | 所有测试替代项均进入 provenance、capability、水印和审计 | 待验证 |
| Beta 标识 | testOnly、productionEligible=false、非正式和测试签发说明清楚 | 待验收 |
| 监控与值班 | 告警负责人、阈值和处理手册明确 | 待签字 |
| 回滚 | 前一 digest、回滚命令和恢复检查已演练 | 待演练 |

任一项未通过时不得放量；可以继续暗部署和内部验证，但不能开放外部用户。

## 8. 监控和停止条件

必须监控设备在线/心跳、TLS 注册失败、协议版本、健康采样覆盖、Benchmark challenge/过期、预评估生成失败、乱码/字体、规格匹配、数据库容量、回调积压、鉴权失败和跨租户拒绝。

以下任一情况立即停止新增流量并回滚应用：

- 任一测试替代项未标记 `test_only`，或报告出现 `productionEligible=true`；
- 跨租户读取、鉴权绕过、Hash 不一致或报告绑定错误；
- 缺失/不支持字段被展示为真实的 0；
- 新版本健康检查持续失败、心跳大面积中断或旧客户端无法连接；
- Beta 技术 `issued` 状态被对外解释为正式签发或机构签名；
- 数据库迁移异常、重复报告或不可解释的数据增长。

## 9. 回滚

1. 关闭新增 Beta 入口并收紧 client/tenant allowlist。
2. 停止候选 writer/worker，不删除已采集记录和审计。
3. 恢复上一版本固定二进制/镜像 digest 和配置版本，禁止使用 `latest`。
4. 保留加法数据库迁移，不执行破坏性 down migration。
5. 验证健康、注册、心跳、旧客户端、租户隔离和旧预评估读取后再恢复流量。
6. 对失败版本保存日志、配置摘要、数据库快照和报告样本用于复盘。
7. 若发生数据损坏，先恢复到新数据库验证，不得无批准原地覆盖生产库。

Beta 可以存在技术链路的 `issued` 状态，但它始终是 test-only。回滚不得通过删除报告掩盖问题；错误产物应标记不可用并保留审计证据。

## 10. 后续阶段和独立分支

实名人工审核在 `feature/production-human-review-identity` 独立分支实现，范围记录在 `asset-assessment-service/docs/production-human-review-identity-plan-cn.md`。它完成后仍不能绕过缺失的生产对象存储/Scanner 和 HSM/TSA。

后续门槛：

1. **Beta-A**：真实设备采集、Benchmark、健康观测、规格目录和预评估。
2. **全链路 Beta（当前目标）**：固定 reviewer、合成市场、隔离 MinIO、SoftHSM 和本地 TSA；允许技术测试签发，但强制 test-only。
3. **Beta-B（依赖未满足）**：identity service + 生产存储/Scanner + 真实市场/策略 + 人工双审，只到可信冻结。
4. **正式版（依赖未满足）**：机构 HSM/KMS、正式 RFC 3161 TSA、生产报告存储和完整签发治理全部验收后，才允许生产可信 `issued`。

## 11. 上线责任表

| 事项 | 负责人 | 证据/版本 | 签字 |
|---|---|---|---|
| 产品 Beta 文案与法律边界 | 产品/法务 |  |  |
| 真实生产设备入驻 | 运营 |  |  |
| 本地构建、测试和制品清单 | 开发/测试 |  |  |
| 数据库备份恢复演练 | DBA/SRE |  |  |
| Beta 测试来源与正式边界复核 | 发布负责人/安全 |  |  |
| TLS、凭据和 tenant 隔离 | 安全/SRE |  |  |
| Canary 验收与 24 小时观察 | 业务/测试/SRE |  |  |
| 回滚演练 | SRE/开发 |  |  |
