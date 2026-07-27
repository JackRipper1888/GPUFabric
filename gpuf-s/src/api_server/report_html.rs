use std::fmt::Write;

use super::pre_evaluation::Report;

pub fn render(report: &Report, report_sha256: &str) -> String {
    let mut gpu_rows = String::new();
    for gpu in &report.hardware.gpus {
        let _ = write!(
            gpu_rows,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            gpu.index,
            escape(&gpu.model),
            display_bytes(gpu.memory_bytes),
            display_theoretical_performance(gpu),
            escape(gpu.specification_version.as_deref().unwrap_or("-")),
        );
    }

    let mut benchmark_rows = String::new();
    for benchmark in &report.benchmarks {
        let _ = write!(
            benchmark_rows,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.2} {}</td><td>{}</td></tr>",
            escape(&benchmark.suite),
            escape(&benchmark.task),
            escape(&benchmark.metric),
            benchmark.value,
            escape(&benchmark.unit),
            escape(&benchmark.tested_at),
        );
    }
    if benchmark_rows.is_empty() {
        benchmark_rows.push_str("<tr><td colspan=\"5\">暂无可信实测基准</td></tr>");
    }

    let missing_codes = code_list(&report.evidence.missing_codes);
    let warning_codes = code_list(&report.evidence.warning_codes);
    let next_actions = code_list(&report.evidence.next_actions);
    let runtime_rows = runtime_rows(report);

    format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{report_id} · 算力资产技术预评估</title>
  <style>
    :root {{ --ink:#17202a; --muted:#5c6875; --line:#d5dce3; --canvas:#f4f6f8; --blue:#1759bb; --green:#0f766e; --amber:#b96508; }}
    * {{ box-sizing:border-box; }}
    body {{ margin:0; color:var(--ink); background:var(--canvas); font-family:Inter,"Noto Sans SC","Microsoft YaHei",Arial,sans-serif; font-size:14px; line-height:1.6; letter-spacing:0; }}
    header, main, footer {{ width:min(1120px,calc(100% - 32px)); margin:0 auto; }}
    header {{ padding:30px 0 22px; }}
    h1 {{ margin:0 0 8px; font-size:28px; }}
    h2 {{ margin:0 0 14px; font-size:18px; }}
    p {{ margin:0; }}
    .muted {{ color:var(--muted); }}
    .summary {{ display:grid; grid-template-columns:repeat(4,minmax(0,1fr)); border:1px solid var(--line); background:#fff; }}
    .metric {{ min-width:0; padding:16px; border-right:1px solid var(--line); }}
    .metric:last-child {{ border-right:0; }}
    .metric strong {{ display:block; margin-top:5px; color:var(--blue); font-size:21px; overflow-wrap:anywhere; }}
    section {{ margin-top:18px; padding:20px 0; border-top:1px solid var(--line); }}
    .table-wrap {{ overflow-x:auto; border:1px solid var(--line); background:#fff; }}
    table {{ width:100%; min-width:720px; border-collapse:collapse; }}
    th,td {{ padding:11px 13px; text-align:left; vertical-align:top; border-right:1px solid var(--line); border-bottom:1px solid var(--line); }}
    th:last-child,td:last-child {{ border-right:0; }}
    tr:last-child td {{ border-bottom:0; }}
    th {{ background:#eef1f4; white-space:nowrap; }}
    code {{ color:#174f98; overflow-wrap:break-word; }}
    .asset-table th {{ width:14%; }}
    .asset-table td {{ width:36%; }}
    .codes {{ display:grid; grid-template-columns:1fr; gap:10px; margin-top:14px; }}
    .codes > div {{ padding:15px; border:1px solid var(--line); background:#fff; }}
    ul {{ margin:8px 0 0; padding-left:20px; }}
    .notice {{ margin-top:20px; padding:14px 16px; border-left:4px solid var(--amber); background:#fff5e7; }}
    footer {{ padding:24px 0 36px; color:var(--muted); font-size:12px; }}
    @media(max-width:720px) {{ .summary,.codes {{ grid-template-columns:1fr; }} .metric {{ border-right:0; border-bottom:1px solid var(--line); }} .metric:last-child {{ border-bottom:0; }} }}
    @media print {{ body {{ background:#fff; }} header,main,footer {{ width:100%; }} section {{ break-inside:avoid; }} }}
  </style>
</head>
<body>
  <header>
    <h1>算力资产技术预评估报告</h1>
    <p class="muted">报告编号 {report_id} · 生成于 {generated_at} · 状态 {status}</p>
  </header>
  <main>
    <div class="summary">
      <div class="metric"><span>技术证据评分</span><strong>{score} / 100</strong></div>
      <div class="metric"><span>技术等级</span><strong>{grade}</strong></div>
      <div class="metric"><span>证据完整度</span><strong>{completeness}%</strong></div>
      <div class="metric"><span>可信基准数量</span><strong>{benchmark_count}</strong></div>
    </div>
    <section>
      <h2>资产与来源</h2>
      <div class="table-wrap"><table class="asset-table"><tbody>
        <tr><th>资产名称</th><td>{asset_name}</td><th>主要 GPU</th><td>{gpu_model}</td></tr>
        <tr><th>设备数量</th><td>{device_count}</td><th>来源类型</th><td>{source_type}</td></tr>
        <tr><th>来源完整性</th><td>{integrity}</td><th>技术快照</th><td>{snapshot_id}</td></tr>
      </tbody></table></div>
    </section>
    <section>
      <h2>GPU 技术台账</h2>
      <div class="table-wrap"><table><thead><tr><th>序号</th><th>型号</th><th>显存</th><th>理论性能</th><th>规格版本</th></tr></thead><tbody>{gpu_rows}</tbody></table></div>
    </section>
    <section>
      <h2>运行历史与健康观测</h2>
      <div class="table-wrap"><table class="asset-table"><tbody>{runtime_rows}</tbody></table></div>
    </section>
    <section>
      <h2>可信性能基准</h2>
      <div class="table-wrap"><table><thead><tr><th>套件</th><th>任务</th><th>指标</th><th>结果</th><th>测试时间</th></tr></thead><tbody>{benchmark_rows}</tbody></table></div>
    </section>
    <section>
      <h2>技术结论</h2>
      <p>{conclusion}</p>
      <div class="codes">
        <div><strong>缺失码</strong>{missing_codes}</div>
        <div><strong>警告码</strong>{warning_codes}</div>
        <div><strong>下一步</strong>{next_actions}</div>
      </div>
      <div class="notice">本报告仅描述设备技术事实和证据完整度，不包含权属确认、市场估值、质押率、贷款额度或银行授信结论。</div>
    </section>
  </main>
  <footer>JSON SHA-256: <code>{report_sha256}</code></footer>
</body>
</html>"#,
        report_id = escape(&report.report_id),
        generated_at = escape(&report.generated_at.to_rfc3339()),
        status = escape(report.report_status),
        score = report.assessment.evidence_score,
        grade = escape(report.assessment.grade),
        completeness = report.assessment.completeness_percent,
        benchmark_count = report.performance.benchmark_count,
        asset_name = escape(&report.asset.name),
        gpu_model = escape(&report.asset.primary_gpu_model),
        device_count = report.asset.device_count,
        source_type = escape(report.source.source_type),
        integrity = escape(report.source.integrity_level),
        snapshot_id = escape(
            report
                .technical_snapshot
                .as_ref()
                .map(|value| value.snapshot_id.as_str())
                .unwrap_or("-")
        ),
        conclusion = escape(&report.assessment.conclusion),
    )
}

fn runtime_rows(report: &Report) -> String {
    let Some(runtime) = report.runtime.as_ref() else {
        return "<tr><td colspan=\"4\">暂无运行历史</td></tr>".to_string();
    };
    format!(
        "<tr><th>本地观测天数</th><td>{}</td><th>服务端观测天数</th><td>{}</td></tr>\
         <tr><th>采样覆盖率</th><td>{}</td><th>最大采样间隔</th><td>{}</td></tr>\
         <tr><th>缺失采样</th><td>{}</td><th>缺失 GPU 观测</th><td>{}</td></tr>\
         <tr><th>高温观测</th><td>{}</td><th>接近功率上限</th><td>{}</td></tr>\
         <tr><th>时钟限制</th><td>{}</td><th>热限频</th><td>{}</td></tr>\
         <tr><th>功率限制</th><td>{}</td><th>硬件减速</th><td>{}</td></tr>\
         <tr><th>驱动要求恢复</th><td>{}</td><th>不可纠正 ECC 观测</th><td>{}</td></tr>\
         <tr><th>最大不可纠正 ECC</th><td>{}</td><th>待处理显存修复</th><td>{}</td></tr>",
        display_number(runtime.observation_days),
        display_number(runtime.server_observation_days),
        runtime
            .sample_coverage_percent
            .map(|value| format!("{value:.2}%"))
            .unwrap_or_else(|| "-".to_string()),
        runtime
            .maximum_sample_gap_seconds
            .map(|value| format!("{value} 秒"))
            .unwrap_or_else(|| "-".to_string()),
        display_number(runtime.missing_sample_count),
        display_number(runtime.missing_gpu_observation_count),
        display_number(runtime.high_temperature_observation_count),
        display_number(runtime.near_power_limit_observation_count),
        display_number(runtime.clock_limit_observation_count),
        display_number(runtime.thermal_throttle_observation_count),
        display_number(runtime.power_throttle_observation_count),
        display_number(runtime.hardware_slowdown_observation_count),
        display_number(runtime.recovery_action_required_observation_count),
        display_number(runtime.uncorrected_ecc_error_observation_count),
        display_number(runtime.max_uncorrected_ecc_errors),
        display_number(
            runtime
                .pending_page_retirement_observation_count
                .zip(runtime.pending_row_remap_observation_count)
                .and_then(|(pages, rows)| pages.checked_add(rows)),
        ),
    )
}

fn display_number<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn code_list(values: &[String]) -> String {
    if values.is_empty() {
        return "<p class=\"muted\">无</p>".to_string();
    }
    let items = values
        .iter()
        .map(|value| format!("<li><code>{}</code></li>", escape(value)))
        .collect::<String>();
    format!("<ul>{items}</ul>")
}

fn display_bytes(value: Option<u64>) -> String {
    value
        .map(|bytes| format!("{:.1} GiB", bytes as f64 / 1024_f64.powi(3)))
        .unwrap_or_else(|| "-".to_string())
}

fn display_theoretical_performance(gpu: &super::pre_evaluation::Gpu) -> String {
    [
        (gpu.fp16_tflops, "FP16", "TFLOPS"),
        (gpu.fp32_tflops, "FP32", "TFLOPS"),
        (gpu.int8_tops, "INT8", "TOPS"),
        (gpu.int4_tops, "INT4", "TOPS"),
    ]
    .into_iter()
    .find_map(|(value, precision, unit)| {
        value.map(|value| format!("{precision} {value:.2} {unit}"))
    })
    .unwrap_or_else(|| "-".to_string())
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_covers_markup_and_quotes() {
        assert_eq!(escape("<&>\"'"), "&lt;&amp;&gt;&quot;&#39;");
    }
}
