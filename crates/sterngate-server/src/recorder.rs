use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use sterngate_core::TelemetrySnapshot;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlightRecorderStatus {
    pub is_recording: bool,
    pub current_file: Option<String>,
    pub records_count: usize,
    pub started_at_ms: Option<u64>,
    pub elapsed_seconds: u64,
}

pub struct FlightRecorder {
    is_recording: Arc<AtomicBool>,
    current_file: Arc<RwLock<Option<PathBuf>>>,
    records_count: Arc<AtomicUsize>,
    started_at_ms: Arc<RwLock<Option<u64>>>,
    file_writer: Arc<Mutex<Option<File>>>,
    output_dir: PathBuf,
}

impl FlightRecorder {
    pub fn new<P: AsRef<Path>>(output_dir: P) -> Self {
        Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            current_file: Arc::new(RwLock::new(None)),
            records_count: Arc::new(AtomicUsize::new(0)),
            started_at_ms: Arc::new(RwLock::new(None)),
            file_writer: Arc::new(Mutex::new(None)),
            output_dir: output_dir.as_ref().to_path_buf(),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Relaxed)
    }

    pub async fn start(
        &self,
        custom_name: Option<String>,
        mut telemetry_rx: broadcast::Receiver<TelemetrySnapshot>,
    ) -> Result<FlightRecorderStatus> {
        if self.is_recording.load(Ordering::Relaxed) {
            return Ok(self.status().await);
        }

        create_dir_all(&self.output_dir).context("Failed to create log directory")?;

        let filename = match custom_name {
            Some(name) => {
                let clean = name.trim();
                if clean.ends_with(".csv") {
                    clean.to_string()
                } else {
                    format!("{}.csv", clean)
                }
            }
            None => {
                let now = Utc::now();
                format!("flight_telemetry_{}.csv", now.format("%Y%m%d_%H%M%S"))
            }
        };

        let file_path = self.output_dir.join(&filename);
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .context(format!(
                "Failed to create log file: {}",
                file_path.display()
            ))?;

        // Write CSV standard header
        let header = "timestamp_ms,battery_voltage,engine_rpm,coolant_temp,trans_fluid_temp,rail_pressure,boost_pressure,tcc_slip_rpm,inj_cyl1,inj_cyl2,inj_cyl3,inj_cyl4\n";
        file.write_all(header.as_bytes())?;
        file.flush()?;

        *self.file_writer.lock().await = Some(file);
        *self.current_file.write().await = Some(file_path.clone());
        let start_ms = Utc::now().timestamp_millis() as u64;
        *self.started_at_ms.write().await = Some(start_ms);
        self.records_count.store(0, Ordering::Relaxed);
        self.is_recording.store(true, Ordering::Relaxed);

        info!(
            "Flight recorder started logging to: {}",
            file_path.display()
        );

        // Spawn background writer task consuming broadcast stream
        let is_rec = self.is_recording.clone();
        let writer_arc = self.file_writer.clone();
        let count_arc = self.records_count.clone();

        tokio::spawn(async move {
            while is_rec.load(Ordering::Relaxed) {
                match telemetry_rx.recv().await {
                    Ok(snap) => {
                        let row = format!(
                            "{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.2},{:.2},{:.2},{:.2}\n",
                            snap.timestamp_ms,
                            snap.battery_voltage,
                            snap.engine_rpm.unwrap_or(0.0),
                            snap.coolant_temp.unwrap_or(0.0),
                            snap.trans_fluid_temp.unwrap_or(0.0),
                            snap.rail_pressure.unwrap_or(0.0),
                            snap.boost_pressure.unwrap_or(0.0),
                            snap.tcc_slip_rpm.unwrap_or(0.0),
                            snap.inj_corr_cyl1.unwrap_or(0.0),
                            snap.inj_corr_cyl2.unwrap_or(0.0),
                            snap.inj_corr_cyl3.unwrap_or(0.0),
                            snap.inj_corr_cyl4.unwrap_or(0.0),
                        );

                        let mut writer_guard = writer_arc.lock().await;
                        if let Some(ref mut f) = *writer_guard {
                            if let Err(e) = f.write_all(row.as_bytes()) {
                                error!("Flight recorder disk write failed: {}", e);
                                break;
                            }
                            let _ = f.flush();
                            count_arc.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!("Flight recorder skipped {} lagging frames", skipped);
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        Ok(self.status().await)
    }

    pub async fn stop(&self) -> FlightRecorderStatus {
        self.is_recording.store(false, Ordering::Relaxed);
        let mut writer_guard = self.file_writer.lock().await;
        if let Some(mut f) = writer_guard.take() {
            let _ = f.flush();
        }
        info!("Flight recorder stopped.");
        self.status().await
    }

    pub async fn status(&self) -> FlightRecorderStatus {
        let is_rec = self.is_recording.load(Ordering::Relaxed);
        let current = self
            .current_file
            .read()
            .await
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        let count = self.records_count.load(Ordering::Relaxed);
        let started = *self.started_at_ms.read().await;
        let elapsed = if let Some(st) = started {
            let now = Utc::now().timestamp_millis() as u64;
            (now.saturating_sub(st)) / 1000
        } else {
            0
        };

        FlightRecorderStatus {
            is_recording: is_rec,
            current_file: current,
            records_count: count,
            started_at_ms: started,
            elapsed_seconds: elapsed,
        }
    }
}

impl Default for FlightRecorder {
    fn default() -> Self {
        Self::new("logs")
    }
}
