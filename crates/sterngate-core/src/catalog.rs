use crate::error::{Result, SterngateError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogMetadata {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub total_ecus: usize,
    #[serde(default)]
    pub total_cbf_files: usize,
    #[serde(default)]
    pub unique_ecus: usize,
    #[serde(default)]
    pub redundant_file_copies: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct EcuVersionInfo {
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub tx_id: Option<String>,
    #[serde(default)]
    pub rx_id: Option<String>,
    #[serde(default)]
    pub func_id: Option<String>,
    #[serde(default)]
    pub dtc_count: usize,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub presentation_count: usize,
    #[serde(default)]
    pub primary_path: String,
}

pub type CbfVersionInfo = EcuVersionInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcuCatalogEntry {
    pub ecu_name: String,
    pub protocol: String,
    #[serde(default)]
    pub tx_id: Option<String>,
    #[serde(default)]
    pub rx_id: Option<String>,
    #[serde(default)]
    pub func_id: Option<String>,
    #[serde(default)]
    pub dtc_count: usize,
    #[serde(default)]
    pub chassis: Vec<String>,
    #[serde(default)]
    pub canonical_version: EcuVersionInfo,
    #[serde(default)]
    pub all_chassis_supported: Vec<String>,
    #[serde(default)]
    pub total_copies_in_cbf: usize,
    #[serde(default)]
    pub distinct_versions_count: usize,
}

pub type CbfEcuEntry = EcuCatalogEntry;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcuSearchResult {
    pub ecu_name: String,
    pub protocol: String,
    pub tx_id: Option<String>,
    pub rx_id: Option<String>,
    pub func_id: Option<String>,
    pub dtc_count: usize,
    pub chassis: Vec<String>,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub total_copies: usize,
    #[serde(default)]
    pub distinct_versions: usize,
    #[serde(default)]
    pub primary_path: String,
    #[serde(default)]
    pub all_chassis_supported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcuCatalog {
    pub metadata: CatalogMetadata,
    pub ecus: BTreeMap<String, EcuCatalogEntry>,
}

pub type CbfCatalog = EcuCatalog;

impl EcuCatalog {
    /// Load catalog from a specific file path
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to read ECU catalog at {}: {}",
                path_ref.display(),
                e
            ))
        })?;
        let mut cat: Self = serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to parse ECU catalog JSON at {}: {}",
                path_ref.display(),
                e
            ))
        })?;

        // Harmonize fields for 100% backward compatibility
        for entry in cat.ecus.values_mut() {
            if entry.all_chassis_supported.is_empty() {
                entry.all_chassis_supported = entry.chassis.clone();
            }
            if entry.chassis.is_empty() {
                entry.chassis = entry.all_chassis_supported.clone();
            }
            if entry.distinct_versions_count == 0 {
                entry.distinct_versions_count = 1;
            }
            if entry.total_copies_in_cbf == 0 {
                entry.total_copies_in_cbf = 1;
            }
            if entry.canonical_version.protocol.is_empty() {
                entry.canonical_version.protocol = entry.protocol.clone();
                entry.canonical_version.tx_id = entry.tx_id.clone();
                entry.canonical_version.rx_id = entry.rx_id.clone();
                entry.canonical_version.func_id = entry.func_id.clone();
                entry.canonical_version.dtc_count = entry.dtc_count;
            }
        }
        Ok(cat)
    }

    /// Load catalog using standard lookup heuristics
    pub fn load_default() -> Result<Self> {
        if let Ok(env_path) = std::env::var("STERNGATE_ECU_CATALOG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Self::load_from_path(p);
            }
        }
        if let Ok(env_path) = std::env::var("STERNGATE_CBF_CATALOG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Self::load_from_path(p);
            }
        }

        let candidates = [
            Path::new("data/ecu_catalog.json"),
            Path::new("../../data/ecu_catalog.json"),
            Path::new("../data/ecu_catalog.json"),
            Path::new("data/cbf_catalog.json"),
            Path::new("../../data/cbf_catalog.json"),
            Path::new("../data/cbf_catalog.json"),
        ];

        for &candidate in &candidates {
            if candidate.exists() {
                return Self::load_from_path(candidate);
            }
        }

        Err(SterngateError::ProfileError(
            "ECU catalog not found in standard paths (data/ecu_catalog.json)".into(),
        ))
    }

    /// Return metadata statistics
    pub fn stats(&self) -> &CatalogMetadata {
        &self.metadata
    }

    /// Retrieve an ECU by name (case-insensitive)
    pub fn get_ecu(&self, ecu: &str) -> Option<&EcuCatalogEntry> {
        let upper = ecu.to_uppercase();
        self.ecus.get(&upper)
    }

    /// Search ECUs by name or chassis keyword
    pub fn search(&self, query: &str, limit: usize) -> Vec<EcuSearchResult> {
        let q = query.trim().to_uppercase();

        let mut results: Vec<EcuSearchResult> = self
            .ecus
            .iter()
            .filter_map(|(name, info)| {
                let matches_name = q.is_empty() || name.contains(&q);
                let matches_chassis = !q.is_empty()
                    && info
                        .all_chassis_supported
                        .iter()
                        .any(|c| c.to_uppercase().contains(&q));

                if matches_name || matches_chassis {
                    Some(EcuSearchResult {
                        ecu_name: info.ecu_name.clone(),
                        protocol: info.protocol.clone(),
                        tx_id: info.tx_id.clone(),
                        rx_id: info.rx_id.clone(),
                        func_id: info.func_id.clone(),
                        dtc_count: info.dtc_count,
                        chassis: info.chassis.clone(),
                        date: info.canonical_version.date.clone(),
                        total_copies: info.total_copies_in_cbf,
                        distinct_versions: info.distinct_versions_count,
                        primary_path: info.canonical_version.primary_path.clone(),
                        all_chassis_supported: info.all_chassis_supported.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Sort by relevance: exact match first, then prefix, then by name
        results.sort_by(|a, b| {
            let a_exact = a.ecu_name == q;
            let b_exact = b.ecu_name == q;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }
            let a_starts = a.ecu_name.starts_with(&q);
            let b_starts = b.ecu_name.starts_with(&q);
            if a_starts != b_starts {
                return b_starts.cmp(&a_starts);
            }
            a.ecu_name.cmp(&b.ecu_name)
        });

        if limit > 0 && results.len() > limit {
            results.truncate(limit);
        }

        results
    }
}
