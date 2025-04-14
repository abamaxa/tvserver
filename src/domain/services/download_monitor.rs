use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use async_trait::async_trait;
use bytesize::ByteSize;
use tokio::fs;
use tokio::task;

use crate::domain::algorithm::{generate_display_name, skip_file};
use crate::domain::messages::{DownloadInfo, DownloadRequest, LocalMessageSender, TaskState};
use crate::domain::TaskType;
use crate::domain::traits::{DownloadProgressMonitor, Storer, TaskMonitor};

// Struct that contains all the mutable state
struct DownloadMonitorState {
    last_time: Instant,
    files: Vec<String>,
    bytes_downloaded: i64,
    download_rate: i64,
    bytes_uploaded: Option<i64>,
    upload_rate: Option<i64>,
    progress: Option<f32>,
    total_size: Option<i64>,
    remaining: Option<Duration>,
    finished: Option<SystemTime>,
    progress_details: String,
    error_message: String,
}

pub struct DownloadMonitor {
    request: DownloadRequest,
    downloader: DownloadProgressMonitor,
    state: Mutex<DownloadMonitorState>,
    done: Arc<tokio::sync::Notify>,
}

impl DownloadMonitor {
    pub fn new(request: DownloadRequest, downloader: DownloadProgressMonitor) -> Self {
        Self {
            request,
            downloader,
            state: Mutex::new(DownloadMonitorState {
                last_time: Instant::now(),
                files: Vec::new(),
                bytes_downloaded: 0,
                download_rate: 0,
                bytes_uploaded: None,
                upload_rate: None,
                progress: None,
                total_size: None,
                remaining: None,
                finished: None,
                progress_details: String::new(),
                error_message: String::new(),
            }),
            done: Arc::new(tokio::sync::Notify::new()),
        }
    }
    
    pub fn monitor(this: Arc<Self>, sender: LocalMessageSender) {
        let downloader = this.downloader.clone();
        let done = this.done.clone();

        tracing::info!("Starting download monitor of {}", this.request.link);
        
        task::spawn(async move {
            loop {
                tokio::select! {
                    info = downloader.observe() => {
                        let is_finished = this.update_state(&info);
                        let state = this.get_state().await;

                        tracing::info!("Current state: {} {}", state.display_name, state.size_details);
                        
                        // Send the state message
                        /*if sender.send(state).await.is_err() {
                            break;
                        }*/
                        
                        if info.finished || this.has_finished() || is_finished {
                            info.send_media_messages(&sender, skip_file, this.request.series.clone()).await;
                            break;
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            
            done.notify_one();
            tracing::info!("Download monitor of {} finished", this.request.link);
        });
    }
    
    fn humanize_bytes(bytes: i64) -> String {
        let bytes_u64: u64 = if bytes < 0 { 0 } else { bytes as u64 };
        ByteSize(bytes_u64).to_string()
    }

    fn update_state(&self, last_state: &DownloadInfo) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap();
        
        let elapsed = now.duration_since(state.last_time).as_millis() as i64;
        if elapsed == 0 {
            return false;
        }
        
        let bytes_downloaded = last_state.downloaded_size;
        let download_rate = 1000 * (bytes_downloaded - state.bytes_downloaded) / elapsed;
        
        let mut has_upload_info = false;
        let mut bytes_uploaded = 0;
        let mut upload_rate = 0;
        
        if let Some(uploaded_size) = last_state.uploaded_size {
            has_upload_info = true;
            
            let current_bytes_uploaded = if let Some(current) = state.bytes_uploaded {
                current
            } else {
                0
            };
            
            upload_rate = 1000 * (uploaded_size - current_bytes_uploaded) / elapsed;
            bytes_uploaded = uploaded_size;
        }
        
        let mut total_size = 0;
        let mut has_total_size = false;
        
        if let Some(size) = last_state.total_size {
            has_total_size = true;
            total_size = size;
        }
        
        // Calculate progress
        let mut progress = None;
        let mut remaining_time = Duration::from_secs(0);
        
        if has_total_size && total_size > 0 {
            let p = bytes_downloaded as f32 / total_size as f32;
            progress = Some(p);
            
            // Estimate remaining time
            if download_rate > 0 {
                remaining_time = Duration::from_secs(
                    ((total_size - bytes_downloaded) / download_rate) as u64
                );
            }
        }
        
        // Store the updated state
        state.last_time = now;
        state.bytes_downloaded = bytes_downloaded;
        state.download_rate = download_rate;
        
        if has_upload_info {
            state.bytes_uploaded = Some(bytes_uploaded);
            state.upload_rate = Some(upload_rate);
        }
        
        if has_total_size {
            state.total_size = Some(total_size);
        }
        
        if progress.is_some() {
            state.progress = progress;
            state.remaining = Some(remaining_time);
        }
        
        state.progress_details = last_state.progress_message.clone();
        if last_state.finished || 
           (has_total_size && bytes_downloaded == total_size && state.finished.is_none()) {
            let now = SystemTime::now();
            state.finished = Some(now);
            return true;
        }
        
        false
    }
    
    fn delete_files(&self) {
        let state = self.state.lock().unwrap();
        for full_path in &state.files {
            let full_path = full_path.clone();
            task::spawn(async move {
                let path = Path::new(&full_path);
                if path.exists() {
                    let _ = fs::remove_file(path).await;
                }
            });
        }
    }
}

#[async_trait]
impl TaskMonitor for DownloadMonitor {
    async fn get_state(&self) -> TaskState {
        let state = self.state.lock().unwrap();
        
        let size_details: String;
        let rate_details: String;
        let mut eta: i64 = 0;
        
        if let Some(total_size) = state.total_size {
            size_details = format!("{}/{}",
                Self::humanize_bytes(state.bytes_downloaded),
                Self::humanize_bytes(total_size)
            );
        } else {
            size_details = Self::humanize_bytes(state.bytes_downloaded);
        }
        
        if let Some(upload_rate) = state.upload_rate {
            rate_details = format!("{}/{}",
                Self::humanize_bytes(state.download_rate) + "/sec",
                Self::humanize_bytes(upload_rate) + "/sec"
            );
        } else {
            rate_details = Self::humanize_bytes(state.download_rate) + "/sec";
        }
        
        if let Some(remaining) = state.remaining {
            eta = remaining.as_secs() as i64;
        }
        
        TaskState {
            key: self.request.link.clone(),
            name: self.request.name.clone(),
            display_name: generate_display_name(&Some(self.request.name.clone())),
            finished: state.finished.is_some(),
            eta,
            percent_done: state.progress.unwrap_or(0.0),
            size_details,
            rate_details,
            process_details: state.progress_details.clone(),
            error_string: state.error_message.clone(),
            task_type: TaskType::Transmission,
        }
    }
    
    fn get_key(&self) -> String {
        self.request.link.clone()
    }
    
    fn get_seconds_since_finished(&self) -> i64 {
        let state = self.state.lock().unwrap();
        if state.finished.is_none() {
            return 0;
        }
        
        match state.finished.unwrap().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default();
                (now.as_secs() - duration.as_secs()) as i64
            }
            Err(_) => 0,
        }
    }
    
    fn terminate(&self) {
        self.downloader.terminate();
        
        // Set the finished timestamp
        {
            let mut state = self.state.lock().unwrap();
            let now = SystemTime::now();
            state.finished = Some(now);
        }
        
        self.delete_files();
    }
    
    fn has_finished(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.finished.is_some()
    }
    
    async fn cleanup(&self, _store: &Storer, force_delete: bool) -> anyhow::Result<()> {
        if force_delete {
            self.delete_files();
        }
        
        Ok(())
    }
}

impl ToString for DownloadMonitor {
    fn to_string(&self) -> String {
        self.request.link.clone()
    }
}
/* 
pub async fn wait(monitor: &DownloadMonitor) {
    monitor.done.notified().await;
}
*/