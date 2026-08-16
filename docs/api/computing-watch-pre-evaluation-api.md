# 算力资产预评估报告 API 文档

更新时间：2026-08-05
适用环境：测试环境
实现服务：`gpuf-api-server`（报告能力）与 `gpunexus-web`（会话鉴权及安全代理）

## 1. 接口关系

外部算力资产列表：

`GET /api/computing/watch/list`

该接口由当前 `gpunexus-web` BFF 提供，并将请求转发至 GPUFabric：

`GET /api/user/client_list`

报告存在性、报告元数据、预览地址和下载地址均由 `gpuf-api-server` 生成。BFF 校验登录会话后，将 API Server 的内部报告路径改写为同源、会话绑定的预览和下载路径。

> API Server 原始 `preview_url` 和 `download_url` 相对于 `gpuf-api-server` 基址。浏览器不得直接使用这些内部路径或持有 Banking Token，应使用 BFF 改写后的同源 URL。

## 2. 算力资产列表

### 2.1 外部接口

`GET /api/computing/watch/list`

#### Query 参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|---:|---|
| `client_id` | string | 否 | 精确过滤算力客户端 ID |
| `status` | string | 否 | 过滤客户端状态 |
| `name` | string | 否 | 按资产名称模糊匹配 |

BFF 从登录会话取得用户 ID，忽略浏览器提交的 `user_id`，并固定增加 `valid_status=valid`。未指定 `status` 时同时返回在线和离线有效设备；指定 `status=offline` 可只查询离线设备。

### 2.2 API Server 上游接口

`GET /api/user/client_list`

基址示例：`http://<gpuf-api-server>:18081`

#### Query 参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|---:|---|
| `user_id` | string | 是 | 用户 ID |
| `client_id` | string | 否 | 客户端 ID，必须可解析为 GPUFabric ClientId |
| `status` | string | 否 | 客户端状态 |
| `name` | string | 否 | 资产名称，SQL 使用 `ILIKE %name%` |
| `valid_status` | string | 否 | 资产有效状态，例如 `valid` |

#### 新增响应字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `data.devices[].has_pre_evaluation_report` | boolean | 是否存在已生成且同时保存 HTML 和摘要的预评估报告 |
| `data.devices[].pre_evaluation_report` | object | 最新报告元数据；无报告时不返回 |
| `data.devices[].pre_evaluation_report.report_id` | string | 报告 ID |
| `data.devices[].pre_evaluation_report.generated_at` | RFC3339 string | 报告生成时间 |
| `data.devices[].pre_evaluation_report.preview_url` | string | API Server HTML 预览路径 |
| `data.devices[].pre_evaluation_report.download_url` | string | API Server HTML 下载路径 |

#### 响应示例

```json
{
  "success": true,
  "data": {
    "total": 2,
    "devices": [
      {
        "client_id": "<client-id>",
        "has_pre_evaluation_report": true,
        "pre_evaluation_report": {
          "report_id": "PRE-2026-07-...",
          "generated_at": "2026-07-28T03:20:27.783964Z",
          "preview_url": "/api/banking/provider/pre-evaluations/PRE-2026-07-.../pdf",
          "download_url": "/api/banking/provider/pre-evaluations/PRE-2026-07-.../pdf?download=true"
        }
      },
      {
        "client_id": "<client-id-without-report>",
        "has_pre_evaluation_report": false
      }
    ]
  }
}
```

API Server 的两个设备列表接口 `/api/user/client_list` 与 `/api/user/client_status_list` 都返回以上字段。`/api/computing/watch/list` 当前使用前者。

同一设备生成多份报告时不会覆盖历史记录。列表只关联 `created_at` 最新的一份；时间相同时以 `report_id` 倒序确定结果。设备变为 `offline` 不会解除已有报告关联。

## 3. 报告预览与下载

### 3.1 浏览器接口（推荐）

```http
GET /api/computing/watch/pre-evaluation/{reportId}/preview
GET /api/computing/watch/pre-evaluation/{reportId}/download
```

浏览器只携带现有登录会话。BFF 执行以下检查：

1. 校验登录会话并从会话取得用户 ID。
2. 回查该用户的有效设备，确认 `reportId` 属于其设备当前关联报告。
3. 在服务端附加 Banking Token 请求 API Server。
4. 校验 `application/pdf`、`%PDF-` 文件头和响应 SHA-256。
5. 以 `inline` 或 `attachment` 返回同源 PDF，并设置 `private, no-store`。

Banking Token 只能配置在 BFF 服务端环境变量 `GPUF_BANKING_API_TOKEN` 中，不得写入前端构建变量、浏览器存储、URL 或日志。

### 3.2 API Server 内部接口

`GET /api/banking/provider/pre-evaluations/{reportId}/pdf`

#### Path 参数

| 参数 | 类型 | 必填 | 规则 |
|---|---|---:|---|
| `reportId` | string | 是 | 1 至 64 字节，只允许 ASCII 字母、数字、`-`、`_` |

#### Query 参数

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|---|---|---:|---|---|
| `download` | boolean | 否 | `false` | `false` 为预览；`true` 为附件下载 |

#### 请求头

| 请求头 | 必填 | 说明 |
|---|---:|---|
| `Authorization: Bearer <GPUF_BANKING_API_TOKEN>` | 是 | Banking 服务令牌；不得放入 URL 查询参数 |

#### 内部预览请求

```bash
curl -H "Authorization: Bearer <token>" \
  "http://<gpuf-api-server>:18081/api/banking/provider/pre-evaluations/<reportId>/pdf"
```

预览响应包含：

- `Content-Type: application/pdf`
- `Content-Disposition: inline; filename="pre-evaluation-<reportId>.pdf"`
- `X-Content-SHA256: <64-hex>`
- `Cache-Control: private, no-store`
- `X-Content-Type-Options: nosniff`

#### 内部下载请求

```bash
curl -OJ -H "Authorization: Bearer <token>" \
  "http://<gpuf-api-server>:18081/api/banking/provider/pre-evaluations/<reportId>/pdf?download=true"
```

下载返回经过 PDF 文件头和 SHA-256 校验的 PDF 字节，响应使用 `Content-Disposition: attachment`。

#### 状态码

| 状态码 | 场景 |
|---:|---|
| `200` | 报告存在且鉴权成功 |
| `400` | 报告 ID 或 `download` 参数格式错误 |
| `401` | 未提供令牌或令牌错误 |
| `404` | 报告不存在 |
| `500` | 数据库读取失败或冻结 HTML 的 SHA-256 校验失败 |
| `502` | PDF 支撑服务不可用或返回的 PDF/Hash 无效 |
| `503` | Banking 令牌或 PDF 支撑服务配置无效 |

## 4. 涉及的数据表

### 4.1 列表读取主表

| 表 | 用途 | 关键字段 |
|---|---|---|
| `gpu_assets` | 用户资产、客户端状态和有效状态 | `user_id`, `client_id`, `client_name`, `client_status`, `valid_status`, `created_at`, `updated_at` |
| `device_info` | 设备名称等设备信息 | `client_id`, `device_index`, `device_name` |
| `system_info` | 最新 CPU、内存、磁盘和 TFLOPS 指标 | `client_id`, `cpu_usage`, `mem_usage`, `disk_usage`, `total_tflops`, `created_at` |
| `client_daily_stats` | 健康率和运行天数 | `client_id`, `total_heartbeats` |

### 4.2 报告读取主表

| 表 | 用途 | 关键字段 |
|---|---|---|
| `pre_evaluation_reports` | 报告归属、状态、冻结 JSON/HTML 及哈希 | `user_id`, `source_type`, `source_id`, `report_id`, `report_status`, `report_html`, `report_html_sha256`, `created_at` |

报告关联规则：

1. 资产 `client_id` 计算稳定的 SHA-256 `source_id`。
2. 仅匹配同一 `user_id`、`source_type='gpuf_online'`、`report_status='generated'` 的报告。
3. `report_html` 和 `report_html_sha256` 必须同时存在。
4. 每个资产只返回 `created_at` 最新的报告。

设备状态不参与报告关联，因此在线生成的报告在设备离线后仍可预览和下载。离线采集报告使用不同的 `source_type/source_id`，除非业务层显式把它绑定到该 GPUFabric 设备，否则不会自动显示在在线资产列表中。

### 4.3 非数据库数据

`loaded_models` 通过 Redis 批量读取，不来自 PostgreSQL 表。此次修改没有新增或修改表结构，但更新了 `gpu_model_specs` 的规格 seed。

## 5. 安全与部署说明

- 报告 HTML 接口要求 Banking Bearer Token。
- 浏览器通过 BFF 的登录会话代理访问，Banking Token 不下发浏览器。
- BFF 必须按当前会话用户回查报告归属，不能只凭 `reportId` 代理。
- 读取报告前会重新计算冻结 HTML 的 SHA-256，不一致时拒绝返回。
- 预览与下载均禁止共享缓存。
- 列表新增字段为向后兼容的附加字段。
- 后端字段和 HTML 能力只需部署 `gpuf-api-server`。面向浏览器启用安全预览/下载时，还需部署包含会话代理的 `gpunexus-web`，并配置其到 API Server 的网络地址和 Banking Token。两个服务只需网络互通，不要求部署在同一台服务器。

## 6. 报告生成前置条件与缺项

在线设备生成技术预评估报告至少需要：

| 前置数据 | 缺失影响 |
|---|---|
| `gpu_assets` 中存在同一 `user_id/client_id` 的有效资产 | 无法按用户创建或关联报告 |
| `device_info` 与最新 `system_info` | 报告出现硬件、显存、性能或运行信息缺项 |
| GPU 型号命中 `gpu_model_specs` | 技术快照缺少 `assetConfiguration`，正式估值门禁会拒绝继续 |
| T2 所需且未过期的签名 benchmark 证据 | 不能满足 T2 benchmark 门禁；不得把测试造数描述为真实性能 |

测试设备 `e5dd57907588424abb886eff4bcfd378` 上报的 `Apple Apple M1 Pro` 已通过别名映射到 `apple-m1-pro-gpu`。该规格 seed 同时写入开发 schema 和生产增量 schema；其他未收录型号仍需先审核并补充规格目录。

## 7. HTML 数据是否需要拆分存储

### 7.1 当前存储方式

当前冻结 HTML 存储在 PostgreSQL：

`pre_evaluation_reports.report_html`

完整性摘要存储在：

`pre_evaluation_reports.report_html_sha256`

API Server 从数据库读取 HTML 后重新计算 SHA-256，只有与数据库摘要一致时才返回。

### 7.2 当前阶段建议

**当前阶段不建议立即拆分。**

原因：

- HTML 与报告状态、用户归属和 SHA-256 保存在同一条报告记录中，事务一致性简单。
- 冻结 HTML 是不可变快照，当前体量和访问量较小时 PostgreSQL 的 TOAST 能够正常承载。
- 立即拆分会新增对象存储可用性、跨存储事务、回收和故障恢复复杂度。
- 现有 API 已经封装了读取逻辑，后续更换底层存储不需要改变客户端契约。

当前应保留 API 抽象：调用方只能通过 `gpuf-api-server` 读取报告，不应直接读取数据库或依赖具体存储路径。

### 7.3 建议启动拆分评估的条件

出现以下任一情况时，建议把 HTML 字节迁移到私有对象存储：

- `pre_evaluation_reports` 及其 TOAST 数据明显影响数据库备份、恢复、VACUUM 或复制。
- 冻结 HTML 总量达到约 10 GB，或报告量接近 10 万份并持续增长。
- 单份 HTML 平均超过约 512 KiB，或者开始内嵌较大的图片和其他资源。
- 预览、下载并发明显增加，需要独立扩展带宽或使用 CDN。
- 需要独立的归档、生命周期、跨区域复制或不可变保留策略。
- 后续需要同时存储 HTML、PDF、附件等多种大对象。

这些数值是启动容量评估的参考线，不是硬性数据库限制；最终应结合数据库增长速度、备份窗口和下载流量决定。

### 7.4 推荐的拆分目标

HTML 放入私有 OSS、S3 或 MinIO bucket，PostgreSQL 只保留报告元数据和对象引用。

建议保留或新增以下元数据：

| 字段 | 用途 |
|---|---|
| `report_html_sha256` | HTML 字节完整性摘要，继续保留 |
| `report_html_object_key` | 私有对象存储 key，不保存公开 URL |
| `report_html_size_bytes` | 内容大小，用于限流和完整性检查 |
| `report_html_content_type` | 固定为 `text/html; charset=utf-8` |
| `report_html_storage_version` | 存储格式或迁移版本 |
| `report_html_stored_at` | 对象成功持久化时间 |

对象 bucket 必须为私有。不要把永久 OSS URL 或长期签名 URL 保存到列表响应或数据库。

源 HTML 存储可以迁移而不改变浏览器 PDF 契约：

```text
GET /api/banking/provider/pre-evaluations/{reportId}/pdf
GET /api/banking/provider/pre-evaluations/{reportId}/pdf?download=true
```

API Server 负责鉴权、读取源 HTML、验证 SHA-256、调用 Chromium 渲染并校验 PDF。客户端不需要知道源 HTML 位于 PostgreSQL 还是对象存储。

### 7.5 推荐迁移步骤

1. 增加对象 key、大小和存储版本等可空元数据字段。
2. 新报告双写：先写临时对象，校验 SHA-256 后转为正式对象，再提交数据库报告记录。
3. 读取时优先读取对象存储；对象引用为空时回退到现有 `report_html`。
4. 后台批量回填历史 HTML，每份都校验 `report_html_sha256`。
5. 观察一段时间，核对对象缺失率、哈希失败率、读取延迟和回退次数。
6. 停止新报告写入 `report_html`，但继续保留历史回退能力。
7. 完成备份和抽样恢复验证后，再分批清理数据库中的历史 HTML 字节。

不要在同一事务中直接依赖“数据库写成功且对象上传也一定成功”。推荐先完成可校验的对象写入，再提交数据库指针；失败对象使用临时前缀和生命周期规则自动回收。

### 7.6 最终结论

- **现在：** HTML 继续保存在 PostgreSQL，暂不增加独立存储服务。
- **以后：** 当容量、备份或并发触发上述条件时，迁移到私有对象存储。
- **接口：** 无论底层是否拆分，列表字段以及预览、下载 API 均保持不变。
