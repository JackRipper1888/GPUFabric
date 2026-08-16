# gpuf-s API Server 中文文档

本文档是 `api_server.md` 的中文对应说明，重点覆盖前端银行后台
`/api/banking/admin/*` 接口。完整通用接口仍以英文主文档为准。

## 基础信息

- 默认地址：`http://127.0.0.1:18081`
- 受保护部署可使用：`http://<host>:18081`
- 请求和响应内容类型：`application/json`

银行后台接口统一返回：

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

字段说明：

| 字段 | 类型 | 说明 |
|---|---|---|
| code | number | `0` 表示成功 |
| message | string | 成功时为 `ok` |
| data | object | 具体接口数据 |

## 推理网关说明

本文档中的管理 API 通常监听 gpuf-s API Server 端口，例如 `18081`。
OpenAI 兼容推理请求使用 gpuf-s 的独立推理网关端口，例如：

```text
http://<gpuf-s-host>:<inference_gateway_port>/v1/chat/completions
```

`POST /v1/chat/completions` 的 `messages[].content` 支持两种格式：

| 格式 | 说明 |
|---|---|
| string | 纯文本对话，保持旧 worker 协议兼容 |
| array | OpenAI 风格多模态内容数组，会走新版 chat task 协议 |

PaddleOCR-VL OCR 多模态请求示例：

```json
{
  "model": "PaddleOCR-VL-1.6-GGUF",
  "messages": [
    {
      "role": "user",
      "content": [
        {"type": "text", "text": "请识别图片中的文字。"},
        {
          "type": "image_url",
          "image_url": {
            "url": "data:image/png;base64,<base64-image>"
          }
        }
      ]
    }
  ],
  "max_tokens": 256,
  "temperature": 0.0,
  "stream": false
}
```

多模态数组说明：

- 支持的 part 类型：`text`、`image_url`、`image`、`media_marker`。
- 图片 URL 支持 `data:`、`http://`、`https://`。
- `file://` 默认禁用；只有服务该请求的 `gpuf-c` 进程设置
  `GPUF_ALLOW_FILE_IMAGE_URLS=1` 时才允许，建议只用于可信本地测试。
- OCR/视觉模型需要 `gpuf-c` 启动时传入匹配的 `--llama-mmproj-path`。
- 多模态数组请求需要更新后的非 Android `gpuf-c` worker；`cuda`、
  `vulkan`、`metal`、`cpu` 构建都包含 multimodal 支持。

完整 OpenAI 兼容推理接口见 `docs/gpuf-openai-compatible-api.md`。

## 数据来源和口径

当前银行后台看板主要读取以下表：

| 表 | 用途 |
|---|---|
| gpu_assets | 节点、状态、地区、用户、更新时间 |
| system_info | 算力、CPU/内存/磁盘使用率 |
| device_info | GPU 型号、GPU 数量、GPU 使用率 |
| inference_token_usage | Token 吞吐和 Token 总量 |

基础可见节点口径：

```sql
COALESCE(gpu_assets.valid_status, 'valid') IN ('valid', 'warning')
```

也就是说，看板接口只统计 API 可见的 `valid/warning` 节点。

## 节点状态分类

接口里有三个库存分类：

| 分类 | 含义 |
|---|---|
| all | 全部 API 可见节点 |
| online | 归一化状态为 `active` 或 `online` 的节点 |
| offline | API 可见节点中非 `active/online` 的节点 |

离线分类包含：

```text
offline
warning
maintenance
error
```

固定同一地区和同一时刻下：

```text
online + offline = all
```

这个等式适用于节点数量和算力。

## GET `/api/banking/admin/overview`

获取后台首页概览卡片、资源使用、集群构成，以及在线/离线/全部三分类数据。

### 查询参数

| 参数 | 类型 | 是否可选 | 说明 |
|---|---:|:---:|---|
| region | string | 是 | 按城市、区域、省份或算力区过滤 |
| from | ISO8601 string | 是 | Token 总量统计开始时间 |
| to | ISO8601 string | 是 | Token 总量统计结束时间，必须晚于 `from` |

注意：

- `overview` 不使用 `nodeStatus` 切换结果集。
- `overview` 一次返回 `all/online/offline` 三套库存维度数据。
- `from/to` 只影响 Token 总量卡片；节点数、算力、资源使用、集群构成仍是当前库存状态。

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| summaryCards | SummaryCard[] | 前端卡片数组，包含节点数、算力、Token 卡片 |
| resourceUsage | ResourceUsage | 全部节点口径的资源使用 |
| clusterStack | ClusterStackItem[] | 全部节点口径的集群构成 |
| statusBreakdown | OverviewStatusBreakdown | `all/online/offline` 三分类结构化数据 |

### SummaryCard

| 字段 | 类型 | 说明 |
|---|---|---|
| key | string | 卡片唯一 key |
| label | string | 前端展示名称 |
| value | number | 原始值；算力为 TFLOPS |
| displayValue | string | 展示值；算力换算为 PF 展示 |
| unit | string | 单位 |
| caption | string | 可选说明 |

库存相关 `summaryCards` key：

| key | label | value 口径 | 单位 | 说明 |
|---|---|---:|---|---|
| onlineNodes | 在线节点 | 节点数 | 个 | 在线节点数量 |
| offlineNodes | 离线节点 | 节点数 | 个 | 离线/非在线节点数量 |
| allNodes | 全部节点 | 节点数 | 个 | 全部 API 可见节点数量 |
| totalCompute | 总算力 | TFLOPS | PF | 兼容旧前端，等于 `allCompute` |
| onlineCompute | 在线算力 | TFLOPS | PF | 在线节点算力 |
| offlineCompute | 离线算力 | TFLOPS | PF | 离线/非在线节点算力 |
| allCompute | 全部算力 | TFLOPS | PF | 全部 API 可见节点算力 |

Token 相关 `summaryCards` key：

| key | 说明 |
|---|---|
| realtimeTokenThroughput | 最近 10 秒窗口的 Token/s |
| todayTokenTotal | 今日 Token 总量，或 `from/to` 指定时间段 Token 总量 |

Token 数据不按 `online/offline/all` 拆分。原因是 Token 是历史调用时间序列，
而节点在线/离线是当前库存状态；直接混用会造成口径歧义。

### ResourceUsage

| 字段 | 类型 | 说明 |
|---|---|---|
| totalDevices | number | 当前分类下的节点数 |
| usedDevices | number | 当前分类下的使用中节点估算值 |
| usageRate | number | `usedDevices / totalDevices`，四舍五入百分比 |

`usedDevices` 是估算值：在线节点中，存在模型元数据或当前遥测负载大于等于
5% 的节点会被认为在使用中。

### ClusterStackItem

| 字段 | 类型 | 说明 |
|---|---|---|
| key | string | GPU 分组 key |
| label | string | 展示名称 |
| percent | number | 当前分类下的占比 |

当前分组：

| key | label |
|---|---|
| a100_h100 | GPU A100/H100 |
| a800_4090 | GPU A800/4090 |
| cpu_edge | CPU/边缘节点 |

### statusBreakdown

`statusBreakdown` 是前端推荐使用的新结构，避免从 `summaryCards` 中手动拼装三分类。

```json
{
  "statusBreakdown": {
    "all": {},
    "online": {},
    "offline": {}
  }
}
```

每个分类的结构相同：

| 字段 | 类型 | 说明 |
|---|---|---|
| nodes | number | 当前分类节点数 |
| compute | number | 当前分类原始算力，单位 TFLOPS |
| computeDisplayValue | string | 展示用算力值，单位 PF |
| computeUnit | string | 固定为 `PF` |
| resourceUsage | ResourceUsage | 当前分类资源使用 |
| clusterStack | ClusterStackItem[] | 当前分类集群构成 |

### overview 示例

下面是结构示例，数值会随线上节点状态和 Token 调用变化。

```bash
curl "http://<host>:18081/api/banking/admin/overview"
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
        "value": 3,
        "displayValue": "3",
        "unit": "个"
      },
      {
        "key": "totalCompute",
        "label": "总算力",
        "value": 100,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "offlineNodes",
        "label": "离线节点",
        "value": 7,
        "displayValue": "7",
        "unit": "个"
      },
      {
        "key": "allNodes",
        "label": "全部节点",
        "value": 10,
        "displayValue": "10",
        "unit": "个"
      },
      {
        "key": "onlineCompute",
        "label": "在线算力",
        "value": 40,
        "displayValue": "0",
        "unit": "PF"
      },
      {
        "key": "offlineCompute",
        "label": "离线算力",
        "value": 60,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "allCompute",
        "label": "全部算力",
        "value": 100,
        "displayValue": "0.1",
        "unit": "PF"
      },
      {
        "key": "realtimeTokenThroughput",
        "label": "实时Token吞吐",
        "value": 3.1,
        "displayValue": "3.1",
        "unit": "/s",
        "caption": "当前TPS"
      },
      {
        "key": "todayTokenTotal",
        "label": "今日Token总量",
        "value": 62,
        "displayValue": "62",
        "unit": "tokens",
        "caption": "今日已调用"
      }
    ],
    "resourceUsage": {
      "totalDevices": 10,
      "usedDevices": 2,
      "usageRate": 20
    },
    "clusterStack": [
      {
        "key": "a100_h100",
        "label": "GPU A100/H100",
        "percent": 30
      },
      {
        "key": "a800_4090",
        "label": "GPU A800/4090",
        "percent": 20
      },
      {
        "key": "cpu_edge",
        "label": "CPU/边缘节点",
        "percent": 50
      }
    ],
    "statusBreakdown": {
      "all": {
        "nodes": 10,
        "compute": 100,
        "computeDisplayValue": "0.1",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 10,
          "usedDevices": 2,
          "usageRate": 20
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 30
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 20
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 50
          }
        ]
      },
      "online": {
        "nodes": 3,
        "compute": 40,
        "computeDisplayValue": "0",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 3,
          "usedDevices": 2,
          "usageRate": 67
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 67
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 33
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 0
          }
        ]
      },
      "offline": {
        "nodes": 7,
        "compute": 60,
        "computeDisplayValue": "0.1",
        "computeUnit": "PF",
        "resourceUsage": {
          "totalDevices": 7,
          "usedDevices": 0,
          "usageRate": 0
        },
        "clusterStack": [
          {
            "key": "a100_h100",
            "label": "GPU A100/H100",
            "percent": 14
          },
          {
            "key": "a800_4090",
            "label": "GPU A800/4090",
            "percent": 14
          },
          {
            "key": "cpu_edge",
            "label": "CPU/边缘节点",
            "percent": 72
          }
        ]
      }
    }
  }
}
```

## GET `/api/banking/admin/network-map`

获取地图城市节点、区域、重点省份和 Top 城市。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| nodeStatus | string | 是 | all | `all`、`online`、`offline` |
| status | string | 是 | all | `nodeStatus` 缺省时的兼容别名 |

支持中文值：

| 中文值 | 等价值 |
|---|---|
| 全部 | all |
| 在线 | online |
| 离线 | offline |

错误处理：

- 非法 `nodeStatus/status` 会返回 HTTP `400 Bad Request`。
- 当前实现的 400 响应体为空，不会返回 `{ code, message, data }` envelope。

默认行为：

```bash
/api/banking/admin/network-map
```

等价于：

```bash
/api/banking/admin/network-map?nodeStatus=all
```

也就是说，不传 `nodeStatus` 时展示全部节点，包含在线和离线/非在线节点。

### nodeStatus 对返回值的影响

`nodeStatus` 会先过滤节点集合，再计算以下字段：

| 字段 | 是否受 nodeStatus 影响 | 说明 |
|---|---|---|
| cities | 是 | 只聚合过滤后的节点 |
| regions | 是 | 只包含过滤后仍有城市数据的区域 |
| highlightProvinces | 是 | 基于过滤后的省份算力/节点数计算 |
| topCities | 是 | 基于过滤后的城市集合重新排序取 Top 5 |
| links | 否 | v1 固定返回空数组 |

`topCities` 排序规则：

1. `tflops` 降序
2. `nodes` 降序
3. `cityId` 升序

因此：

```bash
/api/banking/admin/network-map?nodeStatus=offline
```

返回的 `topCities` 是离线/非在线节点集合里的 Top 5，不会混入在线节点。

### NetworkCity

| 字段 | 类型 | 说明 |
|---|---|---|
| id | string | 城市稳定 id |
| name | string | 城市名称 |
| province | string | 省份名称 |
| coord | [number, number] | `[lng, lat]` |
| nodes | number | 当前过滤集合中的城市节点数 |
| tflops | number | 当前过滤集合中的城市算力，单位 TFLOPS |
| gpuModel | string | Top GPU 型号，多个用 `/` 拼接 |
| tier | string | `mega/large/medium/small` |
| onlineNodes | number | 当前城市聚合结果里的在线节点数 |
| usedNodes | number | 当前城市聚合结果里的使用中节点估算值 |

注意：

- `cities[].onlineNodes` 字段保留字面含义。
- 当 `nodeStatus=offline` 时，`cities[].onlineNodes` 通常为 `0`。
- `cities[].nodes` 才是当前过滤结果集的节点数。

### network-map 示例

```bash
# 全部节点，默认行为
curl "http://<host>:18081/api/banking/admin/network-map"

# 在线节点地图和 Top 5
curl "http://<host>:18081/api/banking/admin/network-map?nodeStatus=online"

# 离线/非在线节点地图和 Top 5
curl "http://<host>:18081/api/banking/admin/network-map?nodeStatus=offline"
```

## GET `/api/banking/admin/compute-nodes`

获取算力节点分页列表，用于节点表格。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| status | string | 是 | 无 | `active/online/warning/maintenance/offline/error` |
| device | string | 是 | 无 | `linux/windows/mac`；无法识别的系统归为 `unknown` |
| region | string | 是 | 无 | 匹配城市 id、城市名、地区或节点池文本 |
| keyword | string | 是 | 无 | 在 id/name/owner/region/GPU 中模糊搜索 |
| page | number | 是 | 1 | 页码 |
| pageSize | number | 是 | 20 | 每页数量，最大 200 |

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| items | ComputeNodeItem[] | 节点列表 |
| pagination | Pagination | 分页信息 |
| stats | ComputeNodeStats | 过滤前后统计 |

### ComputeNodeItem

| 字段 | 类型 | 说明 |
|---|---|---|
| id | string | `client_id` 的 hex 字符串 |
| name | string | `client_name` 或后端生成的备用名称 |
| owner | string | `user_id` 或后端生成的节点池名称 |
| region | string | 城市名或原始 geo 区域 |
| regionId | string/null | 城市 id |
| device | string | 归一化系统类型 |
| status | string | 归一化节点状态 |
| gpu | string | 例如 `2 x H100` |
| gpuModel | string/null | Top GPU 型号 |
| gpuCount | number/null | GPU 设备数量 |
| load | number | CPU/内存/磁盘/GPU 使用率中的最大值，0-100 |
| tokensPerSecond | number | 最近 10 秒该节点 Token/s |
| lastSeenAt | RFC3339 string | 最近资产/系统/设备更新时间 |
| lastSeenText | string/null | 后端格式化的相对时间 |

### compute-nodes 示例

```bash
curl "http://<host>:18081/api/banking/admin/compute-nodes?page=1&pageSize=20&device=linux"
```

## GET `/api/banking/admin/token-throughput`

获取 Token 吞吐折线图数据。

### 查询参数

| 参数 | 类型 | 是否可选 | 默认值 | 说明 |
|---|---:|:---:|---|---|
| windowSeconds | number | 是 | 180 | 聚合窗口，最大 86400 |
| intervalSeconds | number | 是 | 3 | 桶间隔，最大 300；后端可能自动调大以控制点数 |
| region | string | 是 | 无 | 按服务节点 `geo_city` 或 `geo_region` 过滤 |

### 返回字段

| 字段 | 类型 | 说明 |
|---|---|---|
| windowSeconds | number | 实际使用的窗口秒数 |
| intervalSeconds | number | 实际使用的桶间隔秒数 |
| latest | TokenThroughputPoint | 最新桶 |
| peaks | object | 当前窗口 input/output 峰值 TPS |
| totals | object | 当前窗口原始 token 汇总 |
| points | TokenThroughputPoint[] | 连续时间桶；空桶补 0 |

### TokenThroughputPoint

| 字段 | 类型 | 说明 |
|---|---|---|
| timestamp | RFC3339 string | 桶时间 |
| input | number | input tokens/s |
| output | number | output tokens/s |
| inputTokens | number | 桶内 input token 原始数量 |
| outputTokens | number | 桶内 output token 原始数量 |
| totalTokens | number | 桶内 token 原始总量 |

### token-throughput 示例

```bash
curl "http://<host>:18081/api/banking/admin/token-throughput?windowSeconds=180&intervalSeconds=30"
```

## 前端接入建议

- 首页卡片继续可以按 `summaryCards[].key` 读取。
- 新页面如果要做在线/离线/全部切换，优先使用 `statusBreakdown`。
- 地图页如果要切换在线/离线/全部，调用 `network-map?nodeStatus=<value>`。
- 地图页不传 `nodeStatus` 时就是全部节点。
- Token 卡片不要和当前在线/离线状态做强行对账。

## 算力资产预评估

预评估接口将在线节点数据或离线硬件证据转换为统一的
`gpuf.pre_evaluation.v1` 草稿快照。所有接口要求管理令牌：

```http
Authorization: Bearer <GPUF_BANKING_API_TOKEN>
```

服务端未配置至少 32 字符的令牌时，接口返回 HTTP `503`。令牌错误返回 HTTP `401`。
该令牌是服务间管理凭据，只能保存在后端或受控运维环境，不能下发到浏览器前端。
生产部署可使用逗号分隔的 `GPUF_BANKING_API_TOKENS` 同时配置新旧 Token 完成轮换；
`GPUF_BANKING_API_TOKEN` 作为单 Token 兼容配置保留。
当前阶段不接受调用方直接提交确权、基准数值、估值、质押率或手工硬件规格；旧客户端可以省略 `supplements` 或发送空对象，非空值返回 HTTP `422`。

在线和离线创建接口可携带 `Idempotency-Key`。作用域为经过 SHA-256 脱敏的服务主体、租户和操作；相同键与相同请求返回首次生成的报告，相同键与不同请求返回 HTTP `409`。不提供该请求头时保持旧客户端行为。`GPUF_BANKING_SERVICE_SUBJECT` 用于定义调用服务主体，数据库不保存原始主体或租户标识。

new-api 等内部编排服务可以使用以下等价路径：

```text
POST /internal/v1/technical-pre-evaluations/from-client
POST /internal/v1/technical-pre-evaluations/challenge
POST /internal/v1/technical-pre-evaluations/from-evidence
```

internal 路径与现有 banking 路径共用鉴权、请求体限制、证据校验、幂等存储和报告生成逻辑。internal 请求可以使用 `gpufUserRef`、`gpufClientRef`、`tenantRef` 和 `clientRequestId`；前两个字段分别是旧 `userId`、`clientId` 的别名。body 中的 `clientRequestId` 可以直接启用幂等；同时提供 `Idempotency-Key` 时二者必须完全一致，否则返回 HTTP `400`。显式租户引用必须通过格式校验，数据库只保存其 SHA-256 派生作用域。

### POST `/api/banking/provider/benchmark-evidence`

可信基准由受控 Benchmark Runner 先注册，再由预评估请求通过 `benchmarkEvidenceIds` 引用。注册接口使用独立 `GPUF_BENCHMARK_PRODUCER_TOKEN`，并按 `GPUF_BENCHMARK_ED25519_PUBLIC_KEYS_JSON` 中的 `keyId` 对 `payloadJson` 原始 UTF-8 字节执行 Ed25519 验签。生产必须设置 `GPUF_BENCHMARK_REQUIRE_KEY_METADATA=true`；每个 key 条目包含 `publicKeyBase64`、`status=active|retired|revoked`、`purpose=test_only|performance_claim`、`notBefore` 和 `notAfter`。缺少 purpose 时按 `test_only` 失败关闭；只有 `performance_claim` key 签名的证据可进入报告并参与评分，测试 key 仍可验证登记链路。只有当前有效的 active key 能登记新证据；retired key 只允许其有效窗口内已登记证据继续用于新报告；revoked key 的证据立即禁止进入新报告。Payload 必须绑定 64 位十六进制 `sourceRef`、参数 SHA-256、测试时间和不超过 30 天的有效期；证据表 INSERT-only。

`benchmarkEvidenceIds` 非空时严格加载指定且 key 状态可用的证据；为空时服务端按当前技术 `sourceRef` 自动选择每个 metric 最新且未过期、未吊销的一条已验签证据。若同指标最新记录来自 revoked key，会回退到上一条仍可用的记录。`scripts/run_signed_ollama_benchmark.sh` 一次运行会分别登记 `tokens_per_second` 和 `sustained_throughput_percent`，用于满足 T2 的 LLM 类别和 T1/T2 的稳定性类别。自动选择不会跨设备、接受过期证据或执行任意调用方命令。

`scripts/manage_benchmark_keyring.sh issue` 以 `0600` 权限签发 Ed25519 私钥并生成 managed keyring 条目；`transition` 只允许 `active -> retired/revoked` 和 `retired -> revoked`，拒绝撤销后恢复。常规轮换顺序为：先增加新 active key 并滚动部署 GPUFabric，再切换 Runner 的 `GPUF_BENCHMARK_KEY_ID`/私钥，确认新证据可登记后把旧 key 改为 retired；旧证据全部过期后才移除旧公钥。私钥疑似泄露时直接改为 revoked、滚动部署并重新采集受影响 Benchmark。已冻结的历史报告保持不可变，但 revoked 证据不能进入之后创建的报告。

报告请求不能直接提交任意性能数值、命令、脚本或镜像地址。不存在、过期或 `sourceRef` 与设备技术来源不一致的证据返回 HTTP `422`。

### POST `/api/banking/provider/pre-evaluations/from-client`

使用现有 `gpu_assets`、`system_info`、`device_info` 和最近 30 天
`device_daily_stats` 生成在线节点草稿。
未提供 `assetName` 时使用 GPU 型号，不把客户端名称或主机名写入报告；报告中的在线
来源引用使用 SHA-256，不直接暴露原始 `clientId`。

```json
{
  "userId": "1",
  "clientId": "00112233445566778899aabbccddeeff",
  "assetName": "GPU节点-A01",
  "benchmarkEvidenceIds": ["BENCH-2026-07-A01-TPS"]
}
```

### POST `/api/banking/provider/pre-evaluations/challenge`

签发一个 5 分钟有效且只能消费一次的离线采集 challenge。调用采集器时传入：

```bash
hw-asset-collector --challenge "$CHALLENGE" > report.json
```

### POST `/api/banking/provider/pre-evaluations/from-evidence`

`hardwareEvidenceJson` 必须是 `report.json` 的原始文本。前端应使用 `File.text()` 读取，
不能先解析再重新序列化。服务端重新计算采集器 SHA-256，并原子消费 challenge；哈希错误、
challenge 过期或重放均返回 HTTP `422`。最大 4 MiB。
只接受采集器默认的 `serials_redacted` 模式；包含非空序列号、UUID、WWN 或资产标签的
报告返回 HTTP `422`。

```json
{
  "userId": "1",
  "assetName": "离线GPU节点-01",
  "offlineAssetRef": "offline-asset:hmac:v1:<opaque>",
  "hardwareEvidenceJson": "<report.json 原始文本>",
  "benchmarkEvidenceIds": []
}
```

内部编排服务应提供稳定、脱敏且租户绑定的 `offlineAssetRef`。GPUFabric 使用固定协议
`gpuf.offline_asset_source.v1` 将它转换为 64 位 `sourceRef`；collector 的
`payloadSha256` 继续只证明本次 challenge 报告完整性。旧调用方省略该字段时保持原来的
单次 payload Hash 来源语义。可信 Runner 必须在提交同一份 collector JSON 前，使用会话
返回的稳定 `sourceRef` 登记 Benchmark；提交时空 `benchmarkEvidenceIds` 会自动关联。

运行历史中的 `runtime.observationDays` 只表示 collector 本地 JSONL 覆盖窗口，仍属于
自报告。GPUFabric 另按稳定 `sourceRef` 和 UTC 自然日，对最近 30 天内成功提交且其最新
运行样本距离提交不超过 10 分钟的 challenge 绑定不可变报告去重计数，结果写入
`runtime.serverObservationDays`。只有这个服务端计数达到 7 天才获得长期观测完整度和
证据分；不足时报告保留 `SERVER_OBSERVATION_WINDOW_SHORT`，并返回后续动作
`COLLECT_SERVER_RUNTIME_OBSERVATIONS`。

在线 `from-client` 报告使用 `gpuf.online_heartbeat_history.v1` 计算采样质量。每个 UTC
自然日只统计当天第一条到最后一条已存心跳之间的实际观测窗口，不假定设备全天在线；目标
间隔读取 `heartbeat_config_daily`，无配置时为 120 秒，实际样本数使用
`client_daily_stats` 去重采样桶，逐卡观测数使用 `device_daily_stats`，最大间隔由原始
`heartbeat` 时间戳计算。只有时间戳记录与每日聚合能够关联时才输出采样覆盖率、缺失采样、
最大采样间隔和缺失 GPU 观测；输入缺失或越界时保持 `null`。

`gpuf.runtime_history.v1` 还把采样覆盖率、最大间隔、缺失样本/逐卡观测以及高温、
接近功率上限、时钟限制、热/功率限频、硬件减速、驱动恢复动作、不可纠正 ECC 和待处理
显存修复计数归一化到 `runtime`，冻结 HTML 同步展示这些事实。覆盖率低于 90% 或出现
硬件异常时返回相应 `warningCodes` 和 `nextActions`；这些字段不直接改变技术分数。
字段口径与完整代码映射见 `docs/api/gpufabric-assessment-api.md`。

使用 `jq -Rs` 构造请求，只转义外层字符串，不重新序列化采集报告：

```bash
jq -Rs --arg userId "1" --arg assetName "离线GPU节点-01" \
  '{userId: $userId, assetName: $assetName, hardwareEvidenceJson: .}' \
  report.json > request.json
```

### GET `/api/banking/provider/pre-evaluations/{reportId}`

按报告编号读取保存的 JSON 快照。报告不存在返回 HTTP `404`；完整性哈希不匹配时返回
HTTP `500`，不会返回被修改的数据。

### GET `/api/banking/provider/pre-evaluations/{reportId}/html`

返回创建时冻结的技术预评估 HTML 字节，`Content-Type` 为 `text/html; charset=utf-8`，`ETag` 和 `X-Content-SHA256` 声明 `gpuf.report-html-bytes.v1` 字节 Hash。读取时重新计算 Hash，不一致返回 HTTP `500`。HTML 只展示技术事实、可信基准和结构化缺失项，不展示估值、质押率、贷款额或授信资格。

### GET `/internal/v1/technical-pre-evaluations/{reportId}`

以原始字节完整性信封返回不可变 v1 报告，包含 `reportJson`、`reportSha256` 和
`hashProfile: gpuf.report-json-bytes.v1`。新报告同时返回可选 `reportHtmlSha256` 与 `htmlHashProfile: gpuf.report-html-bytes.v1`，并新增可选 `technicalSnapshot` 引用；旧客户端可以忽略这些字段。

### GET `/internal/v2/technical-snapshots/{snapshotId}`

返回 `snapshotJson`、`snapshotSha256`、`hashProfile: gpuf.snapshot-json-bytes.v2` 和解析后的
技术快照。每个非空技术叶子字段都带 provenance，质量限定为 `measured`、`observed`、
`collected`、`catalog` 或 `derived`。快照不包含确权结论、市场金额、质押率、贷款额或银行结论。

完整、同构且命中服务端规格目录的 GPU 清单还会返回 `assetConfiguration`：

```json
{
  "schemaVersion": "gpuf.asset_configuration.v1",
  "hashProfile": "gpuf.asset-configuration-lines.v1",
  "canonicalModelId": "nvidia-a100-pcie-80gb",
  "deviceForm": "pcie_card",
  "gpuCount": 2,
  "memoryPerGpuBytes": 80,
  "configurationHash": "e60efc858d6231954cec34c58acd34d3ffbb59c44d73b8639f4a92d9afb8e9df"
}
```

Hash 输入是以下固定 UTF-8 字节（末尾包含换行），不是重新序列化后的 JSON：

```text
gpuf.asset_configuration.v1
canonicalModelId=<canonicalModelId>
deviceForm=<deviceForm>
gpuCount=<十进制整数>
memoryPerGpuBytes=<十进制整数>
```

型号 ID 或设备形态缺失、逐卡清单不完整、型号/形态/逐卡显存不一致时省略该对象，不猜测市场可比配置。

### DELETE `/api/banking/provider/pre-evaluations/{reportId}/evidence`

清除该报告临时保留的原始离线 JSON，但保留证据 SHA-256、报告快照和清除时间。
接口是幂等的；原文未保存或已清除时返回 `rawEvidencePurged: false`。

默认不保存原始离线 JSON，只保存完整文档 SHA-256。仅当
`GPUF_PRE_EVALUATION_STORE_RAW_EVIDENCE=true` 时临时保存原文，并要求
`GPUF_PRE_EVALUATION_RAW_EVIDENCE_TTL_DAYS` 为 1–90 天，缺省为 30 天。API 进程每小时
清理到期原文。未提供 `assetName` 时使用 GPU 型号，不把采集器主机名复制到报告快照。

### 结构化技术结论

新报告在保留 `missingEvidence` 和 `warnings` 兼容文本的同时，提供稳定的 `missingCodes`、`warningCodes` 和 `nextActions`。`evidenceScore` 与等级仅衡量技术证据质量；`valuation` 固定为 `null`，`eligibleForListing` 和 `eligibleForCreditPrecheck` 固定为 `false`。

### 安全和聚合边界

- `evidenceScore` 只是证据完整度，不是性能评分或银行信用评分。
- 在线来源标记为 `authenticated_client_telemetry`，表示数据来自已认证 gpuf-c 遥测，不等同于服务端硬件证明。
- 报告编号使用随机 UUID；快照 INSERT-only，并保存独立 SHA-256。
- 离线哈希证明报告在 challenge 生成后未被传输篡改，但不等同于 TPM/TEE 厂商硬件证明；响应标记为 `self_reported_challenge_bound`。
- v3 哈希只覆盖 collector、challenge 和 hardware；未被哈希覆盖的 `attestation.evidence_sources/warnings/missing_evidence` 不进入预评估结果。
- 报告快照不可修改；原始离线 JSON 默认不落库，显式临时留存时可按 TTL 或清除接口删除。
- GPU 规格按 `vendorId + deviceId` 优先匹配，不修改 `gpuf-c/common` 协议。
- 多 GPU 规格保留在逐卡结构中；异构节点不生成单一架构/TDP/带宽。
- 旧协议只有总显存时，多卡逐卡显存保持为空，不做平均猜测。
- 上报 GPU 数量与逐卡清单不一致时，不根据部分逐卡数据生成节点汇总，也不标记为可挂牌。
- 多卡互联拓扑未经探针验证时，不生成节点级互联带宽。
- 当前报告固定为 `draft`，不构成正式估值或授信承诺。
