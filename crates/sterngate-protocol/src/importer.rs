use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sterngate_core::{
    EcuCatalog, ModuleDef, ParameterDef, Result, ScalingDef, SterngateError, VehicleProfile,
};
use tracing::info;

/// Summary report of an automated profile import run
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ImportReport {
    pub total_files_scanned: usize,
    pub cbf_files_found: usize,
    pub smrd_files_found: usize,
    pub profiles_generated: Vec<String>,
    pub warnings: Vec<String>,
}

/// Automated importer for Daimler CBF and SMR-D diagnostic databases
pub struct ProfileImporter;

impl ProfileImporter {
    /// Scan a directory or single file and import into Sterngate JSON profiles
    pub fn import_from_path(
        input_path: impl AsRef<Path>,
        output_dir: impl AsRef<Path>,
    ) -> Result<ImportReport> {
        let input = input_path.as_ref();
        let output = output_dir.as_ref();

        if !input.exists() {
            return Err(SterngateError::ProfileError(format!(
                "Input path does not exist: {}",
                input.display()
            )));
        }

        std::fs::create_dir_all(output).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Cannot create output dir {}: {}",
                output.display(),
                e
            ))
        })?;

        let catalog = EcuCatalog::load_default().ok();

        let mut cbf_files = Vec::new();
        let mut smrd_files = Vec::new();
        let mut total_scanned = 0;

        Self::collect_files(input, &mut cbf_files, &mut smrd_files, &mut total_scanned);

        let mut generated = Vec::new();
        let mut warnings = Vec::new();

        // Process CBF files
        for cbf_path in &cbf_files {
            match Self::process_cbf_file(cbf_path, output, catalog.as_ref()) {
                Ok(Some(name)) => {
                    info!("Successfully generated profile from CBF: {}", name);
                    generated.push(name);
                }
                Ok(None) => {}
                Err(e) => {
                    warnings.push(format!("{}: {}", cbf_path.display(), e));
                }
            }
        }

        // Process SMR-D files
        for smrd_path in &smrd_files {
            match Self::process_smrd_file(smrd_path, output, catalog.as_ref()) {
                Ok(Some(name)) => {
                    info!("Successfully generated profile from SMR-D: {}", name);
                    generated.push(name);
                }
                Ok(None) => {}
                Err(e) => {
                    warnings.push(format!("{}: {}", smrd_path.display(), e));
                }
            }
        }

        Ok(ImportReport {
            total_files_scanned: total_scanned,
            cbf_files_found: cbf_files.len(),
            smrd_files_found: smrd_files.len(),
            profiles_generated: generated,
            warnings,
        })
    }

    fn collect_files(
        dir: &Path,
        cbf_files: &mut Vec<PathBuf>,
        smrd_files: &mut Vec<PathBuf>,
        total: &mut usize,
    ) {
        if dir.is_file() {
            *total += 1;
            let ext = dir
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "cbf" {
                cbf_files.push(dir.to_path_buf());
            } else if ext == "smr-d" || ext == "smrd" || ext == "odx" {
                smrd_files.push(dir.to_path_buf());
            }
            return;
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    Self::collect_files(&p, cbf_files, smrd_files, total);
                } else if p.is_file() {
                    *total += 1;
                    let ext = p
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if ext == "cbf" {
                        cbf_files.push(p);
                    } else if ext == "smr-d" || ext == "smrd" || ext == "odx" {
                        smrd_files.push(p);
                    }
                }
            }
        }
    }

    /// Process a legacy Caesar Binary File (.cbf)
    pub fn process_cbf_file(
        path: &Path,
        output_dir: &Path,
        catalog: Option<&EcuCatalog>,
    ) -> Result<Option<String>> {
        let data = std::fs::read(path).map_err(|e| {
            SterngateError::ProfileError(format!("Failed reading CBF {}: {}", path.display(), e))
        })?;

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNKNOWN")
            .to_uppercase();

        let catalog_match = catalog.and_then(|c| c.get_ecu(&stem));

        let primary_chassis = catalog_match
            .and_then(|m| m.chassis.first().cloned())
            .unwrap_or_else(|| "W211".into());

        let (tx_id, rx_id) = if let Some(m) = catalog_match {
            (
                m.tx_id.clone().unwrap_or_else(|| "0x7E0".into()),
                m.rx_id.clone().unwrap_or_else(|| "0x7E8".into()),
            )
        } else {
            ("0x7E0".into(), "0x7E8".into())
        };

        let mut modules = HashMap::new();
        modules.insert(
            stem.clone(),
            ModuleDef {
                name: format!("{} Diagnostic Module", stem),
                names: HashMap::new(),
                tx_id: tx_id.clone(),
                rx_id: rx_id.clone(),
                protocol: "ISO-14229".into(),
                seed_key_algo: Some("DaimlerStandardLevel01".into()),
            },
        );

        // Extract basic standard DIDs
        let mut parameters = Vec::new();
        parameters.push(ParameterDef {
            id: format!("{}_SUPPLIER_HW", stem.to_lowercase()),
            name: "Supplier Hardware Number".into(),
            names: HashMap::new(),
            module: stem.clone(),
            service: 0x22,
            did: "0xF192".into(),
            byte_offset: 0,
            length: 4,
            scaling: ScalingDef {
                slope: 1.0,
                offset: 0.0,
            },
            unit: "".into(),
            min: None,
            max: None,
        });
        parameters.push(ParameterDef {
            id: format!("{}_SUPPLIER_SW", stem.to_lowercase()),
            name: "Supplier Software Version".into(),
            names: HashMap::new(),
            module: stem.clone(),
            service: 0x22,
            did: "0xF194".into(),
            byte_offset: 0,
            length: 4,
            scaling: ScalingDef {
                slope: 1.0,
                offset: 0.0,
            },
            unit: "".into(),
            min: None,
            max: None,
        });

        // Scan CBF binary for custom DID markers if present
        let mut i = 0;
        while i + 4 <= data.len().min(16384) {
            if data[i] == 0x22 && data[i + 1] == 0x01 {
                let did_hex = format!("0x{:02X}{:02X}", data[i + 1], data[i + 2]);
                parameters.push(ParameterDef {
                    id: format!(
                        "{}_did_{:02x}{:02x}",
                        stem.to_lowercase(),
                        data[i + 1],
                        data[i + 2]
                    ),
                    name: format!("Custom Diagnostic DID {}", did_hex),
                    names: HashMap::new(),
                    module: stem.clone(),
                    service: 0x22,
                    did: did_hex,
                    byte_offset: 0,
                    length: 2,
                    scaling: ScalingDef {
                        slope: 1.0,
                        offset: 0.0,
                    },
                    unit: "".into(),
                    min: None,
                    max: None,
                });
                i += 3;
            } else {
                i += 1;
            }
        }

        let sanitized_chassis = primary_chassis.replace(['/', '\\'], "_");
        let profile_name = format!(
            "{}_{}",
            sanitized_chassis.to_lowercase(),
            stem.to_lowercase()
        );
        let profile = VehicleProfile {
            profile_name: profile_name.clone(),
            oem: "Mercedes-Benz".into(),
            chassis: primary_chassis,
            gateway_type: Some("Central Gateway (CGW)".into()),
            default_bitrate: 500_000,
            modules,
            parameters,
        };

        let out_file = output_dir.join(format!("{}.json", profile_name));
        let json = serde_json::to_string_pretty(&profile).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed serializing profile {}: {}",
                profile_name, e
            ))
        })?;
        std::fs::write(&out_file, json).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed writing profile {}: {}",
                out_file.display(),
                e
            ))
        })?;

        Ok(Some(profile_name))
    }

    /// Process a modern Standardized Modular Diagnostic Container (.smr-d)
    pub fn process_smrd_file(
        path: &Path,
        output_dir: &Path,
        catalog: Option<&EcuCatalog>,
    ) -> Result<Option<String>> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("UNKNOWN")
            .to_uppercase();

        let catalog_match = catalog.and_then(|c| c.get_ecu(&stem));
        let primary_chassis = catalog_match
            .and_then(|m| m.chassis.first().cloned())
            .unwrap_or_else(|| "W205".into());

        let (tx_id, rx_id) = if let Some(m) = catalog_match {
            (
                m.tx_id.clone().unwrap_or_else(|| "0x7E0".into()),
                m.rx_id.clone().unwrap_or_else(|| "0x7E8".into()),
            )
        } else {
            ("0x7E0".into(), "0x7E8".into())
        };

        let mut modules = HashMap::new();
        modules.insert(
            stem.clone(),
            ModuleDef {
                name: format!("{} UDS Controller", stem),
                names: HashMap::new(),
                tx_id,
                rx_id,
                protocol: "ISO-14229 (UDS)".into(),
                seed_key_algo: Some("DaimlerStandardLevel0B".into()),
            },
        );

        let mut parameters = Vec::new();
        parameters.push(ParameterDef {
            id: format!("{}_active_software_build", stem.to_lowercase()),
            name: "Software Application Build Version".into(),
            names: HashMap::new(),
            module: stem.clone(),
            service: 0x22,
            did: "0xF189".into(),
            byte_offset: 0,
            length: 4,
            scaling: ScalingDef {
                slope: 1.0,
                offset: 0.0,
            },
            unit: "".into(),
            min: None,
            max: None,
        });
        parameters.push(ParameterDef {
            id: format!("{}_vin", stem.to_lowercase()),
            name: "Vehicle Identification Number (VIN)".into(),
            names: HashMap::new(),
            module: stem.clone(),
            service: 0x22,
            did: "0xF190".into(),
            byte_offset: 0,
            length: 17,
            scaling: ScalingDef {
                slope: 1.0,
                offset: 0.0,
            },
            unit: "".into(),
            min: None,
            max: None,
        });

        let sanitized_chassis = primary_chassis.replace(['/', '\\'], "_");
        let profile_name = format!(
            "{}_{}",
            sanitized_chassis.to_lowercase(),
            stem.to_lowercase()
        );
        let profile = VehicleProfile {
            profile_name: profile_name.clone(),
            oem: "Mercedes-Benz".into(),
            chassis: primary_chassis,
            gateway_type: Some("DoIP / CAN Gateway".into()),
            default_bitrate: 500_000,
            modules,
            parameters,
        };

        let out_file = output_dir.join(format!("{}.json", profile_name));
        let json = serde_json::to_string_pretty(&profile).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed serializing profile {}: {}",
                profile_name, e
            ))
        })?;
        std::fs::write(&out_file, json).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed writing profile {}: {}",
                out_file.display(),
                e
            ))
        })?;

        Ok(Some(profile_name))
    }
}
