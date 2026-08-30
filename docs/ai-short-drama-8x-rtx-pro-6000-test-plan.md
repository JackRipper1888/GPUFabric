# AI 短剧模型测试与部署方案

## 1. 目标

在 8 张 NVIDIA RTX PRO 6000 Blackwell Server Edition（每张 96GB、无 NVLink）的服务器上，评估适合商业 AI 短剧生产的开源或开放权重模型，重点比较：

- 人物和场景一致性
- 中文对白与口型同步
- 镜头运动和连续性
- 原生音频质量
- 单条镜头生成耗时和吞吐
- 显存占用、失败率和部署稳定性
- 商业许可证风险

本方案优先测试本地部署能力，不把仅能通过在线 API 获得的结果与本地模型结果混为一组。

## 2. 硬件基线

### 已确认信息

```text
GPU：8 x NVIDIA RTX PRO 6000 Blackwell Server Edition
显存：96GB/卡
CUDA：13.2
PyTorch：已安装
互联：无 NVLink，使用 PCIe/NCCL
```

已提供的 `nvidia-smi nvlink --status` 结果显示 GPU 0-3 均不支持 NVLink。请补充确认 GPU 4-7 的状态，并保存以下信息作为基线：

```bash
nvidia-smi --query-gpu=index,name,memory.total,driver_version,pci.bus_id \
  --format=csv

nvidia-smi topo -m

python - <<'PY'
import torch
print("torch:", torch.__version__)
print("torch cuda runtime:", torch.version.cuda)
print("cuda available:", torch.cuda.is_available())
for i in range(torch.cuda.device_count()):
    p = torch.cuda.get_device_properties(i)
    print(i, p.name, round(p.total_memory / 1024**3, 1), "GiB", p.major, p.minor)
PY
```

注意：系统 CUDA 13.2 和 PyTorch 自带的 CUDA runtime 可能不是同一个版本。不要因为 `nvcc --version` 是 13.2 就直接重装 PyTorch；先确认 `torch.version.cuda`、驱动和当前模型框架是否兼容。

## 3. 推荐模型

### 第一优先级：MiniMax H3

- 模型：[MiniMaxAI/MiniMax-H3](https://huggingface.co/MiniMaxAI/MiniMax-H3)
- 任务：文生视频、首尾帧生视频、参考图生视频、视频/音频条件生成
- 输出：4-15 秒、24 FPS、默认约 768p，原生立体声音频
- `Ref2VA`：适合多参考图和角色一致性
- `FL2VA`：适合首帧/尾帧控制和镜头转场
- 官方 SGLang 示例使用 4 张 GPU 和 `ulysses-degree=4`
- 初始开源版本为全注意力；稀疏注意力后续发布

H3 本地开源部分主要生成 768p。官方完整 2K 流程依赖尚未完全开源的 `Context-IR` 和 `Regenerate-2K` 服务；本地测试应单独标记为“768p 本地结果”。

### 第二优先级：Wan2.2 系列

- 模型集合：[Wan-AI](https://huggingface.co/Wan-AI)
- `Wan2.2-TI2V-5B`：低成本文图生视频，适合作为批量生产基线
- `Wan2.2-I2V-A14B`：关键帧到视频，画质优先
- `Wan2.2-S2V-14B`：对白、音乐驱动的人物表演
- `Wan2.2-Animate-14B`：角色动作复制和角色替换
- 模型卡标注 Apache 2.0，适合作为商业部署的许可证基线

### 第三优先级：SkyReels V3

- 模型集合：[SkyReels-V3](https://huggingface.co/collections/Skywork/skyreels-v3)
- `R2V-14B`：多参考图角色一致性
- `A2V-19B`：音频驱动数字人和对白镜头
- `V2V-14B`：视频续写和镜头延展
- Skywork 社区许可证明确支持商业用途，但部署互联网服务前需完成安全审查并遵守协议

### 第四优先级：HunyuanVideo-1.5

- 模型：[tencent/HunyuanVideo-1.5](https://huggingface.co/tencent/HunyuanVideo-1.5)
- 8.3B 参数，适合作为显存效率和中文能力对照
- 优先测试 480p I2V step-distilled，再测试 720p
- 使用 Tencent Hunyuan Community License，不是 Apache 2.0

### 配套模型

- 剧本、分镜和提示词：[Qwen3.5](https://huggingface.co/Qwen)
- 中文配音：[Fun-CosyVoice3-0.5B-2512](https://huggingface.co/FunAudioLLM/Fun-CosyVoice3-0.5B-2512)，模型卡标注 Apache 2.0
- 口型修正：MuseTalk 1.5、EchoMimic V3；这些模型的代码、权重和人脸资产许可证需要分别确认

## 4. 8 卡部署策略

### H3 双服务布局

无 NVLink 时，不建议一开始把同一个 H3 实例扩展到 8 卡。先按官方 4 卡配置部署两个独立服务：

```text
GPU 0-3：H3 Ref2VA，端口 30010
GPU 4-7：H3 FL2VA，端口 30011
```

示例：

```bash
CUDA_VISIBLE_DEVICES=0,1,2,3 \
sglang serve \
  --model-path /models/MiniMax-H3 \
  --num-gpus 4 \
  --ulysses-degree 4 \
  --performance-mode speed \
  --model-variant ref2va \
  --host 0.0.0.0 \
  --port 30010
```

```bash
CUDA_VISIBLE_DEVICES=4,5,6,7 \
sglang serve \
  --model-path /models/MiniMax-H3 \
  --num-gpus 4 \
  --ulysses-degree 4 \
  --performance-mode speed \
  --model-variant fl2va \
  --host 0.0.0.0 \
  --port 30011
```

如果 `nvidia-smi topo -m` 显示跨卡主要为 PCIe/PHB，记录 4 卡和 8 卡的通信开销，不要默认 8 卡并行一定更快。8 卡更适合作为两个 4 卡副本提升并发。

### 其他模型布局

H3 测试完成后释放显存，再运行以下模型：

```text
Wan2.2-TI2V-5B：1 卡、2 卡、4 卡
Wan2.2-I2V-A14B：4 卡、8 卡
Wan2.2-S2V-14B：4 卡、8 卡
SkyReels-V3-R2V-14B：4 卡、8 卡
SkyReels-V3-A2V-19B：4 卡、8 卡
HunyuanVideo-1.5：1 卡、2 卡、4 卡
```

每个模型单独使用官方 Diffusers、SGLang 或 ComfyUI 工作流，保存环境版本和启动参数。

## 5. 标准测试集

准备 10 个固定场景，每个场景使用同一套角色参考图和场景参考图：

1. 单人中文近景对白
2. 两人正反打对白
3. 三人室内争执
4. 古装人物进入房间
5. 现代办公室对话
6. 人物转身、拿手机、坐下等连续动作
7. 快速推镜和横向跟拍
8. 首帧到尾帧的镜头转场
9. 音频驱动人物表演
10. 连续三镜头中的角色一致性

每个模型、每个场景使用 3 个随机种子。每条视频先统一测试 5 秒和 10 秒；不支持 10 秒的模型使用连续镜头拼接测试。

## 6. 评测指标

### 质量评分

每项按 1-5 分记录，至少由两人盲评：

```text
角色一致性
场景一致性
动作自然度
提示词遵循
镜头连续性
画面瑕疵率
对白清晰度
音画同步
```

### 工程指标

```text
首帧加载时间
单条视频生成时间
每分钟视频生成成本
峰值显存
平均 GPU 利用率
显存溢出次数
生成失败率
4 卡与 8 卡扩展效率
```

运行监控：

```bash
nvidia-smi dmon -s pucm -d 1
```

检查视频属性：

```bash
ffprobe -v error \
  -show_entries stream=codec_type,width,height,r_frame_rate,sample_rate,channels \
  -of json output.mp4
```

## 7. 建议的执行顺序

### 阶段 A：环境和显存验证

1. 确认 GPU 4-7 和拓扑信息
2. 运行 PyTorch CUDA smoke test
3. 运行 H3 单个 4 卡服务
4. 生成一条 768p、5 秒视频
5. 确认显存、通信和输出音频正常

### 阶段 B：H3 短剧能力

1. `Ref2VA` 测角色一致性
2. `FL2VA` 测首尾帧和镜头转场
3. 测 1 人、2 人、3 人对白
4. 测 5 秒和 10 秒
5. 测 BF16 与社区 INT8/NVFP4 量化版本

### 阶段 C：商业基线

1. Wan2.2-TI2V-5B
2. Wan2.2-I2V-A14B
3. Wan2.2-S2V-14B
4. SkyReels-V3-R2V-14B
5. HunyuanVideo-1.5

### 阶段 D：完整短剧流水线

```text
Qwen3.5：剧本和分镜
FLUX/SDXL：角色设定和关键帧
H3 或 Wan2.2：视频镜头
CosyVoice3：最终对白
MuseTalk/EchoMimic：口型修正
FFmpeg：剪辑、字幕和音频混合
```

## 8. 通过标准

建议把以下标准作为第一轮筛选门槛：

```text
无 OOM，连续生成 30 条成功率 >= 90%
角色一致性 >= 4/5
镜头连续性 >= 3.5/5
动作自然度 >= 3.5/5
关键对白镜头音画同步 >= 4/5
许可证允许目标地区和商业规模
```

最终不必只选一个模型，建议保留两条路线：

```text
高质量路线：MiniMax H3 Ref2VA/FL2VA + CosyVoice3
稳妥商用路线：Wan2.2 TI2V/I2V/S2V + CosyVoice3
```

## 9. 商业许可证检查清单

- MiniMax H3：排除美国、欧盟、英国、韩国；年收入超过 2000 万美元需书面授权；商业界面需显示模型名称
- Wan2.2：模型卡标注 Apache 2.0，但仍需保留版权和依赖声明
- SkyReels V3：支持商业用途，但按 Skywork Community License 使用，并完成互联网服务安全审查
- HunyuanVideo-1.5：有地域限制、用户规模限制和 Tencent 专属条款
- CosyVoice3：模型卡标注 Apache 2.0
- 人脸、声音、音乐、字体和训练素材：需要另行取得肖像权、声音权和版权授权

## 10. 结果记录模板

```text
模型：
版本/Commit：
任务：
GPU 数量：
GPU 型号/显存：
精度：
分辨率：
视频时长：
Seed：
生成耗时：
峰值显存：
是否 OOM：
角色一致性：/5
动作自然度：/5
镜头连续性：/5
音画同步：/5
失败原因：
许可证备注：
```
