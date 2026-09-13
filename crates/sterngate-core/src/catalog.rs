use crate::error::{Result, SterngateError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogMetadata {
    pub generated_at: String,
    pub total_cbf_files: usize,
    pub unique_ecus: usize,
    pub unique_sha256_hashes: usize,
    pub exact_duplicate_groups: usize,
    pub redundant_file_copies: usize,
    #[serde(default)]
    pub processing_time_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CbfVersionInfo {
    pub sha256: String,
    pub date: String,
    pub iso_date: String,
    pub size_bytes: u64,
    pub protocol: String,
    #[serde(default)]
    pub tx_id: Option<String>,
    #[serde(default)]
    pub rx_id: Option<String>,
    #[serde(default)]
    pub func_id: Option<String>,
    pub presentation_count: usize,
    pub dtc_count: usize,
    pub primary_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CbfVersionHistoryEntry {
    pub sha256: String,
    pub size: u64,
    pub date: String,
    pub iso_date: String,
    pub protocol: String,
    #[serde(default)]
    pub gpd_version: Option<String>,
    #[serde(default)]
    pub tx_id: Option<String>,
    #[serde(default)]
    pub rx_id: Option<String>,
    #[serde(default)]
    pub func_id: Option<String>,
    pub presentation_count: usize,
    pub dtc_count: usize,
    pub occurrences: Vec<String>,
    pub chassis_list: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CbfEcuEntry {
    pub ecu_name: String,
    pub canonical_version: CbfVersionInfo,
    pub total_copies_in_cbf: usize,
    pub distinct_versions_count: usize,
    pub all_chassis_supported: Vec<String>,
    #[serde(default)]
    pub version_history: Vec<CbfVersionHistoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcuSearchResult {
    pub ecu_name: String,
    pub date: String,
    pub protocol: String,
    pub tx_id: Option<String>,
    pub rx_id: Option<String>,
    pub total_copies: usize,
    pub distinct_versions: usize,
    pub dtc_count: usize,
    pub primary_path: String,
    pub all_chassis_supported: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CbfCatalog {
    pub metadata: CatalogMetadata,
    pub ecus: BTreeMap<String, CbfEcuEntry>,
}

impl CbfCatalog {
    /// Load catalog from a specific file path
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to read CBF catalog at {}: {}",
                path_ref.display(),
                e
            ))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to parse CBF catalog JSON at {}: {}",
                path_ref.display(),
                e
            ))
        })
    }

    /// Load catalog using standard lookup heuristics
    pub fn load_default() -> Result<Self> {
        if let Ok(env_path) = std::env::var("STERNGATE_CBF_CATALOG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Self::load_from_path(p);
            }
        }

        let candidates = [
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
            "CBF catalog not found in standard paths (data/cbf_catalog.json)".into(),
        ))
    }

    /// Return metadata statistics
    pub fn stats(&self) -> &CatalogMetadata {
        &self.metadata
    }

    /// Retrieve an ECU by name (case-insensitive)
    pub fn get_ecu(&self, ecu: &str) -> Option<&CbfEcuEntry> {
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
                        date: info.canonical_version.date.clone(),
                        protocol: info.canonical_version.protocol.clone(),
                        tx_id: info.canonical_version.tx_id.clone(),
                        rx_id: info.canonical_version.rx_id.clone(),
                        total_copies: info.total_copies_in_cbf,
                        distinct_versions: info.distinct_versions_count,
                        dtc_count: info.canonical_version.dtc_count,
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
