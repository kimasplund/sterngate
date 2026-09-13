use crate::error::{Result, SterngateError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Parsed and decoded automotive VIN
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecodedVin {
    pub raw_vin: String,
    pub manufacturer: String,
    pub model_series: String,
    pub body_style: String,
    pub model_name: String,
    pub engine: String,
    pub plant_code: Option<char>,
    pub serial_number: String,
}

impl DecodedVin {
    /// Parse and decode standard 17-character VIN (with special Daimler / Mercedes-Benz heuristics)
    pub fn decode(raw: &str) -> Self {
        let clean = raw.trim().to_uppercase();
        if clean.len() < 17 {
            return Self {
                raw_vin: clean.clone(),
                manufacturer: "Unknown / Non-standard".into(),
                model_series: "Generic".into(),
                body_style: "Generic".into(),
                model_name: "Vehicle".into(),
                engine: "Unspecified".into(),
                plant_code: None,
                serial_number: clean,
            };
        }

        let wmi = &clean[0..3];
        let vds = &clean[3..9];
        let plant = clean.chars().nth(10);
        let serial = clean[11..17].to_string();

        let (manufacturer, model_series, body_style, model_name, engine) = match wmi {
            "WDB" | "WDC" | "WDD" | "WDZ" | "4JG" => {
                let mfg = "Mercedes-Benz".to_string();
                let chassis_digits = &vds[0..3];
                match chassis_digits {
                    "211" => {
                        let body_char = vds.chars().nth(3).unwrap_or('0');
                        let body = match body_char {
                            '2' => "Estate / T-Modell (S211)",
                            '0' => "Sedan / Saloon (W211)",
                            '6' => "Long Wheelbase (V211)",
                            _ => "W211 / S211 Family",
                        };
                        let engine_digits = &vds[4..6];
                        let (name, eng) = match engine_digits {
                            "06" => (
                                if body_char == '2' {
                                    "E 220 T CDI"
                                } else {
                                    "E 220 CDI"
                                },
                                "OM646 2.2L CDI (150 hp, EDC16)",
                            ),
                            "08" => (
                                if body_char == '2' {
                                    "E 220 T CDI EVO"
                                } else {
                                    "E 220 CDI EVO"
                                },
                                "OM646 EVO 2.2L CDI (170 hp, EDC16CP31)",
                            ),
                            "04" => (
                                if body_char == '2' {
                                    "E 200 T CDI"
                                } else {
                                    "E 200 CDI"
                                },
                                "OM646 2.2L CDI (122/136 hp)",
                            ),
                            "16" => (
                                if body_char == '2' {
                                    "E 270 T CDI"
                                } else {
                                    "E 270 CDI"
                                },
                                "OM647 2.7L I5 CDI (177 hp)",
                            ),
                            "26" => (
                                if body_char == '2' {
                                    "E 320 T CDI"
                                } else {
                                    "E 320 CDI"
                                },
                                "OM648 3.2L I6 CDI (204 hp)",
                            ),
                            "22" => (
                                if body_char == '2' {
                                    "E 320 T CDI V6"
                                } else {
                                    "E 320 CDI V6"
                                },
                                "OM642 3.0L V6 CDI (224 hp, CR4)",
                            ),
                            "65" => (
                                if body_char == '2' { "E 320 T" } else { "E 320" },
                                "M112 3.2L V6 (224 hp, ME2.8)",
                            ),
                            "70" => (
                                if body_char == '2' { "E 500 T" } else { "E 500" },
                                "M113 5.0L V8 (306 hp, ME2.8)",
                            ),
                            "72" => (
                                if body_char == '2' { "E 500 T" } else { "E 500" },
                                "M273 5.5L V8 (388 hp, ME9.7)",
                            ),
                            _ => (
                                if body_char == '2' {
                                    "E-Class Estate"
                                } else {
                                    "E-Class Sedan"
                                },
                                "Mercedes-Benz Powertrain",
                            ),
                        };
                        (
                            mfg,
                            "S211 / W211 (E-Class)".into(),
                            body.into(),
                            name.into(),
                            eng.into(),
                        )
                    }
                    "204" => (
                        mfg,
                        "W204 / S204 (C-Class)".into(),
                        "C-Class".into(),
                        "C-Class".into(),
                        "OM651 / M272".into(),
                    ),
                    "221" => (
                        mfg,
                        "W221 (S-Class)".into(),
                        "S-Class Sedan".into(),
                        "S-Class".into(),
                        "M273 / OM642".into(),
                    ),
                    "203" => (
                        mfg,
                        "W203 / S203 (C-Class)".into(),
                        "C-Class".into(),
                        "C-Class".into(),
                        "OM646 / M111 / M271".into(),
                    ),
                    "164" => (
                        mfg,
                        "W164 (ML-Class)".into(),
                        "ML SUV".into(),
                        "ML-Class".into(),
                        "OM642 / M272 / M273".into(),
                    ),
                    _ => (
                        mfg,
                        format!("Chassis {}", chassis_digits),
                        "Passenger Car".into(),
                        "Mercedes-Benz".into(),
                        "Engine Unspecified".into(),
                    ),
                }
            }
            "WVW" | "WAU" => (
                "Volkswagen Group".into(),
                "VAG Platform".into(),
                "Passenger Car".into(),
                "VAG Vehicle".into(),
                "TDI / TSI / TFSI".into(),
            ),
            "WBA" | "WBS" => (
                "BMW AG".into(),
                "BMW Platform".into(),
                "Passenger Car".into(),
                "BMW Vehicle".into(),
                "BMW Powertrain".into(),
            ),
            _ => (
                "Vehicle Manufacturer".into(),
                "Automotive Architecture".into(),
                "Standard Body".into(),
                "Automotive Vehicle".into(),
                "Internal Combustion / Hybrid".into(),
            ),
        };

        Self {
            raw_vin: clean,
            manufacturer,
            model_series,
            body_style,
            model_name,
            engine,
            plant_code: plant,
            serial_number: serial,
        }
    }
}

/// Snapshot of an individual ECU detected during vehicle interrogation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VehicleEcuSnapshot {
    pub module_name: String,
    #[serde(default)]
    pub part_number: Option<String>,
    #[serde(default)]
    pub hardware_version: Option<String>,
    #[serde(default)]
    pub software_version: Option<String>,
    #[serde(default)]
    pub calibration_id: Option<String>,
    #[serde(default)]
    pub serial_number: Option<String>,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub can_tx_id: Option<String>,
    #[serde(default)]
    pub can_rx_id: Option<String>,
    #[serde(default)]
    pub dtc_count: usize,
}

/// Historical record of a vehicle scanned by Sterngate
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VehicleRecord {
    pub vin: String,
    pub decoded: DecodedVin,
    pub first_scanned: String,
    pub last_scanned: String,
    pub scan_count: usize,
    #[serde(default)]
    pub odometer_km: Option<u32>,
    #[serde(default)]
    pub battery_voltage: Option<f64>,
    #[serde(default)]
    pub detected_modules: BTreeMap<String, VehicleEcuSnapshot>,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Git commit metadata for parameter and coding history
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitCommitInfo {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Garage manager handling persistence and Git versioning per VIN
#[derive(Debug, Clone)]
pub struct VehicleGarage {
    base_dir: PathBuf,
}

impl Default for VehicleGarage {
    fn default() -> Self {
        Self::new(Self::default_path())
    }
}

impl VehicleGarage {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    /// Default garage directory path (`data/vehicles`)
    pub fn default_path() -> PathBuf {
        if let Ok(env_path) = std::env::var("STERNGATE_GARAGE_DIR") {
            PathBuf::from(env_path)
        } else {
            let candidates = ["data/vehicles", "../../data/vehicles", "../data/vehicles"];
            for &c in &candidates {
                let p = Path::new(c);
                if p.exists() {
                    return p.to_path_buf();
                }
            }
            PathBuf::from("data/vehicles")
        }
    }

    /// Get directory path for a specific VIN
    pub fn get_vehicle_dir(&self, vin: &str) -> PathBuf {
        let clean_vin = vin.trim().to_uppercase();
        self.base_dir.join(&clean_vin)
    }

    /// List all vehicles currently saved in the garage
    pub fn list_vehicles(&self) -> Result<Vec<VehicleRecord>> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut vehicles = Vec::new();
        let entries = std::fs::read_dir(&self.base_dir).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to read garage directory: {}", e))
        })?;

        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let rec_path = entry.path().join("vehicle.json");
                if rec_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&rec_path) {
                        if let Ok(rec) = serde_json::from_str::<VehicleRecord>(&content) {
                            vehicles.push(rec);
                        }
                    }
                }
            }
        }

        vehicles.sort_by(|a, b| b.last_scanned.cmp(&a.last_scanned));
        Ok(vehicles)
    }

    /// Load a specific vehicle by VIN
    pub fn load_vehicle(&self, vin: &str) -> Result<Option<VehicleRecord>> {
        let vdir = self.get_vehicle_dir(vin);
        let rec_path = vdir.join("vehicle.json");
        if !rec_path.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&rec_path).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to read vehicle record for {}: {}",
                vin, e
            ))
        })?;

        let rec: VehicleRecord = serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to parse vehicle record for {}: {}",
                vin, e
            ))
        })?;

        Ok(Some(rec))
    }

    /// Initialize Git repository for a vehicle directory if not already initialized
    fn init_git_repo_if_needed(&self, vdir: &Path) -> Result<()> {
        let git_dir = vdir.join(".git");
        if !git_dir.exists() {
            let status = Command::new("git")
                .arg("init")
                .current_dir(vdir)
                .status()
                .map_err(|e| SterngateError::ProfileError(format!("Git init failed: {}", e)))?;
            if !status.success() {
                return Err(SterngateError::ProfileError(
                    "git init returned non-zero status".into(),
                ));
            }

            // Configure local git user if not globally set
            let _ = Command::new("git")
                .args(["config", "user.name", "Sterngate Diagnostic Daemon"])
                .current_dir(vdir)
                .status();
            let _ = Command::new("git")
                .args(["config", "user.email", "daemon@sterngate.local"])
                .current_dir(vdir)
                .status();
        }
        Ok(())
    }

    /// Save or update a vehicle record and commit the state to Git
    pub fn save_vehicle(
        &self,
        record: &VehicleRecord,
        commit_message: Option<&str>,
    ) -> Result<PathBuf> {
        let vdir = self.get_vehicle_dir(&record.vin);
        std::fs::create_dir_all(&vdir).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to create vehicle directory: {}", e))
        })?;

        // Create subdirectories for coding and history
        let coding_dir = vdir.join("coding");
        let history_dir = vdir.join("history");
        std::fs::create_dir_all(&coding_dir).ok();
        std::fs::create_dir_all(&history_dir).ok();

        // Save vehicle.json
        let rec_path = vdir.join("vehicle.json");
        let json_str = serde_json::to_string_pretty(record).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to serialize vehicle record: {}", e))
        })?;
        std::fs::write(&rec_path, json_str).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to write vehicle.json: {}", e))
        })?;

        // Initialize Git repo & commit
        if let Err(e) = self.init_git_repo_if_needed(&vdir) {
            eprintln!(
                "Warning: Could not initialize git for vehicle {}: {}",
                record.vin, e
            );
        } else {
            let msg = commit_message.unwrap_or("diagnostic_scan: sync vehicle record");
            let _ = Command::new("git")
                .args(["add", "."])
                .current_dir(&vdir)
                .status();
            let _ = Command::new("git")
                .args(["commit", "-m", msg, "--allow-empty"])
                .current_dir(&vdir)
                .status();
        }

        Ok(rec_path)
    }

    /// Save variant coding hex and JSON representation for an ECU, committing to Git
    pub fn save_coding(
        &self,
        vin: &str,
        module: &str,
        raw_hex: &str,
        decoded_json: Option<&serde_json::Value>,
        commit_msg: &str,
    ) -> Result<()> {
        let vdir = self.get_vehicle_dir(vin);
        let coding_dir = vdir.join("coding");
        std::fs::create_dir_all(&coding_dir).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to create coding directory: {}", e))
        })?;

        // Write raw hex
        let hex_path = coding_dir.join(format!("{}.coding.hex", module));
        std::fs::write(&hex_path, raw_hex.trim()).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to write coding hex: {}", e))
        })?;

        // Write decoded JSON if present
        if let Some(json_val) = decoded_json {
            let json_path = coding_dir.join(format!("{}.coding.json", module));
            let s = serde_json::to_string_pretty(json_val).unwrap_or_default();
            std::fs::write(&json_path, s).ok();
        }

        // Commit to Git
        self.init_git_repo_if_needed(&vdir)?;
        let _ = Command::new("git")
            .args(["add", "coding/"])
            .current_dir(&vdir)
            .status();
        let _ = Command::new("git")
            .args(["commit", "-m", commit_msg])
            .current_dir(&vdir)
            .status();

        Ok(())
    }

    /// Get Git commit history for a vehicle
    pub fn get_history(&self, vin: &str) -> Result<Vec<GitCommitInfo>> {
        let vdir = self.get_vehicle_dir(vin);
        if !vdir.join(".git").exists() {
            return Ok(Vec::new());
        }

        let output = Command::new("git")
            .args(["log", "--pretty=format:%H|%an|%ad|%s", "--date=iso"])
            .current_dir(&vdir)
            .output()
            .map_err(|e| {
                SterngateError::ProfileError(format!("Failed to execute git log: {}", e))
            })?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut commits = Vec::new();
        for line in text.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 4 {
                commits.push(GitCommitInfo {
                    hash: parts[0].to_string(),
                    author: parts[1].to_string(),
                    date: parts[2].to_string(),
                    message: parts[3].to_string(),
                });
            }
        }

        Ok(commits)
    }

    /// Roll back vehicle configuration to a previous Git commit
    pub fn rollback(&self, vin: &str, commit_hash: &str) -> Result<()> {
        let vdir = self.get_vehicle_dir(vin);
        if !vdir.join(".git").exists() {
            return Err(SterngateError::ProfileError(
                "No git history found for vehicle".into(),
            ));
        }

        let status = Command::new("git")
            .args(["checkout", commit_hash, "--", "coding/", "vehicle.json"])
            .current_dir(&vdir)
            .status()
            .map_err(|e| {
                SterngateError::ProfileError(format!("Failed to rollback commit: {}", e))
            })?;

        if !status.success() {
            return Err(SterngateError::ProfileError(format!(
                "Failed to checkout commit {}",
                commit_hash
            )));
        }

        let msg = format!(
            "rollback: restored configuration from commit {}",
            &commit_hash[0..8.min(commit_hash.len())]
        );
        let _ = Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(&vdir)
            .status();

        Ok(())
    }
}
