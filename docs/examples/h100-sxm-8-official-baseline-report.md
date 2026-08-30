# 8 卡 H100 评估报告基线（官方规格版）

> 报告编号：`EXAMPLE-NOT-ISSUED-H100-SXM-8-20260722`
> 状态：`incomplete_official_baseline`
> 重要：这不是某台真实 8 卡 H100 服务器的实测报告。当前没有可访问的 8 卡 H100 节点，因此所有“实机采集、长期运行、可信 Benchmark、权属、市场和估值”字段均保持缺失或 `null`。不能将本文件用于挂牌、授信、估值或资产验收。

完整机器可读字段见 [`h100-sxm-8-official-baseline-report.json`](h100-sxm-8-official-baseline-report.json)。

## 1. 数据来源和可信边界

本基线只使用 NVIDIA 官方资料：

- [NVIDIA H100 Tensor Core GPU](https://www.nvidia.com/en-us/data-center/h100/)
- [NVIDIA DGX H100 Datasheet](https://www.nvidia.com/content/dam/en-zz/Solutions/Data-Center/nvidia-dgx-h100-datasheet.pdf)
- [Introduction to NVIDIA DGX H100/H200 Systems](https://docs.nvidia.com/dgx/dgxh100-user-guide/introduction-to-dgxh100.html)

这些资料提供的是 H100 SXM 和 DGX H100 参考系统的厂商规格，不证明目标资产确实安装了该配置，也不提供目标机器的运行历史或 Benchmark。报告中使用以下来源分级：

| 来源级别 | 含义 | 本文件是否有 |
|---|---|---:|
| `manufacturer_catalog` | NVIDIA 官方规格或参考系统参数 | 有 |
| `collected` / `observed` | collector 或 GPUFabric 从目标机器采集 | 无 |
| `measured` | 目标机器上实际运行的 Benchmark | 无 |
| `verified` | 审核服务核验过的权属、生命周期或市场材料 | 无 |
| `derived` | 从同一目标资产的已验证字段计算 | 只有官方规格的算术求和 |

## 2. 官方 8 卡基线

以下是 NVIDIA DGX H100 参考配置，不能替代目标机器的 collector 结果：

| 指标 | 官方值 | 口径 |
|---|---:|---|
| GPU | 8 × NVIDIA H100 SXM | 参考系统 |
| 单卡显存 | 80 GB HBM3 | 官方标称容量 |
| 总显存 | 640 GB | 8 卡标称容量；JSON 同时保留二进制字节数 |
| FP32 | 67 TFLOPS/卡，536 TFLOPS 简单求和 | 理论规格 |
| FP16/BF16 Tensor | 1,979 TFLOPS/卡（稀疏），989.5 TFLOPS/卡（稠密） | 不能当作业务实测吞吐 |
| FP8 Tensor | 3,958 TFLOPS/卡（稀疏），1,979 TFLOPS/卡（稠密） | DGX 官方约 32 PFLOPS 为 FP8 标称 |
| INT8 Tensor | 3,958 TOPS/卡（稀疏），约 1,979 TOPS/卡（稠密） | 理论规格 |
| HBM 带宽 | 3.35 TB/s/卡 | 单卡接口带宽 |
| NVLink | 900 GB/s/卡 | NVSwitch/NVLink 参考带宽 |
| 单卡最大 TDP | 700 W | 可配置上限，非当前功耗 |
| 参考系统最大输入功率 | 10.2 kW | DGX 整机，不是 GPU TDP 求和 |
| NVSwitch | 4 个第四代 NVSwitch | DGX H100 参考系统 |
| CPU | 2 × Intel Xeon Platinum 8480C，56 核/颗 | DGX H100 参考系统 |
| 系统内存 | 2 TB | DGX H100 参考系统 |
| 系统盘 | 2 × 1.92 TB NVMe M.2，RAID 1 | DGX H100 参考系统 |
| 数据盘 | 8 × 3.84 TB NVMe U.2 SED，RAID 0 | DGX H100 参考系统 |
| 集群网络 | 8 × ConnectX-7，最高 400 Gb/s | DGX H100 参考系统 |
| 工作温度 | 5–30°C | 机房环境参考范围 |

### 2.1 8 卡算术求和的解释

JSON 中 `officialSpecificationExtension.eightGpuArithmeticTotals` 保存了每卡厂商值的简单求和。它们仅用于规格比较：

- FP8 稀疏：31,664 TFLOPS，即约 31.7 PFLOPS，官方 DGX 资料将其约写为 32 PFLOPS。
- FP16/BF16 稀疏：15,832 TFLOPS；稠密：7,916 TFLOPS。
- HBM 带宽简单求和：26.8 TB/s；这不是 NCCL、模型训练或推理的有效带宽。
- GPU 最大 TDP 简单求和：5.6 kW；整机最大输入功率仍应使用官方 10.2 kW 口径。

## 3. 全量指标状态

### 3.1 必须由目标机器采集的硬件指标

当前均为缺失：GPU 序列号哈希、PCI 设备 ID、VBIOS、InfoROM、驱动、CUDA、固件、MIG、PCIe 拓扑、NVLink/NVSwitch 拓扑、主机身份哈希、实际 CPU/内存/存储和网卡配置。不能从“DGX H100 参考系统”自动补齐。

### 3.2 运行稳定性和健康指标

当前均为缺失：每卡利用率/显存占用/温度/功耗/频率序列，采样数、采样间隔、窗口起止时间、7 天以上观测天数、ECC 可纠正/不可纠正错误、退休页/行、Xid、掉卡、降频原因、PCIe replay/error、NVLink CRC/replay、NVSwitch 错误和 DCGM 诊断结果。

`runtime.observationDays` 为 `0`。这意味着本文件没有任何长期运行稳定性结论，不能显示为“稳定”或“运行正常”。

### 3.3 可信 Benchmark

`benchmarks` 为空、`performance.benchmarkCount` 为 `0`。缺少以下目标机器实测值：DCGM burn-in、NCCL all-reduce、HPL/HPL-AI、FP8/FP16/BF16 GEMM、显存带宽、LLM tokens/s、TTFT 和持续吞吐百分比。

Benchmark 必须由受控 Runner 运行并签名，且绑定同一资产的稳定 `sourceRef`。前端不能输入或覆盖 Benchmark 数值。

### 3.4 正式评估指标

当前全部未开始或未核验：

- 权属：采购合同、发票、资产编号、所有权主体。
- 生命周期：出厂、上架、投产、保修、维修、成色。
- 市场：版本化市场快照、样本来源、时间、区域和条件。
- 估值：定价策略、币种、估值区间、质押率、授信参考额。
- 审核：材料审核、技术核验、估值审核、报告冻结、签发和撤销。

因此 `valuation` 为 `null`，`eligibleForListing` 和 `eligibleForCreditPrecheck` 均为 `false`，`grade` 为 `UNASSESSED`。

## 4. 转为真实评估报告的操作

在目标 8 卡 H100 服务器上完成以下闭环后，才能生成真实报告：

1. 通过 new-api 创建离线会话，获取一次性 challenge。
2. 在目标机运行 `hw-asset-collector`，收集完整硬件和现场运行序列；生产环境不要使用 `--include-serials`。
3. 使用历史文件跨进程保留至少 7 个不同自然日的采样，并由 GPUFabric 认证遥测或受控 Runner 生成可信 Benchmark。
4. 将 collector 原始 JSON 字节提交给 new-api；不要解析后重新编码。
5. 补交权属、采购、生命周期、维护和市场材料，等待 assessment-service 审核和签发。

collector 的短时现场采样可以证明“采样时观察到的状态”，但少于 7 天会保留 `SHORT_OBSERVATION_WINDOW`，不能自动升级成长期稳定性结论。

## 5. 缺失码

`ACTUAL_ASSET_EVIDENCE_MISSING`、`RUNTIME_HISTORY_MISSING`、`TRUSTED_BENCHMARK_MISSING`、`OWNERSHIP_EVIDENCE_MISSING`、`LIFECYCLE_EVIDENCE_MISSING`、`MARKET_EVIDENCE_MISSING`、`VALUATION_MISSING`。

完整机器可读缺失字段、扩展健康指标和正式评估占位字段见 JSON 文件；前端页面字段和接口来源见 new-api 仓库的 `docs/banking/frontend-assessment-api-and-fields.md`。
