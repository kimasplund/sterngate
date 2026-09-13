use crate::error::{Result, SterngateError};
use crate::parameter::ParameterValue;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScalingDef {
    pub slope: f64,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub names: HashMap<String, String>,
    pub module: String,
    pub service: u8,
    pub did: String,
    pub byte_offset: usize,
    pub length: usize,
    pub scaling: ScalingDef,
    pub unit: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

impl ParameterDef {
    pub fn localized_name(&self, lang: crate::i18n::Language) -> &str {
        if let Some(loc) = self.names.get(&lang.to_string()) {
            loc.as_str()
        } else {
            &self.name
        }
    }
    pub fn parse_raw(&self, payload: &[u8]) -> Result<ParameterValue> {
        let did_u16 = u16::from_str_radix(self.did.trim_start_matches("0x"), 16).map_err(|e| {
            SterngateError::ParameterParseError {
                name: self.id.clone(),
                reason: format!("Invalid hex DID {}: {}", self.did, e),
            }
        })?;

        let (data_slice, start_offset) = if payload.len() >= 3 && payload[0] == 0x62 {
            let resp_did = u16::from_be_bytes([payload[1], payload[2]]);
            if resp_did != did_u16 {
                return Err(SterngateError::ParameterParseError {
                    name: self.id.clone(),
                    reason: format!(
                        "Mismatched DID response: expected 0x{:04X}, got 0x{:04X}",
                        did_u16, resp_did
                    ),
                });
            }
            (&payload[3..], 0)
        } else if payload.len() >= 2 && payload[0] == 0x61 {
            (&payload[2..], 0)
        } else {
            (payload, self.byte_offset)
        };

        if start_offset + self.length > data_slice.len() {
            return Err(SterngateError::ParameterParseError {
                name: self.id.clone(),
                reason: format!(
                    "Payload too short for length {}: available {}",
                    self.length,
                    data_slice.len()
                ),
            });
        }

        let slice = &data_slice[start_offset..start_offset + self.length];
        let raw_val: f64 = match self.length {
            1 => slice[0] as f64,
            2 => u16::from_be_bytes([slice[0], slice[1]]) as f64,
            4 => u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]) as f64,
            _ => {
                return Err(SterngateError::ParameterParseError {
                    name: self.id.clone(),
                    reason: format!("Unsupported byte length: {}", self.length),
                });
            }
        };

        let physical_val = (raw_val * self.scaling.slope) + self.scaling.offset;
        let raw_hex = slice
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ");

        Ok(ParameterValue {
            id: self.id.clone(),
            name: self.name.clone(),
            module: self.module.clone(),
            value: (physical_val * 100.0).round() / 100.0,
            raw_hex,
            unit: self.unit.clone(),
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleDef {
    pub name: String,
    #[serde(default)]
    pub names: HashMap<String, String>,
    pub tx_id: String,
    pub rx_id: String,
    pub protocol: String,
    #[serde(default)]
    pub seed_key_algo: Option<String>,
}

impl ModuleDef {
    pub fn localized_name(&self, lang: crate::i18n::Language) -> &str {
        if let Some(loc) = self.names.get(&lang.to_string()) {
            loc.as_str()
        } else {
            &self.name
        }
    }

    pub fn tx_can_id(&self) -> Result<u32> {
        u32::from_str_radix(self.tx_id.trim_start_matches("0x"), 16).map_err(|e| {
            SterngateError::ProfileError(format!("Invalid tx_id {}: {}", self.tx_id, e))
        })
    }

    pub fn rx_can_id(&self) -> Result<u32> {
        u32::from_str_radix(self.rx_id.trim_start_matches("0x"), 16).map_err(|e| {
            SterngateError::ProfileError(format!("Invalid rx_id {}: {}", self.rx_id, e))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VehicleProfile {
    pub profile_name: String,
    pub oem: String,
    pub chassis: String,
    #[serde(default)]
    pub gateway_type: Option<String>,
    #[serde(default = "default_bitrate")]
    pub default_bitrate: u32,
    pub modules: HashMap<String, ModuleDef>,
    pub parameters: Vec<ParameterDef>,
}

fn default_bitrate() -> u32 {
    500_000
}

impl VehicleProfile {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = File::open(&path).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to open profile {}: {}",
                path.as_ref().display(),
                e
            ))
        })?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to read profile content: {}", e))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!("JSON syntax error in profile: {}", e))
        })
    }

    pub fn get_module(&self, name: &str) -> Option<&ModuleDef> {
        self.modules.get(name)
    }

    pub fn find_parameter(&self, id: &str) -> Option<&ParameterDef> {
        self.parameters
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(id) || p.did.eq_ignore_ascii_case(id))
    }

    /// Recursively find all JSON vehicle profile paths in a directory (ignoring schema files)
    pub fn discover_paths<P: AsRef<Path>>(dir: P) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        Self::collect_paths_recursive(dir.as_ref(), &mut paths);
        paths.sort();
        paths
    }

    fn collect_paths_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::collect_paths_recursive(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "json")
                    && !path.to_string_lossy().contains("schema")
                {
                    out.push(path);
                }
            }
        }
    }

    /// Discover and load all valid vehicle profiles in a directory
    pub fn discover<P: AsRef<Path>>(dir: P) -> Vec<Self> {
        Self::discover_paths(dir)
            .into_iter()
            .filter_map(|p| Self::load_from_file(p).ok())
            .collect()
    }

    /// In-place localization of parameter and module names
    pub fn localize(&mut self, lang: crate::i18n::Language) {
        let lang_code = lang.to_string();
        for param in &mut self.parameters {
            if let Some(loc) = param.names.get(&lang_code) {
                param.name = loc.clone();
            }
        }
        for module in self.modules.values_mut() {
            if let Some(loc) = module.names.get(&lang_code) {
                module.name = loc.clone();
            }
        }
    }
}
