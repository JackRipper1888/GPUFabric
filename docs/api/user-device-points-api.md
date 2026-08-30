# 用户设备收益 API

本文档单独描述积分中心使用的两个用户设备收益接口：

- 用户收益汇总：累计收益、今日收益、本月收益。
- 设备收益明细：按设备和日期分页查询每日收益记录。

## 1. 基本信息

| 项目 | 值 |
|---|---|
| 默认本地地址 | `http://<api-host>:18081` |
| 测试环境地址 | `http://<api-host>:18081` |
| Content-Type | `application/json` |
| 数据来源 | PostgreSQL `device_points_daily` |

API Server 本身不定义浏览器登录态。对外部署时，应由反向代理或部署层完成 TLS、身份认证和访问控制，不能直接信任前端传入的 `user_id`。

所有成功响应使用统一结构：

```json
{
  "success": true,
  "data": {},
  "message": "success",
  "timestamp": "2026-08-07T03:19:28Z"
}
```

所有失败响应使用统一结构：

```json
{
  "success": false,
  "data": null,
  "message": "error description",
  "timestamp": "2026-08-07T03:19:28Z"
}
```

## 2. 用户收益汇总

```http
GET /api/user/points/summary
```

返回指定用户设备产生的累计收益、今日收益和本月收益。该接口用于积分中心顶部的收益统计卡片。

### 2.1 请求参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|:---:|---|
| `user_id` | string | 是 | 用户 ID，长度为 1 至 64 个字符 |
| `client_id` | string | 否 | 客户端 ID，必须是 32 位十六进制字符串 |
| `client_name` | string | 否 | 客户端名称，使用不区分大小写的模糊匹配 |
| `device_id` | integer | 否 | GPU 型号或设备类型 ID |
| `device_index` | integer | 否 | 客户端内的物理设备索引 |

`device_id` 表示设备型号，同一型号可能对应多块物理设备。需要精确筛选一块设备时，应同时传递 `client_id` 和 `device_index`。

汇总接口不接受 `start_date` 和 `end_date`。今日和本月统计周期由服务端固定计算。

### 2.2 请求示例

查询用户全部设备的收益汇总：

```bash
curl "http://<api-host>:18081/api/user/points/summary?user_id=example-user"
```

查询指定客户端中的一块物理设备：

```bash
curl "http://<api-host>:18081/api/user/points/summary?user_id=example-user&client_id=00000000000000000000000000000000&device_index=0"
```

### 2.3 响应字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `total_points` | number | 截止 `as_of_date` 的累计正收益 |
| `today_points` | number | `as_of_date` 当日的正收益 |
| `month_points` | number | 当月第一天至 `as_of_date` 的正收益 |
| `as_of_date` | string | 数据库结算日期，格式为 `YYYY-MM-DD` |

收益保留两位小数时采用截断口径，与设备收益明细接口保持一致。

### 2.4 响应示例

```json
{
  "success": true,
  "data": {
    "total_points": 21.3,
    "today_points": 0.0,
    "month_points": 0.0,
    "as_of_date": "2026-08-07"
  },
  "message": "success",
  "timestamp": "2026-08-07T03:19:28.192771Z"
}
```

### 2.5 日期与时区

`today_points`、`month_points` 和 `as_of_date` 使用 PostgreSQL 会话的 `CURRENT_DATE`。部署环境必须将数据库会话时区设置为业务结算时区，避免前端自然日与服务端结算日期不一致。

## 3. 设备收益明细

```http
GET /api/user/points
```

按设备和结算日期查询用户收益明细。每条记录表示一块物理设备在一个结算日期内的聚合结果，并不是单次收益流水。

### 3.1 请求参数

| 参数 | 类型 | 必填 | 说明 |
|---|---|:---:|---|
| `user_id` | string | 是 | 用户 ID，长度为 1 至 64 个字符 |
| `client_id` | string | 否 | 客户端 ID，必须是 32 位十六进制字符串 |
| `client_name` | string | 否 | 客户端名称，不区分大小写的模糊匹配 |
| `device_id` | integer | 否 | GPU 型号或设备类型 ID |
| `device_index` | integer | 否 | 客户端内的物理设备索引 |
| `start_date` | string | 否 | 开始日期，格式为 `YYYY-MM-DD`，包含当天 |
| `end_date` | string | 否 | 结束日期，格式为 `YYYY-MM-DD`，包含当天 |
| `page` | integer | 否 | 页码，范围为 1 至 100，默认值为 1 |
| `page_size` | integer | 否 | 每页记录数，范围为 1 至 100，默认值为 20 |

当 `start_date` 和 `end_date` 同时存在时，`start_date` 不能晚于 `end_date`。

### 3.2 请求示例

查询全部设备收益：

```bash
curl "http://<api-host>:18081/api/user/points?user_id=example-user&page=1&page_size=20"
```

按日期范围查询：

```bash
curl "http://<api-host>:18081/api/user/points?user_id=example-user&start_date=2026-07-01&end_date=2026-08-07&page=1&page_size=20"
```

精确查询一块物理设备：

```bash
curl "http://<api-host>:18081/api/user/points?user_id=example-user&client_id=00000000000000000000000000000000&device_index=0&start_date=2026-07-01&end_date=2026-08-07"
```

按 GPU 型号查询：

```bash
curl "http://<api-host>:18081/api/user/points?user_id=example-user&device_id=5510&page=1&page_size=20"
```

### 3.3 顶层响应字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `points` | array | 当前页的设备每日收益记录 |
| `total_points` | number | 当前筛选条件下的收益总额，不限于当前页 |
| `total_count` | integer | 当前筛选条件下的记录总数 |
| `page` | integer | 当前页码 |
| `page_size` | integer | 每页记录数 |

### 3.4 收益记录字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `client_id` | string | 客户端 ID，32 位十六进制字符串 |
| `client_name` | string | 客户端显示名称 |
| `date` | string | 结算日期，格式为 `YYYY-MM-DD` |
| `total_heartbeats` | integer | 当日参与收益计算的心跳数量 |
| `device_name` | string | 设备名称 |
| `device_id` | integer | GPU 型号或设备类型 ID |
| `device_index` | integer | 客户端内的物理设备索引 |
| `contributed_hours` | number | 当日计入收益的贡献时长，单位为小时 |
| `tflops` | number/null | 配置的设备计算能力；未知时为 `null` |
| `points` | number | 该设备在该结算日期产生的收益 |

### 3.5 响应示例

```json
{
  "success": true,
  "data": {
    "points": [
      {
        "client_id": "00000000000000000000000000000000",
        "client_name": "example-client",
        "date": "2026-07-16",
        "total_heartbeats": 81,
        "device_name": "example-gpu",
        "device_id": 5510,
        "device_index": 0,
        "contributed_hours": 2.0,
        "tflops": 16.0,
        "points": 0.1
      }
    ],
    "total_points": 21.3,
    "total_count": 34,
    "page": 1,
    "page_size": 20
  },
  "message": "success",
  "timestamp": "2026-08-07T03:19:29.023273Z"
}
```

## 4. 常见错误

### 4.1 客户端 ID 格式错误

请求：

```http
GET /api/user/points/summary?user_id=example-user&client_id=invalid
```

响应状态：`400 Bad Request`

```json
{
  "success": false,
  "data": null,
  "message": "invalid client_id: expected 32-char hex string",
  "timestamp": "2026-08-07T03:19:28Z"
}
```

### 4.2 日期范围错误

请求：

```http
GET /api/user/points?user_id=example-user&start_date=2026-08-07&end_date=2026-08-01
```

响应状态：`400 Bad Request`

```json
{
  "success": false,
  "data": null,
  "message": "start_date must not be later than end_date",
  "timestamp": "2026-08-07T03:19:29Z"
}
```

### 4.3 服务端查询失败

数据库不可用或查询执行失败时返回 `500 Internal Server Error`：

```json
{
  "success": false,
  "data": null,
  "message": "internal server error",
  "timestamp": "2026-08-07T03:19:29Z"
}
```

## 5. 前端调用建议

积分中心首次加载时分别请求：

1. `/api/user/points/summary?user_id=example-user`，展示累计、今日和本月收益。
2. `/api/user/points?user_id=example-user&page=1&page_size=20`，展示设备收益记录。

设备筛选和日期筛选只需要刷新明细接口。若产品要求顶部统计卡片跟随设备筛选，可将相同的 `client_id`、`client_name`、`device_id` 或 `device_index` 参数传给汇总接口；汇总接口仍固定返回累计、今日和本月口径。
