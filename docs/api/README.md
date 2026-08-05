# 算力资产评估 API 文档索引

> 状态：联调基线
> 版本：v1.2
> 日期：2026-07-21

API 按“谁提供接口”拆分，不再把浏览器接口、评估执行接口和技术数据接口混在一份文档中。

v1.2 增加目录绑定的 `AssetConfiguration` 固定 Hash、空 Benchmark ID 自动关联、生命周期成色审核事实、市场核验队列和估值三方一致性门禁。流程文件见 [时序图 MMD](asset-assessment-sequence.mmd)、[状态图 MMD](asset-assessment-state.mmd) 和 [状态图 SVG](asset-assessment-state.svg)。算力资产列表中的预评估字段、预览和下载契约见 [算力资产预评估报告 API](computing-watch-pre-evaluation-api.md) 与 [调用流程 SVG](computing-watch-pre-evaluation-flow.svg)。

| 接口提供方 | 文档 | 主要调用方 | 网络边界 |
|---|---|---|---|
| asset-assessment-service | [asset-assessment-service API](asset-assessment-service-api.md) | new-api、评估内部工作流 | 私有服务网络 |
| GPUFabric api_server | [GPUFabric 评估数据 API](gpufabric-assessment-api.md) | asset-assessment-service、new-api | 私有服务网络 |
| new-api | [new-api 资产评估 API](new-api-asset-assessment-api.md)、[OpenAPI 3.0](new-api-asset-assessment.openapi.yaml) | 浏览器、asset-assessment-service 回调 | 公共业务入口及受保护回调入口 |

## 归属原则

1. 路径以 `/api/...` 开头且面向浏览器的业务接口属于 new-api。
2. 路径以 `/internal/v1/asset-assessments/...` 开头的接口属于 asset-assessment-service。
3. 路径以 `/internal/v1/technical-pre-evaluations/...` 或 `/internal/v2/technical-snapshots/...` 开头的接口属于 GPUFabric。
4. `POST /api/banking/callback/assessment` 由 new-api 提供，但调用方是 asset-assessment-service。
5. 报告上传地址和下载地址属于私有对象存储，不是三个服务的永久业务 API。

## 调用关系

```text
Browser
   │ new-api public API
   ▼
new-api
   │ asset-assessment-service internal API
   ▼
asset-assessment-service
   │ GPUFabric assessment data API
   ▼
GPUFabric api_server

asset-assessment-service
   │ signed callback
   ▼
new-api
```

完整的在线/离线预评估、正式评估材料闭环、签名回调和报告下载时序图见 [算力资产评估全链路交互流程](new-api-asset-assessment-api.md#11-全链路交互流程图)。

总体业务、安全、数据和状态机设计仍以
[asset-assessment-service 对接与开发规范](../asset-assessment-service-integration.md)
为准。
