use crate::error::{Result, SterngateError};
use crate::i18n::Language;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// SBC (Sensotronic Brake Control) Service Mode Action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SbcServiceAction {
    /// Deactivate SBC system (dump 160 bar accumulator to 0 bar, retract pistons, lock wake-up)
    Deactivate,
    /// Reactivate SBC system (recharge accumulator to ~160 bar, automated bleed check)
    Reactivate,
    /// Hydraulic pressure bleeding procedure
    Bleed,
}

/// SBC Service Mode Execution Status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SbcServiceStatus {
    pub action: SbcServiceAction,
    pub success: bool,
    pub accumulator_pressure_bar: f64,
    pub wake_up_suppressed: bool,
    pub service_mode_active: bool,
    pub message: String,
}

/// Common Rail Injector IMA (Injector Quantity Adaptation) Classification Data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImaClassification {
    /// Cylinder number (1-based, e.g. 1 to 4 for OM646, 1 to 6 for OM642)
    pub cylinder: u8,
    /// Alphanumeric calibration code (e.g. "7B8HNA", "A8B12F")
    pub code: String,
    /// Whether the code passes length and character validation
    pub is_valid: bool,
    /// Injector manufacturer / format classification (e.g. "Bosch IMA (6-char)", "Delphi/Bosch EMA (7-char)")
    pub format: String,
}

impl ImaClassification {
    /// Validate and create an IMA classification entry
    pub fn new(cylinder: u8, code: impl Into<String>) -> Self {
        let code = code.into().trim().to_uppercase();
        let len = code.len();
        let is_alphanumeric = code.chars().all(|c| c.is_ascii_alphanumeric());

        let (is_valid, format) = if (len == 6 || len == 7) && is_alphanumeric {
            let fmt = if len == 6 {
                "Bosch IMA (6-character)".to_string()
            } else {
                "Bosch/Delphi EMA (7-character)".to_string()
            };
            (true, fmt)
        } else {
            (
                false,
                "Invalid Format (must be 6 or 7 alphanumeric characters)".to_string(),
            )
        };

        Self {
            cylinder,
            code,
            is_valid,
            format,
        }
    }
}

/// Air Suspension (ENR / AIRMATIC) Corner Selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionCorner {
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
    BothRear,
    AllCorners,
}

impl SuspensionCorner {
    pub fn as_str(&self) -> &'static str {
        match self {
            SuspensionCorner::FrontLeft => "Front-Left",
            SuspensionCorner::FrontRight => "Front-Right",
            SuspensionCorner::RearLeft => "Rear-Left",
            SuspensionCorner::RearRight => "Rear-Right",
            SuspensionCorner::BothRear => "Both Rear Axle",
            SuspensionCorner::AllCorners => "All 4 Corners",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        let lower = s.to_lowercase().replace('_', "-");
        match lower.as_str() {
            "fl" | "front-left" | "frontleft" => Some(SuspensionCorner::FrontLeft),
            "fr" | "front-right" | "frontright" => Some(SuspensionCorner::FrontRight),
            "rl" | "rear-left" | "rearleft" => Some(SuspensionCorner::RearLeft),
            "rr" | "rear-right" | "rearright" => Some(SuspensionCorner::RearRight),
            "rear" | "both-rear" | "rear-axle" => Some(SuspensionCorner::BothRear),
            "all" | "all-corners" => Some(SuspensionCorner::AllCorners),
            _ => None,
        }
    }
}

/// Air Suspension Corner Actuation Action
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionCornerAction {
    /// Inflate air spring corner
    Inflate,
    /// Deflate air spring corner
    Deflate,
    /// Store zero-height driving calibration level
    CalibrateZeroHeight,
}

/// Discovered ECU during bus-wide interrogation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredEcu {
    /// Physical CAN arbitration request ID (e.g. 0x7E0)
    pub tx_id: u32,
    /// Physical CAN arbitration response ID (e.g. 0x7E8)
    pub rx_id: u32,
    /// Diagnostic protocol (e.g. "ISO-14229 (UDS)", "ISO-14230 (KWP2000)")
    pub protocol: String,
    /// Mercedes/OEM spare part number (DID 0xF187)
    pub part_number: Option<String>,
    /// Hardware version / number (DID 0xF191)
    pub hardware_version: Option<String>,
    /// Software version (DID 0xF189)
    pub software_version: Option<String>,
    /// Software calibration / application number (DID 0xF188)
    pub software_calibration: Option<String>,
    /// Vehicle VIN stored in module (DID 0xF190)
    pub vin: Option<String>,
    /// ECU System name (DID 0xF197 or deduced)
    pub system_name: Option<String>,
    /// Matched ECU catalog definition (from Sterngate catalog database)
    pub matched_catalog_name: Option<String>,
}

/// AdBlue / SCR System Countdown & Lockout Reset Status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdBlueResetStatus {
    pub success: bool,
    pub security_unlocked: bool,
    pub countdown_reset: bool,
    pub adaptations_cleared: bool,
    pub level_sensor_calibrated: bool,
    pub remaining_distance_km: Option<u32>,
    pub message: String,
}

/// ECO Start-Stop Operating Preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EcoStartStopMode {
    /// Factory standard: Always active on engine start
    AlwaysOn,
    /// Enthusiast preference: Remember last user button state across ignition cycles
    RememberLastState,
    /// Inverted: Default to disabled on start, driver must manually press to enable
    DefaultOff,
}

impl EcoStartStopMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            EcoStartStopMode::AlwaysOn => "Always On (Factory Default)",
            EcoStartStopMode::RememberLastState => "Remember Last State (Memory Mode)",
            EcoStartStopMode::DefaultOff => "Default Disabled",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "always-on" | "always_on" | "on" | "factory" => Some(EcoStartStopMode::AlwaysOn),
            "memory" | "remember" | "remember_last_state" | "last_state" => {
                Some(EcoStartStopMode::RememberLastState)
            }
            "off" | "disabled" | "default_off" | "default-off" => {
                Some(EcoStartStopMode::DefaultOff)
            }
            _ => None,
        }
    }
}

/// ECO Start-Stop Configuration Status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcoStartStopStatus {
    pub mode: EcoStartStopMode,
    pub success: bool,
    pub previous_mode: Option<EcoStartStopMode>,
    pub module: String,
    pub did: u16,
    pub message: String,
}

/// EGR (Exhaust Gas Recirculation) Adaptation Soot Reduction Optimization Status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EgrOptimizationStatus {
    pub success: bool,
    pub air_mass_offset_mg: f64,
    pub stops_relearned: bool,
    pub module: String,
    pub message: String,
}

/// Vehicle Maximum Road Speed Limiter (VMax) Configuration Status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeedLimiterStatus {
    pub success: bool,
    pub speed_limit_kmh: u16,
    pub previous_limit_kmh: Option<u16>,
    pub module: String,
    pub did: u16,
    pub message: String,
}

/// Instrument Cluster (KI) Seatbelt Warning Chime Configuration Status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatbeltChimeStatus {
    pub success: bool,
    pub acoustic_chime_enabled: bool,
    pub visual_warning_lamp_active: bool,
    pub module: String,
    pub did: u16,
    pub message: String,
}

/// Instrument Cluster (KI) Remaining Fuel Exact Liters (Restliteranzeige) Status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TankLitersStatus {
    pub success: bool,
    pub exact_liters_display_enabled: bool,
    pub module: String,
    pub did: u16,
    pub message: String,
}

/// Front SAM Intelligent Cornering Fog Lights (Abbiegelicht) Status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorneringLightsStatus {
    pub success: bool,
    pub cornering_lights_enabled: bool,
    pub activation_threshold_kmh: u8,
    pub module: String,
    pub did: u16,
    pub message: String,
}

/// Factory Workshop Service Routine Definition (0x31 RoutineControl)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkshopRoutineDefinition {
    pub routine_id: String,
    pub name_en: String,
    pub name_de: String,
    pub name_sv: String,
    pub category: String,
    pub raw_request_prefix: String,
    #[serde(default)]
    pub ecus: Vec<String>,
}

impl WorkshopRoutineDefinition {
    /// Return the localized routine name
    pub fn localized_name(&self, lang: Language) -> &str {
        match lang {
            Language::En => &self.name_en,
            Language::De => &self.name_de,
            Language::Sv => &self.name_sv,
        }
    }
}

/// Metadata header for Workshop Routine Catalog
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutineCatalogMetadata {
    pub title: String,
    pub total_routines: usize,
    pub version: String,
}

/// Comprehensive Mercedes-Benz Factory Workshop Service Routine Catalog
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkshopRoutineCatalog {
    pub metadata: RoutineCatalogMetadata,
    pub routines: BTreeMap<String, WorkshopRoutineDefinition>,
}

impl WorkshopRoutineCatalog {
    /// Load catalog from a specific file path
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to read Workshop Routine catalog at {}: {}",
                path_ref.display(),
                e
            ))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to parse Workshop Routine catalog JSON: {}",
                e
            ))
        })
    }

    /// Load catalog using standard lookup heuristics
    pub fn load_default() -> Result<Self> {
        if let Ok(env_path) = std::env::var("STERNGATE_ROUTINE_CATALOG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Self::load_from_path(p);
            }
        }
        let candidates = [
            Path::new("data/routine_catalog_mb.json"),
            Path::new("../../data/routine_catalog_mb.json"),
            Path::new("../data/routine_catalog_mb.json"),
        ];

        for &candidate in &candidates {
            if candidate.exists() {
                return Self::load_from_path(candidate);
            }
        }
        // Fallback embedded compile-time catalog
        const FALLBACK_ROUTINES: &str = include_str!("../../../data/routine_catalog_mb.json");
        serde_json::from_str(FALLBACK_ROUTINES).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to parse embedded routine catalog: {}", e))
        })
    }

    /// Find routine by ID (e.g. "0xFF01", "0xff01", or "FF01")
    pub fn get_routine(&self, routine_id: &str) -> Option<&WorkshopRoutineDefinition> {
        let trimmed = routine_id.trim();
        let stripped = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        let clean_id = format!("0x{}", stripped.to_uppercase());
        self.routines.get(&clean_id)
    }

    /// Search routines by query and optional ECU filter
    pub fn search(
        &self,
        query: &str,
        ecu_filter: Option<&str>,
        limit: usize,
    ) -> Vec<WorkshopRoutineDefinition> {
        let q = query.trim().to_uppercase();
        let ecu_q = ecu_filter.map(|e| e.trim().to_uppercase());

        let mut matches: Vec<WorkshopRoutineDefinition> = self
            .routines
            .values()
            .filter(|r| {
                let matches_ecu = match &ecu_q {
                    Some(target_ecu) if !target_ecu.is_empty() => {
                        r.ecus.iter().any(|e| e.to_uppercase().contains(target_ecu))
                    }
                    _ => true,
                };
                if !matches_ecu {
                    return false;
                }

                if q.is_empty() {
                    return true;
                }

                r.routine_id.to_uppercase().contains(&q)
                    || r.name_en.to_uppercase().contains(&q)
                    || r.name_de.to_uppercase().contains(&q)
                    || r.name_sv.to_uppercase().contains(&q)
                    || r.category.to_uppercase().contains(&q)
            })
            .cloned()
            .collect();

        // Sort: exact routine ID first, then by number of applicable ECUs descending
        matches.sort_by(|a, b| {
            let a_exact = a.routine_id == q;
            let b_exact = b.routine_id == q;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }
            b.ecus.len().cmp(&a.ecus.len())
        });

        if limit > 0 && matches.len() > limit {
            matches.truncate(limit);
        }
        matches
    }
}

/// Factory Variant Coding DID Definition (0x2E WriteDataByIdentifier)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantCodingDefinition {
    pub did: String,
    pub name: String,
    pub length_bytes: usize,
    pub is_vin_parameter: bool,
    pub is_fingerprint: bool,
    #[serde(default)]
    pub ecus: Vec<String>,
}

/// Metadata header for Variant Coding Catalog
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodingCatalogMetadata {
    pub title: String,
    pub total_dids: usize,
    pub version: String,
}

/// Comprehensive Mercedes-Benz Factory Variant Coding DID Catalog
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantCodingCatalog {
    pub metadata: CodingCatalogMetadata,
    pub coding_dids: BTreeMap<String, VariantCodingDefinition>,
}

impl VariantCodingCatalog {
    /// Load catalog from a specific file path
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        let content = std::fs::read_to_string(path_ref).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to read Variant Coding catalog at {}: {}",
                path_ref.display(),
                e
            ))
        })?;
        serde_json::from_str(&content).map_err(|e| {
            SterngateError::ProfileError(format!(
                "Failed to parse Variant Coding catalog JSON: {}",
                e
            ))
        })
    }

    /// Load catalog using standard lookup heuristics
    pub fn load_default() -> Result<Self> {
        if let Ok(env_path) = std::env::var("STERNGATE_CODING_CATALOG") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                return Self::load_from_path(p);
            }
        }
        let candidates = [
            Path::new("data/coding_catalog_mb.json"),
            Path::new("../../data/coding_catalog_mb.json"),
            Path::new("../data/coding_catalog_mb.json"),
        ];

        for &candidate in &candidates {
            if candidate.exists() {
                return Self::load_from_path(candidate);
            }
        }
        const FALLBACK_CODING: &str = include_str!("../../../data/coding_catalog_mb.json");
        serde_json::from_str(FALLBACK_CODING).map_err(|e| {
            SterngateError::ProfileError(format!("Failed to parse embedded coding catalog: {}", e))
        })
    }

    /// Find coding DID (e.g. "0xF190", "0xf190", or "F190")
    pub fn get_did(&self, did: &str) -> Option<&VariantCodingDefinition> {
        let trimmed = did.trim();
        let stripped = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        let clean_did = format!("0x{}", stripped.to_uppercase());
        self.coding_dids.get(&clean_did)
    }

    /// Search coding DIDs by query and optional ECU filter
    pub fn search(
        &self,
        query: &str,
        ecu_filter: Option<&str>,
        limit: usize,
    ) -> Vec<VariantCodingDefinition> {
        let q = query.trim().to_uppercase();
        let ecu_q = ecu_filter.map(|e| e.trim().to_uppercase());

        let mut matches: Vec<VariantCodingDefinition> = self
            .coding_dids
            .values()
            .filter(|c| {
                let matches_ecu = match &ecu_q {
                    Some(target_ecu) if !target_ecu.is_empty() => {
                        c.ecus.iter().any(|e| e.to_uppercase().contains(target_ecu))
                    }
                    _ => true,
                };
                if !matches_ecu {
                    return false;
                }

                if q.is_empty() {
                    return true;
                }

                c.did.to_uppercase().contains(&q) || c.name.to_uppercase().contains(&q)
            })
            .cloned()
            .collect();

        matches.sort_by(|a, b| {
            let a_exact = a.did == q;
            let b_exact = b.did == q;
            if a_exact != b_exact {
                return b_exact.cmp(&a_exact);
            }
            b.ecus.len().cmp(&a.ecus.len())
        });

        if limit > 0 && matches.len() > limit {
            matches.truncate(limit);
        }
        matches
    }
}

/// Donor ECU Re-VIN Adaptation Status and Result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DonorEcuVinAdaptation {
    pub target_ecu: String,
    pub original_vin: Option<String>,
    pub current_vin: Option<String>,
    pub new_vin: String,
    pub security_level: String,
    pub success: bool,
    pub verified_by_readback: bool,
    pub message: String,
}

impl DonorEcuVinAdaptation {
    /// Validate 17-character ISO 3779 VIN format
    pub fn validate_vin(vin: &str) -> bool {
        let v = vin.trim().to_uppercase();
        if v.len() != 17 {
            return false;
        }
        // ISO 3779 forbids letters I, O, Q to avoid confusion with numerals 1, 0
        v.chars().all(|c| {
            (c.is_ascii_uppercase() && c != 'I' && c != 'O' && c != 'Q') || c.is_ascii_digit()
        })
    }
}
