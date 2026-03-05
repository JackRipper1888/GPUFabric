use anyhow::{anyhow, Result};
use common::{Command, CommandV1, DevicesInfo, EngineType as CommonEngineType, OsType, SystemInfo};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

const CURRENT_VERSION: u32 = 1;

static WORKER_TCP_STREAM: OnceLock<Mutex<Option<Arc<Mutex<std::net::TcpStream>>>>> = OnceLock::new();
static WORKER_SERVER_ADDR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static WORKER_CONTROL_PORT: OnceLock<Mutex<Option<u16>>> = OnceLock::new();
static WORKER_CLIENT_ID: OnceLock<Mutex<Option<[u8; 16]>>> = OnceLock::new();
static WORKER_STOP_SIGNAL: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn os_type() -> OsType {
    #[cfg(target_os = "ios")]
    {
        return OsType::IOS;
    }
    #[cfg(target_os = "android")]
    {
        return OsType::ANDROID;
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        return OsType::NONE;
    }
}

pub fn get_tcp_stream() -> Option<Arc<Mutex<std::net::TcpStream>>> {
    WORKER_TCP_STREAM
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
}

pub async fn perform_login(
    server_addr: &str,
    control_port: u16,
    client_id_hex: &str,
    auto_models: bool,
) -> Result<()> {
    let mut stream = std::net::TcpStream::connect(format!("{}:{}", server_addr, control_port))
        .map_err(|e| anyhow!("Failed to connect to server: {}", e))?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();

    let system_info = SystemInfo::default();

    // Use a fixed DevicesInfo to avoid heavy platform-specific probes.
    let mut fixed_devices_info = DevicesInfo::default();
    fixed_devices_info.num = 1;
    fixed_devices_info.pod_id = 0;
    fixed_devices_info.os_type = os_type();
    fixed_devices_info.engine_type = CommonEngineType::Llama;
    // Default vendor/device ids to avoid server-side assumptions.
    fixed_devices_info.vendor_id = 0x41;
    fixed_devices_info.device_id = 0x1000;

    let client_id: [u8; 16] = hex::decode(client_id_hex)
        .unwrap_or_default()
        .try_into()
        .unwrap_or_default();

    let login_cmd = CommandV1::Login {
        version: CURRENT_VERSION,
        auto_models,
        os_type: os_type(),
        client_id,
        system_info,
        device_memtotal_gb: 0,
        device_total_tflops: 0,
        devices_info: vec![fixed_devices_info],
    };

    common::write_command_sync(&mut stream, &Command::V1(login_cmd))
        .map_err(|e| anyhow!("Failed to send login command: {}", e))?;

    let stream_arc = Arc::new(Mutex::new(stream));
    {
        let slot = WORKER_TCP_STREAM.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(stream_arc);
    }
    {
        let slot = WORKER_SERVER_ADDR.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(server_addr.to_string());
    }
    {
        let slot = WORKER_CONTROL_PORT.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(control_port);
    }
    {
        let slot = WORKER_CLIENT_ID.get_or_init(|| Mutex::new(None));
        let mut guard = slot.lock().unwrap();
        *guard = Some(client_id);
    }

    Ok(())
}

pub async fn start_worker_tasks_with_callback_ptr(
    callback: Option<extern "C" fn(*const c_char, *mut c_void)>,
) -> Result<()> {
    let tcp_stream = get_tcp_stream().ok_or_else(|| anyhow!("TCP connection not initialized"))?;

    let stop_signal = if let Some(existing) = WORKER_STOP_SIGNAL.get() {
        existing.clone()
    } else {
        let new_signal = Arc::new(AtomicBool::new(false));
        let _ = WORKER_STOP_SIGNAL.set(new_signal.clone());
        new_signal
    };
    stop_signal.store(false, Ordering::Relaxed);

    let heartbeat_stop = stop_signal.clone();
    let heartbeat_callback = callback;
    std::thread::spawn(move || loop {
        if heartbeat_stop.load(Ordering::Relaxed) {
            break;
        }

        if let Some(cb) = heartbeat_callback {
            if let Ok(msg) = std::ffi::CString::new("HEARTBEAT - Sending heartbeat to server") {
                unsafe { cb(msg.as_ptr(), std::ptr::null_mut()) };
            }
        }

        // Sleep 120s (match existing behavior)
        std::thread::sleep(std::time::Duration::from_secs(120));
    });

    let handler_stop = stop_signal.clone();
    std::thread::spawn(move || {
        // Keep a local stream clone so reads don't fight with other writes.
        let mut stream = match tcp_stream.lock().ok().and_then(|g| g.try_clone().ok()) {
            Some(s) => s,
            None => return,
        };

        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .ok();

        loop {
            if handler_stop.load(Ordering::Relaxed) {
                break;
            }

            let cmd = match common::read_command_sync(&mut stream) {
                Ok(c) => c,
                Err(e) => {
                    if let Some(ioe) = e.downcast_ref::<std::io::Error>() {
                        if matches!(
                            ioe.kind(),
                            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                        ) {
                            continue;
                        }
                    }
                    break;
                }
            };

            let Command::V1(v1) = cmd else {
                continue;
            };

            match v1 {
                CommandV1::InferenceTask {
                    task_id,
                    prompt,
                    max_tokens,
                    temperature,
                    top_k,
                    top_p,
                    repeat_penalty,
                    ..
                } => {
                    // Execute inference synchronously and send back chunks.
                    if let Err(_e) = handle_inference_task(
                        &mut stream,
                        task_id.clone(),
                        &prompt,
                        max_tokens,
                        temperature,
                        std::cmp::min(top_k, i32::MAX as u32) as i32,
                        top_p,
                        repeat_penalty,
                    ) {
                        // best-effort
                    }
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn handle_inference_task(
    stream: &mut std::net::TcpStream,
    task_id: String,
    prompt: &str,
    max_tokens: u32,
    temperature: f32,
    top_k: i32,
    top_p: f32,
    repeat_penalty: f32,
) -> Result<()> {
    use crate::{gpuf_generate_with_sampling, GLOBAL_CONTEXT_PTR, GLOBAL_MODEL_PTR, GLOBAL_INFERENCE_MUTEX};

    let _lock = GLOBAL_INFERENCE_MUTEX.lock().unwrap();

    let model_ptr = GLOBAL_MODEL_PTR.load(Ordering::SeqCst);
    let ctx_ptr = GLOBAL_CONTEXT_PTR.load(Ordering::SeqCst);

    if model_ptr.is_null() || ctx_ptr.is_null() {
        let result_command = CommandV1::InferenceResultChunk {
            task_id,
            seq: 0,
            delta: String::new(),
            phase: common::OutputPhase::Unknown,
            done: true,
            error: Some("Model not loaded - please load a model first".to_string()),
            prompt_tokens: 0,
            completion_tokens: 0,
            analysis_tokens: 0,
            final_tokens: 0,
        };
        let _ = common::write_command_sync(stream, &Command::V1(result_command));
        return Ok(());
    }

    let prompt_c = std::ffi::CString::new(prompt).map_err(|e| anyhow!("Invalid prompt: {}", e))?;

    let output_len = 16 * 1024;
    let mut output = vec![0i8; output_len];

    let token_buf_size = 4096;
    let mut token_buf: Vec<crate::LlamaToken> = vec![0; token_buf_size];

    let rc = unsafe {
        gpuf_generate_with_sampling(
            model_ptr,
            ctx_ptr,
            prompt_c.as_ptr(),
            max_tokens as i32,
            temperature,
            top_k,
            top_p,
            repeat_penalty,
            output.as_mut_ptr() as *mut c_char,
            output_len as i32,
            token_buf.as_mut_ptr(),
            token_buf_size as i32,
        )
    };

    if rc < 0 {
        let result_command = CommandV1::InferenceResultChunk {
            task_id,
            seq: 0,
            delta: String::new(),
            phase: common::OutputPhase::Unknown,
            done: true,
            error: Some(format!("Inference failed: {}", rc)),
            prompt_tokens: 0,
            completion_tokens: 0,
            analysis_tokens: 0,
            final_tokens: 0,
        };
        let _ = common::write_command_sync(stream, &Command::V1(result_command));
        return Ok(());
    }

    let text = unsafe { std::ffi::CStr::from_ptr(output.as_ptr() as *const c_char) }
        .to_string_lossy()
        .to_string();

    let mut seq: u32 = 0;
    let chunk_size = 256usize;
    for chunk in text.as_bytes().chunks(chunk_size) {
        let delta = String::from_utf8_lossy(chunk).to_string();
        let cmd = CommandV1::InferenceResultChunk {
            task_id: task_id.clone(),
            seq,
            delta,
            phase: common::OutputPhase::Unknown,
            done: false,
            error: None,
            prompt_tokens: 0,
            completion_tokens: rc as u32,
            analysis_tokens: 0,
            final_tokens: rc as u32,
        };
        seq = seq.wrapping_add(1);
        let _ = common::write_command_sync(stream, &Command::V1(cmd));
    }

    let done_cmd = CommandV1::InferenceResultChunk {
        task_id,
        seq,
        delta: String::new(),
        phase: common::OutputPhase::Unknown,
        done: true,
        error: None,
        prompt_tokens: 0,
        completion_tokens: rc as u32,
        analysis_tokens: 0,
        final_tokens: rc as u32,
    };

    let _ = common::write_command_sync(stream, &Command::V1(done_cmd));

    Ok(())
}

pub async fn stop_global_worker() {
    if let Some(stop) = WORKER_STOP_SIGNAL.get() {
        stop.store(true, Ordering::Relaxed);
    }

    if let Some(m) = WORKER_TCP_STREAM.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_SERVER_ADDR.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_CONTROL_PORT.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
    if let Some(m) = WORKER_CLIENT_ID.get() {
        if let Ok(mut guard) = m.lock() {
            *guard = None;
        }
    }
}

pub async fn get_worker_status() -> Result<String> {
    if get_tcp_stream().is_some() {
        Ok("Worker is running".to_string())
    } else {
        Ok("Worker not available".to_string())
    }
}
