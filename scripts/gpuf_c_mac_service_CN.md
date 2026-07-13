# gpuf-c 线上节点运维脚本使用说明

本文档说明 `scripts/gpuf_c_mac_service.sh` 的使用方法。该脚本用于 macOS 线上节点运行 `gpuf-c`，支持后台启动、停止、重启、状态查看、日志查看、错误筛选、日志轮转和配置修改。

## 适用场景

- macOS Metal 版本 `gpuf-c`
- 线上服务地址：`agent.gpunexus.com`
- TLS 控制连接
- 本地二进制默认路径：`/usr/local/bin/gpuf-c`
- 证书默认路径：`/usr/local/bin/ca-cert.pem`
- 模型默认路径：`/usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf`

## 文件位置

脚本路径：

```bash
scripts/gpuf_c_mac_service.sh
```

默认配置文件：

```bash
$HOME/.gpuf-c.env
```

默认日志文件：

```bash
$HOME/gpuf-c.log
```

默认 PID 文件：

```bash
$HOME/.gpuf-c.pid
```

## 第一次初始化

在客户机器上先执行：

```bash
scripts/gpuf_c_mac_service.sh init
```

脚本会创建默认配置文件：

```bash
$HOME/.gpuf-c.env
```

配置文件权限会设置为 `600`。

## 修改 client id

客户节点需要设置自己的 `client_id`：

```bash
scripts/gpuf_c_mac_service.sh client-id 00112233445566778899aabbccddeeff
```

`client_id` 必须是 32 位 hex 字符。

## 常用启动命令

后台启动：

```bash
scripts/gpuf_c_mac_service.sh start
```

停止：

```bash
scripts/gpuf_c_mac_service.sh stop
```

重启：

```bash
scripts/gpuf_c_mac_service.sh restart
```

查看运行状态：

```bash
scripts/gpuf_c_mac_service.sh status
```

实时查看日志：

```bash
scripts/gpuf_c_mac_service.sh logs
```

查看最近 120 行日志：

```bash
scripts/gpuf_c_mac_service.sh tail
```

查看最近 300 行日志：

```bash
scripts/gpuf_c_mac_service.sh tail 300
```

筛选最近错误和警告：

```bash
scripts/gpuf_c_mac_service.sh errors
```

执行基础诊断：

```bash
scripts/gpuf_c_mac_service.sh doctor
```

打印实际启动命令：

```bash
scripts/gpuf_c_mac_service.sh cmd
```

轮转日志：

```bash
scripts/gpuf_c_mac_service.sh rotate-log
```

## 默认启动参数

默认等价于以下命令：

```bash
sudo nohup env RUST_LOG=gpuf_c=debug,common=info /usr/local/bin/gpuf-c \
  --client-id <CLIENT_ID> \
  --server-addr agent.gpunexus.com \
  --control-port 17000 \
  --proxy-port 17001 \
  --engine-type llama \
  --llama-model-path /usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf \
  --n-gpu-layers 99 \
  --n-ctx 2048 \
  --n-batch 512 \
  --control-tls \
  --control-tls-server-name agent.gpunexus.com \
  --cert-chain-path /usr/local/bin/ca-cert.pem \
  --local-port 11435 \
  > "$HOME/gpuf-c.log" 2>&1 &
```

实际命令以 `scripts/gpuf_c_mac_service.sh cmd` 输出为准。

## 配置文件说明

默认配置文件内容示例：

```bash
CLIENT_ID=00112233445566778899aabbccddeeff
SERVER_ADDR=agent.gpunexus.com
CONTROL_PORT=17000
PROXY_PORT=17001
TLS_SERVER_NAME=agent.gpunexus.com
MODEL_PATH=/usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf
CERT_CHAIN_PATH=/usr/local/bin/ca-cert.pem
GPUF_C_BIN=/usr/local/bin/gpuf-c
LOCAL_PORT=11435
USE_SUDO=1
RUST_LOG=gpuf_c=debug,common=info
N_GPU_LAYERS=99
N_CTX=2048
N_BATCH=512
```

可以使用 `set` 命令修改单个配置项：

```bash
scripts/gpuf_c_mac_service.sh set MODEL_PATH /usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf
scripts/gpuf_c_mac_service.sh set CERT_CHAIN_PATH /usr/local/bin/ca-cert.pem
scripts/gpuf_c_mac_service.sh set SERVER_ADDR agent.gpunexus.com
scripts/gpuf_c_mac_service.sh set TLS_SERVER_NAME agent.gpunexus.com
```

修改配置后，需要重启：

```bash
scripts/gpuf_c_mac_service.sh restart
```

## 自定义配置文件和日志路径

可以通过环境变量指定配置、日志和 PID 文件：

```bash
GPUF_C_CONFIG=/opt/gpuf-c/gpuf-c.env \
GPUF_C_LOG=/var/log/gpuf-c.log \
GPUF_C_PID=/var/run/gpuf-c.pid \
scripts/gpuf_c_mac_service.sh start
```

查看日志时也要带上同样的环境变量：

```bash
GPUF_C_LOG=/var/log/gpuf-c.log scripts/gpuf_c_mac_service.sh logs
```

## 上线前检查

确认二进制存在并可执行：

```bash
ls -lh /usr/local/bin/gpuf-c
```

确认模型存在：

```bash
ls -lh /usr/local/share/gpuf-c/models/bge-m3-q8_0.gguf
```

确认 CA 证书存在：

```bash
ls -lh /usr/local/bin/ca-cert.pem
```

执行脚本诊断：

```bash
scripts/gpuf_c_mac_service.sh doctor
```

## 推荐上线流程

```bash
scripts/gpuf_c_mac_service.sh init
scripts/gpuf_c_mac_service.sh client-id <客户节点32位client_id>
scripts/gpuf_c_mac_service.sh doctor
scripts/gpuf_c_mac_service.sh start
scripts/gpuf_c_mac_service.sh status
scripts/gpuf_c_mac_service.sh logs
```

确认日志中出现类似以下信息：

```text
Connected to control port (tls=true)
Login command written successfully
heartbeat
```

## 常见问题

### 1. 启动后马上退出

执行：

```bash
scripts/gpuf_c_mac_service.sh tail 120
```

重点检查：

- `gpuf-c binary not executable`
- `model file not found`
- `CA cert file not found`
- TLS 证书域名不匹配
- client id 未注册或格式错误

### 2. sudo 后台启动失败

脚本启动前会执行 `sudo -v`，因此第一次启动会在前台要求输入密码。不要直接手写 `nohup sudo ... &`，否则 sudo 可能在后台等待密码并导致启动失败。

如果当前机器不需要 sudo，可以设置：

```bash
scripts/gpuf_c_mac_service.sh set USE_SUDO 0
scripts/gpuf_c_mac_service.sh restart
```

### 3. 看不到日志

默认日志在：

```bash
$HOME/gpuf-c.log
```

查看：

```bash
scripts/gpuf_c_mac_service.sh logs
```

### 4. 修改模型后没有生效

修改配置后必须重启：

```bash
scripts/gpuf_c_mac_service.sh set MODEL_PATH /path/to/model.gguf
scripts/gpuf_c_mac_service.sh restart
```

### 5. TLS 证书域名问题

线上推荐：

```bash
SERVER_ADDR=agent.gpunexus.com
TLS_SERVER_NAME=agent.gpunexus.com
```

`TLS_SERVER_NAME` 必须匹配服务端证书 SAN。不要把 IP 写到 `TLS_SERVER_NAME`，除非证书 SAN 明确包含该 IP。

## 安装到客户机器

可以把脚本复制到固定位置，例如：

```bash
sudo mkdir -p /usr/local/gpuf-c
sudo cp scripts/gpuf_c_mac_service.sh /usr/local/gpuf-c/gpuf-c-service.sh
sudo chmod +x /usr/local/gpuf-c/gpuf-c-service.sh
```

之后客户使用：

```bash
/usr/local/gpuf-c/gpuf-c-service.sh init
/usr/local/gpuf-c/gpuf-c-service.sh client-id <客户节点32位client_id>
/usr/local/gpuf-c/gpuf-c-service.sh start
/usr/local/gpuf-c/gpuf-c-service.sh logs
```
