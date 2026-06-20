use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;
use ssh2::Session;
use chrono::Utc;
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::models::{ServerConfig, GpuConfig, AppConfig, GpuInfo, ServerGpuData, GlobalState};
use crate::config_store;

const GPU_CACHE_FILE: &str = "gpu_data_cache.json";

pub fn load_gpu_cache(app: &AppHandle) -> HashMap<String, ServerGpuData> {
    let items: Vec<ServerGpuData> = config_store::read_config(app, GPU_CACHE_FILE);
    items
        .into_iter()
        .map(|item| (item.host.clone(), item))
        .collect()
}

pub fn hydrate_gpu_from_cache(app: &AppHandle, state: &GlobalState) -> Vec<ServerGpuData> {
    let cached = load_gpu_cache(app);
    if cached.is_empty() {
        return Vec::new();
    }
    if let Ok(mut data) = state.gpu_data.lock() {
        if data.is_empty() {
            for (host, item) in cached {
                data.insert(host, item);
            }
        }
        data.values().cloned().collect()
    } else {
        cached.into_values().collect()
    }
}

pub fn persist_gpu_data_cache(app: &AppHandle, state: &GlobalState) {
    let items: Vec<ServerGpuData> = match state.gpu_data.lock() {
        Ok(data) if !data.is_empty() => data.values().cloned().collect(),
        _ => return,
    };
    if let Err(e) = config_store::write_config(app, GPU_CACHE_FILE, &items) {
        log::warn!("Failed to persist GPU data cache: {}", e);
    }
}

fn gpu_server_fingerprint(server: &ServerConfig) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    server.host.hash(&mut hasher);
    server.port.unwrap_or(22).hash(&mut hasher);
    server.id.as_deref().unwrap_or("").hash(&mut hasher);
    server.user.as_deref().unwrap_or("").hash(&mut hasher);
    server.password.as_deref().unwrap_or("").hash(&mut hasher);
    server.key_file.as_deref().unwrap_or("").hash(&mut hasher);
    server.use_slurm.hash(&mut hasher);
    format!("{}:{:016x}", server.host, hasher.finish())
}

pub fn parse_nvidia_smi_output(output: &str) -> Vec<GpuInfo> {
    let mut list = Vec::new();
    for line in output.lines() {
        let mut node_id = None;
        let mut content = line;

        // Handle Slurm --label output: "0: name, used, ..."
        if line.contains(": ") {
            let parts: Vec<&str> = line.splitn(2, ": ").collect();
            if parts.len() == 2 && parts[0].chars().all(|c| c.is_numeric()) {
                node_id = Some(parts[0].trim().to_string());
                content = parts[1];
            }
        }

        let parts: Vec<&str> = content.split(',').collect();
        if parts.len() >= 6 {
            list.push(GpuInfo {
                node: node_id,
                name: parts[0].trim().to_string(),
                mem_used: parts[1].trim().parse().unwrap_or(0.0),
                mem_total: parts[2].trim().parse().unwrap_or(0.0),
                util: parts[3].trim().parse().unwrap_or(0.0),
                temp: parts[4].trim().parse().ok(),
                power: parts[5].trim().parse().ok(),
                job_id: None,
            });
        }
    }
    list
}

pub fn ssh_authenticate(sess: &mut Session, s: &ServerConfig) -> Result<(), String> {
    let user = s.user.as_deref().unwrap_or("root");
    
    // 1. Try custom key file if provided and not empty
    if let Some(key_path) = &s.key_file {
        if !key_path.trim().is_empty() {
            let expanded = shellexpand::tilde(key_path).to_string();
            return sess.userauth_pubkey_file(user, None, std::path::Path::new(&expanded), None)
                .map_err(|e| format!("Key auth failed for custom key '{}': {}", key_path, e));
        }
    }
    
    // 2. Try password auth if password is provided and not empty
    if let Some(pass) = &s.password {
        if !pass.is_empty() {
            return sess.userauth_password(user, pass)
                .map_err(|e| format!("Password auth failed: {}", e));
        }
    }
    
    // 3. Fallback to default keys and SSH agent
    let default_keys = [
        "~/.ssh/id_ed25519",
        "~/.ssh/id_rsa",
        "~/.ssh/id_ecdsa",
        "~/.ssh/id_dsa",
    ];
    let mut authenticated = false;
    let mut last_err = None;
    for key_path in &default_keys {
        let expanded = shellexpand::tilde(key_path).to_string();
        let path = std::path::Path::new(&expanded);
        if path.exists() {
            match sess.userauth_pubkey_file(user, None, path, None) {
                Ok(_) => {
                    authenticated = true;
                    break;
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                }
            }
        }
    }
    if !authenticated {
        if let Err(agent_err) = sess.userauth_agent(user) {
            return Err(format!(
                "Authentication failed. Tried default keys (last error: {:?}) and SSH agent (error: {})",
                last_err, agent_err
            ));
        }
    }
    Ok(())
}

fn connect_ssh_session(s: &ServerConfig) -> Result<Session, String> {
    connect_ssh_session_with_read_timeout(s, Duration::from_secs(30))
}

fn connect_ssh_session_with_read_timeout(
    s: &ServerConfig,
    read_timeout: Duration,
) -> Result<Session, String> {
    use std::net::ToSocketAddrs;

    let host_id = format!("{}:{}", s.host, s.port.unwrap_or(22));
    let addr = host_id
        .to_socket_addrs()
        .map_err(|e| format!("Invalid SSH address {}: {}", host_id, e))?
        .next()
        .ok_or_else(|| format!("Could not resolve SSH address {}", host_id))?;

    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(15))
        .map_err(|e| format!("TCP connect failed: {}", e))?;
    let _ = tcp.set_read_timeout(Some(read_timeout));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(30)));

    let mut sess = Session::new().map_err(|e| e.to_string())?;
    sess.set_timeout(30000);
    sess.set_tcp_stream(tcp);
    sess.handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;
    ssh_authenticate(&mut sess, s)?;
    Ok(sess)
}

fn is_ssh_session_alive(sess: &Session) -> bool {
    let mut channel = match sess.channel_session() {
        Ok(channel) => channel,
        Err(_) => return false,
    };
    if channel.exec("echo widgitron-ok").is_err() {
        return false;
    }
    let mut output = String::new();
    if channel.read_to_string(&mut output).is_err() {
        return false;
    }
    let _ = channel.wait_close();
    output.contains("widgitron-ok")
}

fn reuse_or_connect_ssh_session(
    sess_opt: Option<Session>,
    server: &ServerConfig,
) -> Result<Session, String> {
    if let Some(sess) = sess_opt {
        if is_ssh_session_alive(&sess) {
            return Ok(sess);
        }
        log::debug!("SSH session for {} is stale, reconnecting", server.host);
    }

    let sess = connect_ssh_session(server)?;

    if server.use_slurm.unwrap_or(false) {
        if let Ok(mut clean_chan) = sess.channel_session() {
            let user = server.user.as_deref().unwrap_or("root");
            let cleanup_cmd = format!(
                "steps=$(squeue -s --me -h -o \"%i %j\" 2>/dev/null || squeue -s -u $(whoami) -h -o \"%i %j\" || squeue -s -u {} -h -o \"%i %j\"); targets=$(echo \"$steps\" | grep \"widgitron-gpu\" | awk '{{print $1}}'); [ -n \"$targets\" ] && scancel $targets",
                user
            );
            let _ = clean_chan.exec(&cleanup_cmd);
            let mut dummy = String::new();
            let _ = clean_chan.read_to_string(&mut dummy);
        }
    }

    Ok(sess)
}

pub fn start_ssh_monitor_task(
    app: AppHandle,
    state: Arc<GlobalState>,
    server: ServerConfig,
    jid: Option<String>,
    node_count: Option<String>,
    interval: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let smi_cmd = "nvidia-smi --query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw --format=csv,noheader,nounits";
        loop {
            let app_inner = app.clone();
            let state_inner = state.clone();
            let s_m = server.clone();
            let j_m = jid.clone();
            let n_m = node_count.clone();
            
            let res = tokio::task::spawn_blocking(move || -> Result<(), String> {
                let my_key = match &j_m {
                    Some(id) => format!("{}:{}:{}", s_m.host, id, n_m.as_deref().unwrap_or("1")),
                    None => format!("{}:node:0", s_m.host),
                };
                let timeout_secs = (interval * 3).max(15) + 10;
                let sess = connect_ssh_session_with_read_timeout(
                    &s_m,
                    Duration::from_secs(timeout_secs),
                )?;
                
                let mut channel = sess.channel_session().map_err(|e| format!("Channel open failed: {}", e))?;
                let watch_cmd = match &j_m {
                    Some(id) => {
                        let n_arg = match &n_m {
                            Some(n) => format!("-n {} --ntasks-per-node=1", n),
                            None => "--ntasks-per-node=1".to_string(),
                        };
                        format!("srun --jobid {} --overlap {} --label --job-name=widgitron-gpu sh -c 'while true; do {} || exit; echo \"END_BATCH\" || exit; sleep {}; done'", id, n_arg, smi_cmd, interval)
                    },
                    None => format!("sh -c 'while true; do {} || exit; echo \"END_BATCH\" || exit; sleep {}; done'", smi_cmd, interval),
                };
                
                channel.exec(&watch_cmd).map_err(|e| format!("Command exec failed: {}", e))?;
                
                let mut task_batches: HashMap<String, String> = HashMap::new();
                let reader = std::io::BufReader::new(channel);
                use std::io::BufRead;
                for line in reader.lines() {
                    let l = line.map_err(|e| format!("Read line error: {}", e))?;
                    // Identify task ID from Slurm --label prefix (e.g., "0: ...")
                    let task_id = if l.contains(": ") {
                        let parts: Vec<&str> = l.splitn(2, ": ").collect();
                        if parts.len() == 2 && parts[0].trim().chars().all(|c| c.is_numeric()) {
                            parts[0].trim().to_string()
                        } else { "default".to_string() }
                    } else { "default".to_string() };

                    if l.contains("END_BATCH") {
                        let app_config = config_store::read_config::<AppConfig>(&app_inner, "app_config.json");
                        if !app_config.gpu_enabled.unwrap_or(true) {
                            return Err("GPU monitoring disabled".to_string());
                        }

                        let still_active = {
                            if let Ok(monitors) = state_inner.active_monitors.lock() {
                                monitors.contains_key(&my_key)
                            } else {
                                true
                            }
                        };
                        if !still_active {
                            return Ok(());
                        }

                        let batch = task_batches.entry(task_id.clone()).or_default();
                        let mut parsed = parse_nvidia_smi_output(batch);
                        if !parsed.is_empty() {
                            for p in &mut parsed { p.job_id = j_m.clone(); }
                            
                            let node_to_replace = parsed[0].node.clone();

                            if let Ok(mut state_gpu) = state_inner.gpu_data.lock() {
                                let data = state_gpu.entry(s_m.host.clone()).or_insert(ServerGpuData {
                                    host: s_m.host.clone(),
                                    is_online: true,
                                    gpu_list: vec![],
                                    error: None,
                                    last_update: None,
                                    slurm_steps: None,
                                    slurm_nodelists: None,
                                    slurm_times: None,
                                });
                                
                                data.is_online = true;
                                data.error = None;
                                
                                if let Some(node) = node_to_replace {
                                    data.gpu_list.retain(|g| !(g.job_id == j_m && g.node == Some(node.clone())));
                                } else {
                                    data.gpu_list.retain(|g| g.job_id != j_m);
                                }
                                
                                data.gpu_list.extend(parsed.clone());
                                data.last_update = Some(Utc::now().format("%H:%M:%S").to_string());
                                let data_clone = data.clone();
                                let _ = app_inner.emit("gpu_update", data_clone);
                            }
                        }
                        batch.clear();
                    } else {
                        let batch = task_batches.entry(task_id).or_default();
                        batch.push_str(&l);
                        batch.push('\n');
                    }
                }
                Err("SSH stream closed EOF".to_string())
            }).await;
            
            match res {
                Ok(Ok(())) => {
                    break;
                }
                Ok(Err(err_msg)) => {
                    log::warn!("SSH monitor task for {} failed: {}", server.host, err_msg);
                    if let Ok(mut state_gpu) = state.gpu_data.lock() {
                        let entry = state_gpu.entry(server.host.clone()).or_insert_with(|| ServerGpuData {
                            host: server.host.clone(),
                            is_online: false,
                            gpu_list: vec![],
                            error: None,
                            last_update: None,
                            slurm_steps: None,
                            slurm_nodelists: None,
                            slurm_times: None,
                        });
                        let has_cached_gpus = !entry.gpu_list.is_empty();
                        entry.is_online = has_cached_gpus;
                        entry.error = Some(if has_cached_gpus {
                            format!("{} (showing cached data)", err_msg)
                        } else {
                            err_msg.clone()
                        });
                        entry.last_update = Some(Utc::now().format("%H:%M:%S").to_string());
                        let data_clone = entry.clone();
                        let _ = app.emit("gpu_update", data_clone);
                    }
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
                Err(join_err) => {
                    log::error!("SSH monitor task for {} panicked or cancelled: {}", server.host, join_err);
                    break;
                }
            }
        }
    })
}

pub async fn start_gpu_monitor(app: AppHandle, state: Arc<GlobalState>) {
    let smi_cmd = "nvidia-smi --query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw --format=csv,noheader,nounits";

        {
            let snapshot = hydrate_gpu_from_cache(&app, state.as_ref());
            let config = config_store::read_config::<GpuConfig>(&app, "gpu_monitor.json");
            let configured_hosts: std::collections::HashSet<String> = config
                .servers
                .iter()
                .map(|s| s.host.clone())
                .collect();
            let mut emitted = 0usize;
            for item in snapshot {
                if configured_hosts.contains(&item.host) {
                    let _ = app.emit("gpu_update", item);
                    emitted += 1;
                }
            }
            if emitted > 0 {
                log::info!("Loaded GPU data cache for {} host(s)", emitted);
            }
        }

    // Startup delay to let frontend initialize cleanly
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut last_cache_persist = std::time::Instant::now();

    loop {
        let app_config = config_store::read_config::<AppConfig>(&app, "app_config.json");
        let gpu_enabled = app_config.gpu_enabled.unwrap_or(true);

        let config = config_store::read_config::<GpuConfig>(&app, "gpu_monitor.json");

        let mut current_server_ids = Vec::new();
        if gpu_enabled {
            for server in &config.servers {
                if server.host.trim().is_empty() {
                    continue;
                }
                let server_id = gpu_server_fingerprint(server);
                current_server_ids.push(server_id.clone());

                let mut workers = state.active_workers.lock().unwrap_or_else(|e| e.into_inner());
                let needs_start = match workers.get(&server_id) {
                    None => true,
                    Some(h) => h.is_finished(),
                };
                if needs_start {
                    let app_inner = app.clone();
                    let state_inner = state.clone();
                    let server_inner = server.clone();
                    let smi_cmd_inner = smi_cmd.to_string();
                    let update_interval = config.update_interval.unwrap_or(5);

                    let handle = tokio::spawn(async move {
                        log::info!("--- Starting persistent worker for host: {} ---", server_inner.host);
                        let mut session: Option<Session> = None;
                        let mut last_squeue_update = Utc::now() - Duration::from_secs(60);
                        let mut slurm_job_ids: Vec<String> = Vec::new();
                        let mut slurm_nodelists: HashMap<String, String> = HashMap::new();
                        let mut slurm_times: HashMap<String, String> = HashMap::new();
                        let mut slurm_steps: HashMap<String, Vec<crate::models::SlurmStep>> = HashMap::new();

                        loop {
                            let res = tokio::task::spawn_blocking({
                                let s = server_inner.clone();
                                let smi = smi_cmd_inner.clone();
                                let state_task = state_inner.clone();
                                let app_task = app_inner.clone();
                                let sess_opt = session.take();
                                let mut job_ids = slurm_job_ids.clone();
                                let mut nodelists = slurm_nodelists.clone();
                                let mut times = slurm_times.clone();
                                let mut steps = slurm_steps.clone();
                                let squeue_needed = (Utc::now() - last_squeue_update).num_seconds() >= 30;

                                move || -> Result<(
                                    Option<Session>,
                                    Vec<String>,
                                    HashMap<String, String>,
                                    HashMap<String, String>,
                                    HashMap<String, Vec<crate::models::SlurmStep>>
                                ), String> {
                                    let mut gpu_data = ServerGpuData {
                                        host: s.host.clone(),
                                        is_online: false,
                                        gpu_list: vec![],
                                        error: None,
                                        last_update: None,
                                        slurm_steps: None,
                                        slurm_nodelists: None,
                                        slurm_times: None,
                                    };

                                    if s.host == "localhost" || s.host == "127.0.0.1" {
                                        let local_smi_args = ["--query-gpu=name,memory.used,memory.total,utilization.gpu,temperature.gpu,power.draw", "--format=csv,noheader,nounits"];
                                        
                                        #[cfg(windows)]
                                        let output = std::process::Command::new("nvidia-smi")
                                            .args(local_smi_args)
                                            .creation_flags(0x08000000) // CREATE_NO_WINDOW
                                            .output();
                                        
                                        #[cfg(not(windows))]
                                        let output = std::process::Command::new("nvidia-smi")
                                            .args(local_smi_args)
                                            .output();

                                        let cached_snapshot = state_task
                                            .gpu_data
                                            .lock()
                                            .ok()
                                            .and_then(|data| data.get(&s.host).cloned());

                                        let mut local_smi_error: Option<String> = None;
                                        match output {
                                            Ok(out) if out.status.success() => {
                                                let s_out = String::from_utf8_lossy(&out.stdout);
                                                let parsed = parse_nvidia_smi_output(&s_out);
                                                if parsed.is_empty() {
                                                    local_smi_error = Some(
                                                        "nvidia-smi returned no GPU data".to_string(),
                                                    );
                                                } else {
                                                    gpu_data.gpu_list = parsed;
                                                    gpu_data.is_online = true;
                                                    gpu_data.error = None;
                                                    gpu_data.last_update =
                                                        Some(Utc::now().format("%H:%M:%S").to_string());
                                                }
                                            }
                                            Ok(out) => {
                                                let stderr = String::from_utf8_lossy(&out.stderr)
                                                    .trim()
                                                    .to_string();
                                                local_smi_error = Some(if stderr.is_empty() {
                                                    format!(
                                                        "nvidia-smi exited with status {}",
                                                        out.status
                                                    )
                                                } else {
                                                    format!("nvidia-smi failed: {}", stderr)
                                                });
                                            }
                                            Err(e) => {
                                                local_smi_error =
                                                    Some(format!("Local smi failed: {}", e));
                                            }
                                        }

                                        if let Some(err) = local_smi_error {
                                            gpu_data.is_online = false;
                                            if let Some(cached) = cached_snapshot {
                                                if !cached.gpu_list.is_empty() {
                                                    gpu_data.gpu_list = cached.gpu_list;
                                                    gpu_data.last_update = cached.last_update;
                                                    gpu_data.error = Some(format!(
                                                        "{} (showing cached data)",
                                                        err
                                                    ));
                                                } else {
                                                    gpu_data.error = Some(err);
                                                }
                                            } else {
                                                gpu_data.error = Some(err);
                                            }
                                            if gpu_data.last_update.is_none() {
                                                gpu_data.last_update =
                                                    Some(Utc::now().format("%H:%M:%S").to_string());
                                            }
                                        }
                                        
                                        if let Ok(mut data) = state_task.gpu_data.lock() {
                                            data.insert(s.host.clone(), gpu_data.clone());
                                        }
                                        let _ = app_task.emit("gpu_update", gpu_data);
                                        
                                        Ok((None, vec![], HashMap::new(), HashMap::new(), HashMap::new()))
                                    } else {
                                        // SSH Logic
                                        let sess = reuse_or_connect_ssh_session(sess_opt, &s)?;

                                        gpu_data.is_online = true;
                                        let mut desired_monitor_keys = Vec::new();
                                        
                                        if s.use_slurm.unwrap_or(false) {
                                             let user = s.user.as_deref().unwrap_or("root");
                                             let mut job_nodes = HashMap::new();
                                             if squeue_needed {
                                                 let mut squeue_success = false;
                                                 if let Ok(mut channel) = sess.channel_session() {
                                                     let q_cmd = format!("squeue --me -t RUNNING -h -o \"%A|%D|%N|%M\" 2>/dev/null || squeue -t RUNNING -u $(whoami) -h -o \"%A|%D|%N|%M\" || squeue -t RUNNING -u {} -h -o \"%A|%D|%N|%M\"", user);
                                                     if let Ok(_) = channel.exec(&q_cmd) {
                                                         let mut s_q = String::new();
                                                         if channel.read_to_string(&mut s_q).is_ok() {
                                                             let lines: Vec<String> = s_q.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                                                             let mut new_nodelists = HashMap::new();
                                                             let mut new_times = HashMap::new();
                                                             for line in lines {
                                                                 let parts: Vec<&str> = line.split('|').collect();
                                                                 if parts.len() >= 4 {
                                                                     job_nodes.insert(parts[0].to_string(), parts[1].to_string());
                                                                     new_nodelists.insert(parts[0].to_string(), parts[2].to_string());
                                                                     new_times.insert(parts[0].to_string(), parts[3].to_string());
                                                                 } else if parts.len() == 3 {
                                                                     job_nodes.insert(parts[0].to_string(), parts[1].to_string());
                                                                     new_nodelists.insert(parts[0].to_string(), parts[2].to_string());
                                                                 } else if parts.len() == 2 {
                                                                     job_nodes.insert(parts[0].to_string(), parts[1].to_string());
                                                                 } else if !line.is_empty() {
                                                                     job_nodes.insert(line, "1".to_string());
                                                                 }
                                                             }
                                                             job_ids = job_nodes.keys().cloned().collect();
                                                             nodelists = new_nodelists;
                                                             times = new_times;
                                                             squeue_success = true;
                                                         }
                                                     }
                                                 }
                                                 if !squeue_success {
                                                     log::warn!(
                                                         "squeue query failed for {}, retaining cached Slurm state",
                                                         s.host
                                                     );
                                                     let has_cached_jobs = !job_ids.is_empty();
                                                     if let Ok(data) = state_task.gpu_data.lock() {
                                                         if let Some(cached) = data.get(&s.host) {
                                                             let has_cached_gpus =
                                                                 !cached.gpu_list.is_empty();
                                                             if has_cached_gpus || has_cached_jobs {
                                                                 gpu_data.gpu_list =
                                                                     cached.gpu_list.clone();
                                                                 gpu_data.slurm_nodelists =
                                                                     cached.slurm_nodelists.clone();
                                                                 gpu_data.slurm_times =
                                                                     cached.slurm_times.clone();
                                                                 gpu_data.slurm_steps =
                                                                     cached.slurm_steps.clone();
                                                                 gpu_data.is_online = has_cached_gpus;
                                                                 gpu_data.error = Some(
                                                                     "squeue failed (showing cached job data)"
                                                                         .to_string(),
                                                                 );
                                                                 gpu_data.last_update = Some(
                                                                     Utc::now()
                                                                         .format("%H:%M:%S")
                                                                         .to_string(),
                                                                 );
                                                             } else if !has_cached_jobs {
                                                                 return Err(
                                                                     "Failed to query squeue. SSH session may be dead."
                                                                         .to_string(),
                                                                 );
                                                             }
                                                         } else if !has_cached_jobs {
                                                             return Err(
                                                                 "Failed to query squeue. SSH session may be dead."
                                                                     .to_string(),
                                                             );
                                                         }
                                                     } else if !has_cached_jobs {
                                                         return Err(
                                                             "Failed to query squeue. SSH session may be dead."
                                                                 .to_string(),
                                                         );
                                                     }
                                                 }
                                             }
                                                
                                                // Now, if we have job_ids, query sacct for SubmitLine of these job steps!
                                                let mut submit_lines = HashMap::new();
                                                if !job_ids.is_empty() {
                                                    if let Ok(mut channel) = sess.channel_session() {
                                                        let sacct_cmd = format!("sacct -j {} -o JobID,SubmitLine%250 -n -P", job_ids.join(","));
                                                        if let Ok(_) = channel.exec(&sacct_cmd) {
                                                            let mut s_acct = String::new();
                                                            let _ = channel.read_to_string(&mut s_acct);
                                                            let lines: Vec<String> = s_acct.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                                                            for line in lines {
                                                                let parts: Vec<&str> = line.split('|').collect();
                                                                if parts.len() >= 2 {
                                                                    let key = parts[0].trim().to_string(); // e.g. "12345.0"
                                                                    let submit_line = parts[1].trim().to_string();
                                                                    if !submit_line.is_empty() {
                                                                        submit_lines.insert(key, submit_line);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                
                                                // Query Slurm Job Steps
                                                if let Ok(mut channel) = sess.channel_session() {
                                                    let s_cmd = format!("squeue -s --me -h -o \"%i|%j|%M\" 2>/dev/null || squeue -s -u $(whoami) -h -o \"%i|%j|%M\" || squeue -s -u {} -h -o \"%i|%j|%M\"", user);
                                                    if let Ok(_) = channel.exec(&s_cmd) {
                                                        let mut s_s = String::new();
                                                        let _ = channel.read_to_string(&mut s_s);
                                                        let lines: Vec<String> = s_s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
                                                        let mut new_steps = HashMap::new();
                                                        for line in lines {
                                                            let parts: Vec<&str> = line.split('|').collect();
                                                            if parts.len() >= 3 {
                                                                let step_id = parts[0].to_string();
                                                                if let Some(dot_idx) = step_id.find('.') {
                                                                    let step_part = &step_id[dot_idx + 1..];
                                                                    // Only list numeric computation steps
                                                                    if step_part.chars().all(|c| c.is_numeric()) {
                                                                        let job_id = step_id[..dot_idx].to_string();
                                                                        
                                                                        // Look up SubmitLine command
                                                                        let mut cmd = submit_lines.get(&step_id).cloned().unwrap_or_else(|| parts[1].to_string());
                                                                        
                                                                        // Clean up "srun" prefix and arguments if present to keep it neat
                                                                        if cmd.starts_with("srun") {
                                                                            let mut cmd_words = Vec::new();
                                                                            let mut is_cmd = false;
                                                                            let mut skip_next = false;
                                                                            for w in cmd.split_whitespace() {
                                                                                if w == "srun" {
                                                                                    continue;
                                                                                }
                                                                                if skip_next {
                                                                                    skip_next = false;
                                                                                    continue;
                                                                                }
                                                                                if is_cmd {
                                                                                    cmd_words.push(w);
                                                                                } else if w.starts_with('-') {
                                                                                    if w == "-n" || w == "-N" || w == "-c" || w == "-w" || w == "-p" || w == "-J" || w == "-t" || w == "-D" || w == "-e" || w == "-i" || w == "-o" || w == "-m" || w == "-A" {
                                                                                        skip_next = true;
                                                                                    }
                                                                                } else {
                                                                                    is_cmd = true;
                                                                                    cmd_words.push(w);
                                                                                }
                                                                            }
                                                                            if !cmd_words.is_empty() {
                                                                                cmd = cmd_words.join(" ");
                                                                            }
                                                                        }
                                                                        
                                                                        let step = crate::models::SlurmStep {
                                                                            id: step_id,
                                                                            name: parts[1].to_string(),
                                                                            time: parts[2].to_string(),
                                                                            command: cmd,
                                                                        };
                                                                        new_steps.entry(job_id).or_insert_with(Vec::new).push(step);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        steps = new_steps;
                                                    }
                                                }
                                            
                                            for jid in &job_ids {
                                                let n_count = job_nodes.get(jid).cloned().unwrap_or_else(|| "1".to_string());
                                                desired_monitor_keys.push(format!("{}:{}:{}", s.host, jid, n_count));
                                            }
                                        } else {
                                            desired_monitor_keys.push(format!("{}:node:0", s.host));
                                        }

                                        // Ensure monitor tasks are running
                                        for key in &desired_monitor_keys {
                                            let mut monitors = state_task.active_monitors.lock().unwrap_or_else(|e| e.into_inner());
                                            let needs_start = match monitors.get(key) {
                                                None => true,
                                                Some(h) => h.is_finished(),
                                            };
                                            if needs_start {
                                                let parts: Vec<&str> = key.split(':').collect();
                                                let (jid, n_count) = if parts.len() >= 3 {
                                                    if parts[1] == "node" { (None, None) } 
                                                    else { (Some(parts[1].to_string()), Some(parts[2].to_string())) }
                                                } else { (None, None) };

                                                let handle = start_ssh_monitor_task(
                                                    app_task.clone(),
                                                    state_task.clone(),
                                                    s.clone(),
                                                    jid,
                                                    n_count,
                                                    update_interval,
                                                );
                                                monitors.insert(key.clone(), handle);
                                            }
                                        }
                                        
                                        // Cleanup monitors for THIS host that are no longer needed
                                        {
                                            let mut monitors = state_task.active_monitors.lock().unwrap_or_else(|e| e.into_inner());
                                            let host_prefix = format!("{}:", s.host);
                                            let mut removed_jids = Vec::new();
                                            monitors.retain(|key, handle| {
                                                if key.starts_with(&host_prefix) {
                                                    if !desired_monitor_keys.contains(key) {
                                                        handle.abort();
                                                        let parts: Vec<&str> = key.split(':').collect();
                                                        if parts.len() >= 2 && parts[1] != "node" {
                                                            removed_jids.push(parts[1].to_string());
                                                        }
                                                        false
                                                    } else {
                                                        true
                                                    }
                                                } else {
                                                    true
                                                }
                                            });

                                            if !removed_jids.is_empty() {
                                                if let Ok(mut data) = state_task.gpu_data.lock() {
                                                    if let Some(server_data) = data.get_mut(&s.host) {
                                                        server_data.gpu_list.retain(|g| {
                                                            if let Some(jid) = &g.job_id {
                                                                !removed_jids.contains(jid)
                                                            } else {
                                                                true
                                                            }
                                                        });
                                                    }
                                                }
                                            }
                                        }

                                        // Sync GPU data from global state (updated by background monitors)
                                        if let Ok(data) = state_task.gpu_data.lock() {
                                            if let Some(cached) = data.get(&s.host) {
                                                gpu_data.gpu_list = cached.gpu_list.clone();
                                                if s.use_slurm.unwrap_or(false) {
                                                    let has_gpus = !gpu_data.gpu_list.is_empty();
                                                    gpu_data.is_online = has_gpus || cached.is_online;
                                                    gpu_data.error = if has_gpus {
                                                        cached.error.as_ref().filter(|e| e.contains("showing cached")).cloned()
                                                    } else {
                                                        cached.error.clone()
                                                    };
                                                    gpu_data.last_update = cached.last_update.clone()
                                                        .or_else(|| Some(Utc::now().format("%H:%M:%S").to_string()));
                                                } else {
                                                    gpu_data.is_online = cached.is_online;
                                                    gpu_data.error = cached.error.clone();
                                                    gpu_data.last_update = cached.last_update.clone();
                                                }
                                            } else {
                                                // No cached data yet
                                                if s.use_slurm.unwrap_or(false) {
                                                    gpu_data.is_online = true;
                                                    gpu_data.error = None;
                                                    gpu_data.last_update = Some(Utc::now().format("%H:%M:%S").to_string());
                                                } else {
                                                    gpu_data.is_online = false;
                                                    gpu_data.error = Some("Waiting for initial data...".to_string());
                                                }
                                            }
                                        }

                                         // Fallback poll if no jobs or no data yet
                                         if gpu_data.gpu_list.is_empty() && !s.use_slurm.unwrap_or(false) {
                                             let mut query_success = false;
                                             if let Ok(mut channel) = sess.channel_session() {
                                                 if let Ok(_) = channel.exec(&smi) {
                                                     let mut s_out = String::new();
                                                     if channel.read_to_string(&mut s_out).is_ok() {
                                                         gpu_data.gpu_list = parse_nvidia_smi_output(&s_out);
                                                         query_success = true;
                                                         gpu_data.is_online = true;
                                                         gpu_data.error = None;
                                                         gpu_data.last_update = Some(Utc::now().format("%H:%M:%S").to_string());
                                                     }
                                                 }
                                             }
                                             if !query_success {
                                                 return Err("Failed to query nvidia-smi. SSH session may be dead.".to_string());
                                             }
                                         } else if gpu_data.gpu_list.is_empty() && s.use_slurm.unwrap_or(false) {
                                             let cached_snapshot = state_task
                                                 .gpu_data
                                                 .lock()
                                                 .ok()
                                                 .and_then(|data| data.get(&s.host).cloned());
                                             let mut smi_error: Option<String> = None;
                                             if let Ok(mut channel) = sess.channel_session() {
                                                 if channel.exec(&smi).is_ok() {
                                                     let mut s_out = String::new();
                                                     if channel.read_to_string(&mut s_out).is_ok() {
                                                         let parsed = parse_nvidia_smi_output(&s_out);
                                                         if parsed.is_empty() {
                                                             smi_error = Some(
                                                                 "nvidia-smi returned no GPU data on login node".to_string(),
                                                             );
                                                         } else {
                                                             gpu_data.gpu_list = parsed;
                                                             gpu_data.is_online = true;
                                                             gpu_data.error = None;
                                                             gpu_data.last_update =
                                                                 Some(Utc::now().format("%H:%M:%S").to_string());
                                                         }
                                                     } else {
                                                         smi_error = Some(
                                                             "Failed to read nvidia-smi output".to_string(),
                                                         );
                                                     }
                                                 } else {
                                                     smi_error = Some(
                                                         "Failed to exec nvidia-smi on login node".to_string(),
                                                     );
                                                 }
                                             } else {
                                                 smi_error = Some(
                                                     "Failed to open SSH channel for nvidia-smi".to_string(),
                                                 );
                                             }
                                             if let Some(err) = smi_error {
                                                 if gpu_data.gpu_list.is_empty() {
                                                     if let Some(cached) = cached_snapshot {
                                                         if !cached.gpu_list.is_empty() {
                                                             gpu_data.gpu_list = cached.gpu_list;
                                                             gpu_data.slurm_nodelists =
                                                                 cached.slurm_nodelists.clone();
                                                             gpu_data.slurm_times =
                                                                 cached.slurm_times.clone();
                                                             gpu_data.slurm_steps =
                                                                 cached.slurm_steps.clone();
                                                             gpu_data.is_online = true;
                                                             gpu_data.error = Some(format!(
                                                                 "{} (showing cached job data)",
                                                                 err
                                                             ));
                                                             gpu_data.last_update = cached
                                                                 .last_update
                                                                 .clone()
                                                                 .or_else(|| {
                                                                     Some(Utc::now().format("%H:%M:%S").to_string())
                                                                 });
                                                         } else {
                                                             gpu_data.error = Some(err);
                                                         }
                                                     } else {
                                                         gpu_data.error = Some(err);
                                                     }
                                                 }
                                             }
                                         }
                                        
                                         if let Ok(mut data) = state_task.gpu_data.lock() {
                                              if s.use_slurm.unwrap_or(false) {
                                                  gpu_data.last_update = Some(Utc::now().format("%H:%M:%S").to_string());
                                                  gpu_data.slurm_nodelists = Some(nodelists.clone());
                                                  gpu_data.slurm_times = Some(times.clone());
                                                  gpu_data.slurm_steps = Some(steps.clone());
                                              }
                                              data.insert(s.host.clone(), gpu_data.clone());
                                          } else {
                                              log::error!("gpu_data lock poisoned in main worker for {}", s.host);
                                          }
                                          log::debug!("Main Worker for {} emitting update", s.host);
                                         let _ = app_task.emit("gpu_update", gpu_data);
                                         
                                         Ok((Some(sess), job_ids, nodelists, times, steps))
                                     }
                                 }
                             }).await;

                             match res {
                                 Ok(Ok((sess, jobs, nodelists, times, steps))) => {
                                     session = sess;
                                     slurm_job_ids = jobs;
                                     slurm_nodelists = nodelists;
                                     slurm_times = times;
                                     slurm_steps = steps;
                                     if (Utc::now() - last_squeue_update).num_seconds() >= 30 {
                                         last_squeue_update = Utc::now();
                                     }
                                 }
                                 _ => {
                                     log::warn!("Worker for {} failed or disconnected, retrying in 10s", server_inner.host);
                                     session = None;

                                     {
                                         let mut monitors = state_inner.active_monitors.lock().unwrap_or_else(|e| e.into_inner());
                                         let prefix = format!("{}:", server_inner.host);
                                         monitors.retain(|key, handle| {
                                             if key.starts_with(&prefix) {
                                                 handle.abort();
                                                 false
                                             } else {
                                                 true
                                             }
                                         });
                                     }

                                     let mut gpu_data = state_inner
                                         .gpu_data
                                         .lock()
                                         .ok()
                                         .and_then(|data| data.get(&server_inner.host).cloned())
                                         .unwrap_or_else(|| ServerGpuData {
                                             host: server_inner.host.clone(),
                                             is_online: false,
                                             gpu_list: vec![],
                                             error: None,
                                             last_update: None,
                                             slurm_steps: None,
                                             slurm_nodelists: None,
                                             slurm_times: None,
                                         });
                                     gpu_data.is_online = false;
                                     if gpu_data.gpu_list.is_empty() {
                                         gpu_data.error =
                                             Some("SSH connection failed or disconnected".to_string());
                                     } else {
                                         gpu_data.error = Some(
                                             "SSH connection failed (showing cached data)".to_string(),
                                         );
                                     }
                                     gpu_data.last_update = Some(Utc::now().format("%H:%M:%S").to_string());

                                     if let Ok(mut data) = state_inner.gpu_data.lock() {
                                         data.insert(server_inner.host.clone(), gpu_data.clone());
                                     }
                                     let _ = app_inner.emit("gpu_update", gpu_data);

                                     tokio::time::sleep(Duration::from_secs(10)).await;
                                 }
                             }

                            tokio::time::sleep(Duration::from_secs(update_interval)).await;
                        }
                    });
                    workers.insert(server_id, handle);
                }
            }
        } else {
            // Clear data when disabled
            if let Ok(mut data) = state.gpu_data.lock() {
                data.clear();
            }
            let _ = config_store::write_config(&app, GPU_CACHE_FILE, &Vec::<ServerGpuData>::new());
            let _ = app.emit("gpu_clear", ());
        }

        // Cleanup workers for removed servers
        {
            let mut workers = state.active_workers.lock().unwrap_or_else(|e| e.into_inner());
            workers.retain(|id, handle| {
                if !current_server_ids.contains(id) {
                    handle.abort();
                    // Also cleanup monitors for this host
                    let host = id.split(':').next().unwrap_or_default();
                    if !host.is_empty() {
                        let mut monitors = state.active_monitors.lock().unwrap_or_else(|e| e.into_inner());
                        let prefix = format!("{}:", host);
                        monitors.retain(|k, h| {
                            if k.starts_with(&prefix) {
                                h.abort();
                                false
                            } else { true }
                        });

                        // Clean up gpu_data entry
                        if let Ok(mut data) = state.gpu_data.lock() {
                            if data.remove(host).is_some() {
                                let _ = app.emit("gpu_prune", host.to_string());
                            }
                        }
                    }
                    false
                } else {
                    true
                }
            });
        }

        if last_cache_persist.elapsed() >= Duration::from_secs(30) {
            persist_gpu_data_cache(&app, state.as_ref());
            last_cache_persist = std::time::Instant::now();
        }

        tokio::time::sleep(Duration::from_secs(5)).await; // Re-check config every 5s
    }
}
