use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Identifies the known Mercedes-Benz cascading failure modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CascadeId {
    SbcAccumulatorExhaustion,
    CommonRailBlackDeath,
    TransmissionPilotBushingWicking,
    TccLockupSlip,
    DpfDifferentialDriftM55,
    CamshaftMagnetOilWicking,
    SuspensionCompressorBurnout,
    AbcPulsationDamperSurge,
    EslMotorLockout,
    M272BalanceShaftChainWear,
    ValeoRadiatorGlycolContamination,
    SamWaterIngressParasiticDrain,
    Om642OilCoolerValleyLeak,
}

impl CascadeId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SbcAccumulatorExhaustion => "sbc_accumulator_exhaustion",
            Self::CommonRailBlackDeath => "common_rail_black_death",
            Self::TransmissionPilotBushingWicking => "transmission_pilot_bushing_wicking",
            Self::TccLockupSlip => "tcc_lockup_slip",
            Self::DpfDifferentialDriftM55 => "dpf_differential_drift_m55",
            Self::CamshaftMagnetOilWicking => "camshaft_magnet_oil_wicking",
            Self::SuspensionCompressorBurnout => "suspension_compressor_burnout",
            Self::AbcPulsationDamperSurge => "abc_pulsation_damper_surge",
            Self::EslMotorLockout => "esl_motor_lockout",
            Self::M272BalanceShaftChainWear => "m272_balance_shaft_chain_wear",
            Self::ValeoRadiatorGlycolContamination => "valeo_radiator_glycol_contamination",
            Self::SamWaterIngressParasiticDrain => "sam_water_ingress_parasitic_drain",
            Self::Om642OilCoolerValleyLeak => "om642_oil_cooler_valley_leak",
        }
    }
}

/// Cascade risk classification severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CascadeSeverity {
    Normal = 0,
    Watchlist = 1,
    ImminentDanger = 2,
}

impl CascadeSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Watchlist => "Watchlist",
            Self::ImminentDanger => "ImminentDanger",
        }
    }
}

/// Alert details for an individual cascade evaluation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CascadeAlert {
    pub id: CascadeId,
    pub name: String,
    pub severity: CascadeSeverity,
    pub root_cause_part: String,
    pub catastrophic_outcome: String,
    pub telemetry_evidence: String,
    pub recommendation: String,
    pub oem_part_numbers: Vec<String>,
    pub affected_chassis: Vec<String>,
}

/// Comprehensive report compiling all checked cascades
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CascadeReport {
    pub timestamp: String,
    pub overall_severity: CascadeSeverity,
    pub alerts: Vec<CascadeAlert>,
    pub total_cascades_checked: usize,
}

impl CascadeReport {
    /// Format report into human-readable Markdown
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("# Mercedes-Benz 'Cascade of Death' Early Warning Report\n\n");
        md.push_str(&format!("- **Scan Timestamp:** `{}`\n", self.timestamp));
        md.push_str(&format!(
            "- **Overall Status:** {}\n",
            match self.overall_severity {
                CascadeSeverity::Normal => "✅ ALL SYSTEMS HEALTHY",
                CascadeSeverity::Watchlist => "⚠️ PREVENTIVE WATCHLIST ITEMS DETECTED",
                CascadeSeverity::ImminentDanger => "🚨 CRITICAL: IMMINENT CASCADE FAILURE DETECTED",
            }
        ));
        md.push_str(&format!(
            "- **Total Cascades Evaluated:** {}\n\n",
            self.total_cascades_checked
        ));

        if self.alerts.is_empty() {
            md.push_str("No active cascade warnings or mechanical anomalies detected.\n");
            return md;
        }

        md.push_str("## Active Cascade Warnings & Intervention Guide\n\n");
        for alert in &self.alerts {
            let icon = match alert.severity {
                CascadeSeverity::Normal => "✅",
                CascadeSeverity::Watchlist => "⚠️",
                CascadeSeverity::ImminentDanger => "🚨",
            };
            md.push_str(&format!(
                "### {} {} ({})\n",
                icon,
                alert.name,
                alert.severity.as_str()
            ));
            md.push_str(&format!(
                "- **Telemetry Evidence:** {}\n",
                alert.telemetry_evidence
            ));
            md.push_str(&format!(
                "- **Inexpensive Root Trigger:** {}\n",
                alert.root_cause_part
            ));
            md.push_str(&format!(
                "- **Catastrophic Outcome if Ignored:** {}\n",
                alert.catastrophic_outcome
            ));
            md.push_str(&format!(
                "- **Recommended Action:** {}\n",
                alert.recommendation
            ));
            if !alert.oem_part_numbers.is_empty() {
                md.push_str(&format!(
                    "- **OEM Part Numbers:** {}\n",
                    alert.oem_part_numbers.join(", ")
                ));
            }
            if !alert.affected_chassis.is_empty() {
                md.push_str(&format!(
                    "- **Affected Models:** {}\n",
                    alert.affected_chassis.join(", ")
                ));
            }
            md.push('\n');
        }
        md
    }
}

/// Input parameters provided to the CascadeWatchdog evaluator
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CascadeTelemetryInput {
    // 1. SBC Braking System
    pub sbc_accumulator_pressure_bar: Option<f64>,
    pub sbc_pump_per_brake_ratio: Option<f64>,
    pub sbc_operating_cycles: Option<u64>,
    pub sbc_max_cycles: Option<u64>,

    // 2. Common Rail Injection (Black Death)
    pub max_cylinder_balance_trim_mm3: Option<f64>,
    pub cylinder_balance_spread_mm3: Option<f64>,
    pub rail_pressure_bleed_rate_bar_sec: Option<f64>,

    // 3. 722.6 Transmission Pilot Bushing
    pub atf_temp_rapid_jump_deg_c: Option<f64>,
    pub transmission_speed_sensor_jitter: Option<bool>,

    // 4. 722.6 Torque Converter Lockup (TCC)
    pub tcc_slip_rpm: Option<f64>,
    pub tcc_lockup_commanded: Option<bool>,

    // 5. DPF Differential Drift & M55 Swirl Flap
    pub dpf_diff_pressure_mbar: Option<f64>,
    pub engine_rpm: Option<f64>,
    pub distance_since_dpf_regen_km: Option<f64>,

    // 6. Camshaft Magnet Oil Wicking
    pub cam_magnet_oil_detected: Option<bool>,
    pub o2_sensor_heater_resistance_drift: Option<bool>,
    pub five_volt_ref_bus_dip: Option<bool>,

    // 7. S211 Air Suspension (ENR) Compressor
    pub compressor_continuous_run_sec: Option<f64>,
    pub compressor_duty_cycle_pct: Option<f64>,
    pub suspension_height_drop_rate_mm_h: Option<f64>,

    // 8. ABC (Active Body Control) Hydraulic System
    pub abc_pressure_ripple_bar: Option<f64>,
    pub abc_system_pressure_bar: Option<f64>,

    // 9. Electronic Steering Lock (ESL / ELV)
    pub esl_unlock_duration_ms: Option<f64>,
    pub esl_retry_count: Option<u32>,

    // 10. M272 / M273 Balance Shaft & Timing Chain
    pub cam_phase_deviation_deg: Option<f64>,

    // 11. Valeo Radiator Glycol Contamination
    pub tcc_slip_oscillation_hz: Option<f64>,
    pub tcc_slip_oscillation_rpm: Option<f64>,

    // 12. SAM Water Ingress & CAN-B Parasitic Drain
    pub can_sleep_delay_seconds: Option<f64>,
    pub quiescent_current_amps: Option<f64>,

    // 13. OM642 V-Valley Oil Cooler Leak
    pub dynamic_oil_loss_rate_mm_100km: Option<f64>,
    pub engine_oil_temperature_c: Option<f64>,

    // General diagnostic codes
    #[serde(default)]
    pub active_dtcs: Vec<String>,
}

/// Evaluator engine for Mercedes-Benz cascading failures
pub struct CascadeWatchdog;

impl CascadeWatchdog {
    /// Evaluates all known cascades and returns a consolidated report
    pub fn evaluate(input: &CascadeTelemetryInput) -> CascadeReport {
        let mut alerts = Vec::new();

        if let Some(alert) = Self::check_sbc(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_black_death(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_pilot_bushing(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_tcc_slip(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_dpf_m55(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_cam_magnets(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_air_suspension(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_abc(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_esl(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_m272_timing(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_valeo_glycol(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_sam_water(input) {
            alerts.push(alert);
        }
        if let Some(alert) = Self::check_om642_oil_cooler(input) {
            alerts.push(alert);
        }

        let overall_severity = alerts
            .iter()
            .map(|a| a.severity)
            .max()
            .unwrap_or(CascadeSeverity::Normal);

        CascadeReport {
            timestamp: Utc::now().to_rfc3339(),
            overall_severity,
            alerts,
            total_cascades_checked: 13,
        }
    }

    /// 1. SBC (Sensotronic Brake Control) Hydraulic Accumulator & Pump Exhaustion
    fn check_sbc(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(pressure) = input.sbc_accumulator_pressure_bar {
            if pressure < 55.0 {
                severity = CascadeSeverity::ImminentDanger;
                evidence.push(format!(
                    "Accumulator pre-charge pressure critically low ({:.1} bar < 55.0 bar threshold)",
                    pressure
                ));
            } else if pressure < 70.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Accumulator pre-charge pressure degraded ({:.1} bar < 70.0 bar nominal)",
                    pressure
                ));
            }
        }

        if let Some(ratio) = input.sbc_pump_per_brake_ratio {
            if ratio >= 0.75 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Pump running on {:.0}% of brake presses (nominal < 25%)",
                    ratio * 100.0
                ));
            } else if ratio >= 0.40 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Frequent pump charging cycles ({:.0}% of brake presses)",
                    ratio * 100.0
                ));
            }
        }

        if let (Some(cycles), Some(max_cycles)) = (input.sbc_operating_cycles, input.sbc_max_cycles)
        {
            if cycles >= max_cycles {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Brake actuations ({} cycles) exceeded factory service limit ({})",
                    cycles, max_cycles
                ));
            } else if cycles >= (max_cycles * 85 / 100) {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Brake actuations ({} cycles) approaching 85% of service threshold",
                    cycles
                ));
            }
        }

        if input.active_dtcs.iter().any(|d| d.contains("C249F")) {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push(
                "DTC C249F stored: Operating time of component A7/3 (SBC hydraulic unit) exceeded"
                    .to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::SbcAccumulatorExhaustion,
                name: "SBC Hydraulic Accumulator Exhaustion".to_string(),
                severity,
                root_cause_part: "Nitrogen pressure accumulator A 000 430 26 94 (~$120)".to_string(),
                catastrophic_outcome: "Pump motor burnout -> Total loss of power brake boost -> Emergency unassisted hydraulic fallback (~$2,500)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "CRITICAL: Replace SBC accumulator sphere immediately and perform pressure bleeder calibration before complete pump failure occurs."
                } else {
                    "Inspect SBC accumulator pre-charge pressure and plan accumulator replacement at next service interval."
                }.to_string(),
                oem_part_numbers: vec!["A 000 430 26 94".to_string(), "A 005 431 97 12".to_string()],
                affected_chassis: vec!["W211 (2002-2006)".to_string(), "R230 (SL)".to_string(), "C219 (CLS 2004-2006)".to_string()],
            })
        } else {
            None
        }
    }

    /// 2. Common Rail Injector "Black Death" (Copper Seal Blow-by)
    fn check_black_death(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(trim) = input.max_cylinder_balance_trim_mm3 {
            if trim >= 3.5 {
                severity = CascadeSeverity::ImminentDanger;
                evidence.push(format!(
                    "Cylinder smooth-running trim critically high (+{:.2} mm³/hub >= +3.5 mm³)",
                    trim
                ));
            } else if trim >= 2.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Cylinder smooth-running balance elevated (+{:.2} mm³/hub >= +2.0 mm³)",
                    trim
                ));
            }
        }

        if let Some(spread) = input.cylinder_balance_spread_mm3 {
            if spread >= 4.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Extreme cylinder-to-cylinder balance asymmetry ({:.2} mm³/hub)",
                    spread
                ));
            }
        }

        if let Some(bleed) = input.rail_pressure_bleed_rate_bar_sec {
            if bleed >= 50.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "High common rail shut-off leak-down rate ({:.1} bar/s)",
                    bleed
                ));
            }
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::CommonRailBlackDeath,
                name: "Common Rail Injector 'Black Death' Blow-By".to_string(),
                severity,
                root_cause_part: "Copper injector seal crush washer A 611 017 00 60 ($1.50) & stretch bolt A 000 990 22 97 ($2.00)".to_string(),
                catastrophic_outcome: "Combustion blow-by solidifies into rock-hard carbon sludge, melting harness loom and cementing injector into head ($1,800–$3,500)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "URGENT: Remove engine vanity cover and inspect injector wells for carbon buildup. Replace copper washers and stretch bolts using ceramic grease before injector cements."
                } else {
                    "Monitor cylinder smooth-running balance and schedule injector seal replacement with genuine Daimler copper washer."
                }.to_string(),
                oem_part_numbers: vec!["A 611 017 00 60".to_string(), "A 000 990 22 97".to_string(), "A 001 989 42 51 10 (Ceramic Paste)".to_string()],
                affected_chassis: vec!["OM646 (W211/S211/W203)".to_string(), "OM648 (W211)".to_string(), "OM642 V6 CDI (W211/W164/W212)".to_string()],
            })
        } else {
            None
        }
    }

    /// 3. 722.6 Transmission 13-Pin Pilot Bushing ATF Wicking
    fn check_pilot_bushing(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(jump) = input.atf_temp_rapid_jump_deg_c {
            if jump >= 25.0 {
                severity = CascadeSeverity::ImminentDanger;
                evidence.push(format!(
                    "Erratic transmission fluid temp spike (+{:.1}°C in <5s; resistive circuit short)",
                    jump
                ));
            } else if jump >= 12.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Unstable transmission fluid temp reading (+{:.1}°C variance)",
                    jump
                ));
            }
        }

        if input.transmission_speed_sensor_jitter.unwrap_or(false) {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push("Transmission RPM speed sensor jitter (Y3/6n2/n3 signal dropout from ATF fluid immersion)".to_string());
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("P220A") || d.contains("P240C") || d.contains("P0715"))
        {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push("EGS transmission speed sensor/solenoid circuit DTC active".to_string());
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::TransmissionPilotBushingWicking,
                name: "722.6 Transmission Pilot Bushing ATF Capillary Wicking".to_string(),
                severity,
                root_cause_part: "13-pin electro-hydraulic adapter bushing A 203 540 02 53 ($8.00)".to_string(),
                catastrophic_outcome: "Capillary action wicks pressurized ATF up wire loom into EGS52 TCU, shorting solenoid MOSFETs and locking in 2nd gear ($1,200–$2,000)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: "Inspect passenger footwell EGS52 TCU connector for transmission fluid. Replace transmission pilot plug adapter immediately with updated double O-ring design.".to_string(),
                oem_part_numbers: vec!["A 203 540 02 53".to_string()],
                affected_chassis: vec!["All 722.6 / NAG1 vehicles (W211, S211, W203, W220, W163, R230)".to_string()],
            })
        } else {
            None
        }
    }

    /// 4. 722.6 Torque Converter Lockup Clutch (TCC / KÜB) Slip
    fn check_tcc_slip(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        let lockup_active = input.tcc_lockup_commanded.unwrap_or(true);
        if lockup_active {
            if let Some(slip) = input.tcc_slip_rpm {
                if slip >= 60.0 {
                    severity = CascadeSeverity::ImminentDanger;
                    evidence.push(format!(
                        "Torque converter clutch slip excessive ({:.1} RPM >= 60 RPM during commanded lockup)",
                        slip
                    ));
                } else if slip >= 30.0 {
                    severity = severity.max(CascadeSeverity::Watchlist);
                    evidence.push(format!(
                        "Elevated torque converter lockup slip ({:.1} RPM >= 30 RPM)",
                        slip
                    ));
                }
            }
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::TccLockupSlip,
                name: "722.6 Torque Converter Lockup Clutch Shredding".to_string(),
                severity,
                root_cause_part: "TCC PWM lockup solenoid Y3/6y6 A 240 270 17 00 or worn valve body bore ($65)".to_string(),
                catastrophic_outcome: "Friction lining disintegrates, distributing abrasive particles into valve body spools and planetary gearsets ($2,800 rebuild)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: "Replace TCC PWM lockup solenoid and Sonnax TCC damper sleeve kit. Flush transmission fluid and replace filter.".to_string(),
                oem_part_numbers: vec!["A 240 270 17 00".to_string(), "A 140 277 00 95 (Filter)".to_string()],
                affected_chassis: vec!["All 722.6 / NAG1 vehicles".to_string()],
            })
        } else {
            None
        }
    }

    /// 5. DPF Differential Pressure Drift -> M55 Swirl Flap Motor Short
    fn check_dpf_m55(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let (Some(dpf_p), Some(rpm)) = (input.dpf_diff_pressure_mbar, input.engine_rpm) {
            if rpm >= 3000.0 && dpf_p < 15.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Differential pressure implausibly flat ({:.1} mbar at {:.0} RPM; sensor diaphragm drifted)",
                    dpf_p, rpm
                ));
            }
        }

        if let Some(dist) = input.distance_since_dpf_regen_km {
            if dist >= 1100.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Distance since successful DPF regeneration ({:.0} km >= 1100 km limit)",
                    dist
                ));
            } else if dist >= 800.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Extended interval since DPF regeneration ({:.0} km >= 800 km)",
                    dist
                ));
            }
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("P2006") || d.contains("P2007") || d.contains("P2452"))
        {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push("DPF pressure sensor or intake swirl flap DTC detected".to_string());
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::DpfDifferentialDriftM55,
                name: "DPF Differential Drift -> Turbo Bearing & M55 Swirl Flap Short".to_string(),
                severity,
                root_cause_part: "DPF differential pressure sensor B28/8 A 006 153 95 28 ($45.00)".to_string(),
                catastrophic_outcome: "Extreme backpressure forces turbo seal blow-by into intake, oiling M55 swirl motor and blowing Front SAM Fuse 54 (highway stall, $3,200)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: "Replace DPF differential pressure sensor B28/8, clear adaptations, and execute service regeneration Routine 0x0202.".to_string(),
                oem_part_numbers: vec!["A 006 153 95 28".to_string(), "A 642 150 04 94 (M55 Motor)".to_string()],
                affected_chassis: vec!["OM642 V6 CDI (W211, W164, W212)".to_string(), "OM646 EVO with DPF".to_string()],
            })
        } else {
            None
        }
    }

    /// 6. Camshaft Adjustment Magnet Oil Wicking into Engine ECU
    fn check_cam_magnets(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if input.cam_magnet_oil_detected.unwrap_or(false) {
            severity = CascadeSeverity::Watchlist;
            evidence.push(
                "Oil residue detected in camshaft adjustment solenoid electrical connector"
                    .to_string(),
            );
        }

        if input.five_volt_ref_bus_dip.unwrap_or(false)
            && input.o2_sensor_heater_resistance_drift.unwrap_or(false)
        {
            severity = CascadeSeverity::ImminentDanger;
            evidence.push("Correlated 5V reference bus voltage dip with O2 sensor heater resistance drift (active capillary oil wicking)".to_string());
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("P0011") || d.contains("P0014") || d.contains("P0021"))
            && input
                .active_dtcs
                .iter()
                .any(|d| d.contains("P0135") || d.contains("P0155"))
        {
            severity = CascadeSeverity::ImminentDanger;
            evidence.push(
                "Simultaneous camshaft positioning and lambda sensor heater circuit faults"
                    .to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::CamshaftMagnetOilWicking,
                name: "Camshaft Magnet Capillary Oil Intrusion into Engine ECU".to_string(),
                severity,
                root_cause_part: "Camshaft adjustment magnet seals A 272 051 01 77 ($30) & blocker pigtails A 271 150 27 33 ($15)".to_string(),
                catastrophic_outcome: "Oil wicks inside wire strands into Bosch ME9.7 ECU motherboard and O2 sensors, destroying electronics ($2,400)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: "Install genuine Mercedes oil-blocking pigtail harnesses (A 271 150 27 33) and replace leaking cam magnets before oil reaches ECU.".to_string(),
                oem_part_numbers: vec!["A 272 051 01 77".to_string(), "A 271 150 27 33 (Pigtail)".to_string(), "A 272 150 00 08 (Pigtail)".to_string()],
                affected_chassis: vec!["M271 (W203/W204/W211)".to_string(), "M272 V6 / M273 V8 (W211, W204, W221, W164)".to_string()],
            })
        } else {
            None
        }
    }

    /// 7. S211 Air Suspension (ENR) / W211 AIRMATIC Compressor Burnout & Relay Welding
    fn check_air_suspension(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(runtime) = input.compressor_continuous_run_sec {
            if runtime >= 40.0 {
                severity = CascadeSeverity::ImminentDanger;
                evidence.push(format!(
                    "Compressor continuous run time critically high ({:.1}s >= 40.0s thermal cutoff limit)",
                    runtime
                ));
            } else if runtime >= 25.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Extended compressor run duration ({:.1}s >= 25.0s)",
                    runtime
                ));
            }
        }

        if let Some(drop_rate) = input.suspension_height_drop_rate_mm_h {
            if drop_rate >= 10.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Stationary rear height drop rate severe ({:.1} mm/h >= 10.0 mm/h; ruptured bellow)",
                    drop_rate
                ));
            } else if drop_rate >= 4.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Pneumatic height drop rate detected ({:.1} mm/h >= 4.0 mm/h)",
                    drop_rate
                ));
            }
        }

        if let Some(duty) = input.compressor_duty_cycle_pct {
            if duty >= 25.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Compressor duty cycle elevated ({:.1}% >= 25.0%)",
                    duty
                ));
            }
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::SuspensionCompressorBurnout,
                name: "S211 Rear Air Suspension Compressor Burnout & Welded Relay".to_string(),
                severity,
                root_cause_part: "Rear air spring bellow A 211 320 09 25 (~$140) & Hella relay A 002 542 72 19 ($12)".to_string(),
                catastrophic_outcome: "Continuous running melts piston Teflon seal; sustained >30A current welds relay contacts closed, burning out motor ($1,200)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "EMERGENCY: Inhibit compressor immediately with `sterngate analyze suspension --inhibit` or set workshop mode `0x0211` to prevent relay welding. Replace leaking air bellow and relay."
                } else {
                    "Inspect rear air bellows with soapy water spray and replace Hella green relay (A 002 542 72 19) preventively."
                }.to_string(),
                oem_part_numbers: vec!["A 211 320 09 25".to_string(), "A 002 542 72 19 (Relay)".to_string(), "A 211 320 03 04 (Compressor)".to_string()],
                affected_chassis: vec!["S211 Estate (ENR rear self-leveling)".to_string(), "W211 / W219 (All 4-corner AIRMATIC)".to_string()],
            })
        } else {
            None
        }
    }

    fn check_abc(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(ripple) = input.abc_pressure_ripple_bar {
            if ripple >= 25.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "ABC line pressure ripple critical ({:.1} bar >= 25.0 bar; pulsation damper diaphragm ruptured)",
                    ripple
                ));
            } else if ripple >= 15.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "ABC line pressure ripple elevated ({:.1} bar >= 15.0 bar; loss of nitrogen pre-charge)",
                    ripple
                ));
            }
        }

        if let Some(pressure) = input.abc_system_pressure_bar {
            if pressure < 130.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "ABC operating pressure low ({:.1} bar < 130.0 bar nominal)",
                    pressure
                ));
            } else if pressure > 215.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "ABC operating pressure surging dangerously ({:.1} bar > 215.0 bar)",
                    pressure
                ));
            }
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("C1525") || d.contains("C1129") || d.contains("C1128"))
        {
            severity = severity.max(CascadeSeverity::Watchlist);
            evidence.push("ABC suspension hydraulic pressure circuit DTC stored".to_string());
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::AbcPulsationDamperSurge,
                name: "ABC Pulsation Damper Rupture & Hydraulic Shockwaves".to_string(),
                severity,
                root_cause_part: "Nitrogen pulsation damper sphere A 220 327 02 15 (~$160)".to_string(),
                catastrophic_outcome: "Undamped 200 bar radial pump pulses fatigue hydraulic line crimps and fracture tandem pump drive shaft, spraying Pentosin CHF 11S over hot exhaust manifold ($8,000–$10,000)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "EMERGENCY: Actuate Routine `0x0220` (system pressure dump to 120 bar fallback) and Routine `0x0221` (lock strut isolation valves) to prevent catastrophic line rupture. Tow to workshop or replace pulsation damper immediately."
                } else {
                    "Inspect ABC pulsation damper A 220 327 02 15 on front tandem pump outlet; replace accumulator sphere before hydraulic surge fractures pump shaft."
                }.to_string(),
                oem_part_numbers: vec!["A 220 327 02 15 (Pulsation Damper)".to_string(), "A 000 989 91 03 (Pentosin CHF 11S)".to_string(), "A 003 466 27 01 (Tandem Pump)".to_string()],
                affected_chassis: vec!["W220 S-Class (ABC 487)".to_string(), "W215 CL-Class".to_string(), "R230 SL-Class".to_string(), "W221 S-Class (V8/V12 ABC)".to_string()],
            })
        } else {
            None
        }
    }

    fn check_esl(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(duration) = input.esl_unlock_duration_ms {
            if duration >= 500.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "ESL unlock latency critical ({:.0} ms >= 500 ms; motor brush carbon depletion, lock seizure imminent)",
                    duration
                ));
            } else if duration >= 250.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "ESL unlock latency elevated ({:.0} ms >= 250 ms; DC motor brush resistance rising)",
                    duration
                ));
            }
        }

        if let Some(retries) = input.esl_retry_count {
            if retries >= 2 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "ESL unlock retries exceeded threshold ({} retries)",
                    retries
                ));
            } else if retries == 1 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push("ESL bolt retraction required retry".to_string());
            }
        }

        if input.active_dtcs.iter().any(|d| {
            d.contains("A25464")
                || d.contains("A25407")
                || d.contains("A25408")
                || d.contains("A25409")
        }) {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push(
                "ESL/ELV electronic steering lock motor or microswitch DTC stored".to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::EslMotorLockout,
                name: "Electronic Steering Lock (ESL / ELV) Brush Seizure & Permanent Lockout".to_string(),
                severity,
                root_cause_part: "12V DC Johnson/Nichibo FC-280SC micro-motor internal commutator brush carbon wear ($5)".to_string(),
                catastrophic_outcome: "Bolt motor stalls mid-stroke; internal NEC microcontroller permanently blows security fuse bit. Terminal 15/50 permanently inhibited, steering column locked, requiring complete column removal and drilling ($2,000+)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "DO NOT REMOVE KEY FROM IGNITION! Removing the key allows the locking bolt to cycle and seize permanently locked. Drive immediately to workshop with key inserted, or install an ESL emulator (A 204 545 57 32 bypass) while column is unlocked."
                } else {
                    "Pre-emptively install an electronic steering lock (ESL/ELV) emulator or replace the internal DC motor (Nichibo FC-280SC) while the steering column is still unlocked."
                }.to_string(),
                oem_part_numbers: vec!["A 204 545 57 32 (ESL/ELV Unit)".to_string(), "Nichibo FC-280SC (DC Motor)".to_string(), "ESL Plug & Play Emulator".to_string()],
                affected_chassis: vec!["W204 C-Class".to_string(), "X204 GLK-Class".to_string(), "W212 E-Class (Early pre-facelift)".to_string(), "W207 E-Coupe".to_string()],
            })
        } else {
            None
        }
    }

    fn check_m272_timing(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(dev) = input.cam_phase_deviation_deg {
            if dev.abs() >= 3.2 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Camshaft phase deviation critical ({:+.2}° >= 3.2°; balance shaft / idler sprocket teeth stripped)",
                    dev
                ));
            } else if dev.abs() >= 1.5 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Camshaft phase angle drift detected ({:+.2}° >= 1.5°; initial tooth wear on balance shaft drive gear)",
                    dev
                ));
            }
        }

        if input.active_dtcs.iter().any(|d| {
            d.contains("1200")
                || d.contains("1208")
                || d.contains("P0016")
                || d.contains("P0017")
                || d.contains("1203")
                || d.contains("1205")
        }) {
            severity = severity.max(CascadeSeverity::ImminentDanger);
            evidence.push("DTC 1200/1208/P0016/P0017 stored: Constant adjustment of exhaust/intake camshaft of right cylinder bank towards retarded direction".to_string());
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::M272BalanceShaftChainWear,
                name: "M272/M273 Balance Shaft & Idler Sprocket Tooth Wear".to_string(),
                severity,
                root_cause_part: "Soft sintered metal on balance shaft sprocket (M272) or timing chain idler gear (M273) A 272 050 08 04 ($60)".to_string(),
                catastrophic_outcome: "Sprocket teeth grind down to nubs, chain jumps multiple teeth, causing intake/exhaust valve-to-piston collision and catastrophic engine destruction ($6,000+)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "CRITICAL: Stop driving immediately. Sintered drive teeth are failing. Do not run engine under load to prevent chain jumping and piston-valve collision. Inspect camshaft sensor stampings and plan balance shaft / idler gear replacement."
                } else {
                    "Inspect timing mark alignment through camshaft Hall sensor ports (stamping 301-304). Plan replacement of balance shaft (M272) or intermediate gear (M273) with updated hardened part (A 272 050 15 04)."
                }.to_string(),
                oem_part_numbers: vec!["A 272 050 15 04 (Hardened Balance Shaft)".to_string(), "A 272 050 08 04 (Superseded Gear)".to_string(), "A 000 993 06 76 (Timing Chain)".to_string()],
                affected_chassis: vec!["W211 E350 / E500 (M272/M273 2004-2008)".to_string(), "W203/W204 C280 / C350".to_string(), "W164 ML350 / ML500".to_string(), "W221 S350 / S500".to_string()],
            })
        } else {
            None
        }
    }

    fn check_valeo_glycol(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let (Some(hz), Some(rpm)) = (
            input.tcc_slip_oscillation_hz,
            input.tcc_slip_oscillation_rpm,
        ) {
            if (4.0..=12.0).contains(&hz) {
                if rpm >= 35.0 {
                    severity = severity.max(CascadeSeverity::ImminentDanger);
                    evidence.push(format!(
                        "Harmonic TCC slip RPM oscillation detected ({:.1} RPM at {:.1} Hz; characteristic glycol contamination clutch delamination)",
                        rpm, hz
                    ));
                } else if rpm >= 15.0 {
                    severity = severity.max(CascadeSeverity::Watchlist);
                    evidence.push(format!(
                        "Micro-oscillation in torque converter clutch slip ({:.1} RPM at {:.1} Hz in 1500-2000 RPM band)",
                        rpm, hz
                    ));
                }
            }
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("2783") || d.contains("P0741") || d.contains("P0744"))
        {
            severity = severity.max(CascadeSeverity::Watchlist);
            evidence.push(
                "Torque converter lockup clutch friction power or slippage DTC stored".to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::ValeoRadiatorGlycolContamination,
                name: "Valeo Radiator Glycol Intrusion into 722.6 Transmission".to_string(),
                severity,
                root_cause_part: "Crimp joint defect in Valeo transmission fluid heat exchanger integrated into radiator core ($0 part of radiator)".to_string(),
                catastrophic_outcome: "Ethylene glycol dissolves water-based adhesive holding friction linings to clutch plates; clutch material peels off into valve body, causing total 722.6 transmission failure ($3,500)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "CRITICAL: Perform immediate cuvette glycol test (A 001 988 84 44). If glycol > 100 mg/l, flush transmission with 14L ATF immediately, replace radiator with Behr unit or fit auxiliary external air-to-oil transmission cooler ($80)."
                } else {
                    "Verify radiator brand (Valeo radiator with corrugated crimping produced pre-09/2003 is vulnerable). Fit external transmission oil cooler to permanently isolate coolant from ATF."
                }.to_string(),
                oem_part_numbers: vec!["A 211 500 31 02 (Behr Radiator)".to_string(), "A 001 988 84 44 (Glycol Test Kit)".to_string(), "A 001 989 68 03 (MB 236.14 ATF)".to_string()],
                affected_chassis: vec!["W211 E-Class (2002-09/2003 with Valeo radiator)".to_string(), "W203 C-Class (2000-09/2003)".to_string(), "W209 CLK-Class".to_string(), "R230 SL350".to_string()],
            })
        } else {
            None
        }
    }

    fn check_sam_water(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(delay) = input.can_sleep_delay_seconds {
            if delay >= 120.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "CAN-B bus refused deep sleep ({:.0}s >= 120s; wake-up loop from SAM water ingress)",
                    delay
                ));
            } else if delay >= 45.0 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Delayed CAN-B interior bus sleep ({:.0}s >= 45s)",
                    delay
                ));
            }
        }

        if let Some(current) = input.quiescent_current_amps {
            if current >= 2.0 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Excessive standby quiescent battery drain ({:.2}A >= 2.0A; SAM high-side bridge latch)",
                    current
                ));
            } else if current >= 0.25 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Elevated standby parasitic draw ({:.2}A >= 0.25A; nominal < 0.04A)",
                    current
                ));
            }
        }

        if input.active_dtcs.iter().any(|d| {
            d.contains("B1000") || d.contains("9022") || d.contains("9023") || d.contains("9028")
        }) {
            severity = severity.max(CascadeSeverity::Watchlist);
            evidence.push(
                "SAM control unit internal fault or undervoltage supply DTC stored".to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::SamWaterIngressParasiticDrain,
                name: "Cowl/Sunroof Drain Clog -> SAM Water Ingress & Parasitic Drain".to_string(),
                severity,
                root_cause_part: "Debris-clogged firewall cowl duckbill drains and sunroof drain hoses ($0 cleaning cost)".to_string(),
                catastrophic_outcome: "Water overflows windshield wiper plenum into Front SAM or taillight seals into Rear SAM, electrolytically corroding multilayer PCB and latching high-side MOSFET drivers on ($1,500 per SAM)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "Disconnect battery ground terminal to prevent thermal runaway / battery drain. Remove Front SAM fuse box (under hood driver side) and inspect underside connectors for green copper verdigris corrosion. Unclog cowl drain duckbills."
                } else {
                    "Inspect and clean windshield cowl rubber drain flaps and rear taillight foam seals. Ensure CAN-B reaches sleep state (< 40mA quiescent draw after 2 minutes)."
                }.to_string(),
                oem_part_numbers: vec!["A 211 545 42 01 (Front SAM)".to_string(), "A 211 545 93 01 (Rear SAM)".to_string(), "A 211 830 00 97 (Cowl Drain Valve)".to_string()],
                affected_chassis: vec!["W211 E-Class / S211 Estate".to_string(), "W219 CLS-Class".to_string(), "W164 ML-Class (Rear SAM water ingress via taillight gasket)".to_string(), "W204 C-Class".to_string()],
            })
        } else {
            None
        }
    }

    fn check_om642_oil_cooler(input: &CascadeTelemetryInput) -> Option<CascadeAlert> {
        let mut severity = CascadeSeverity::Normal;
        let mut evidence = Vec::new();

        if let Some(rate) = input.dynamic_oil_loss_rate_mm_100km {
            if rate >= 0.25 {
                severity = severity.max(CascadeSeverity::ImminentDanger);
                evidence.push(format!(
                    "Dynamic highway oil level drop rate severe ({:.2} mm/100km >= 0.25 mm/100km; V-valley oil cooler seal failure)",
                    rate
                ));
            } else if rate >= 0.10 {
                severity = severity.max(CascadeSeverity::Watchlist);
                evidence.push(format!(
                    "Elevated dynamic oil consumption ({:.2} mm/100km >= 0.10 mm/100km; early valley seal leakage)",
                    rate
                ));
            }
        }

        if input
            .active_dtcs
            .iter()
            .any(|d| d.contains("P2526") || d.contains("P2527"))
        {
            severity = severity.max(CascadeSeverity::Watchlist);
            evidence.push(
                "Oil level sensor implausibility or dynamic level monitoring DTC stored"
                    .to_string(),
            );
        }

        if severity > CascadeSeverity::Normal {
            Some(CascadeAlert {
                id: CascadeId::Om642OilCoolerValleyLeak,
                name: "OM642 V-Valley Oil Cooler Seal Degradation & Starvation".to_string(),
                severity,
                root_cause_part: "Original orange silicone oil cooler seals A 642 188 01 80 bake rock-hard in deep engine V-valley ($4.50)".to_string(),
                catastrophic_outcome: "Engine oil drains out through transmission bellhousing weep holes during sustained highway driving; sudden oil starvation spins connecting rod bearings and seizes crank ($7,500+)".to_string(),
                telemetry_evidence: evidence.join("; "),
                recommendation: if severity == CascadeSeverity::ImminentDanger {
                    "STOP DRIVING: Inspect transmission bellhousing drainage hole behind engine oil pan. Check engine oil dipstick immediately. Do not operate under highway loads. Replace cooler seals with updated purple Viton seals (A 642 188 05 80)."
                } else {
                    "Check bellhousing inspection plug for oil residue. Replace orange silicone seals with improved purple Viton seals (A 642 188 05 80) and replace turbo inlet gasket (A 642 094 00 80) to prevent oil dripping onto swirl flap motor."
                }.to_string(),
                oem_part_numbers: vec!["A 642 188 05 80 (Viton Oil Cooler Seal Kit)".to_string(), "A 642 094 00 80 (Turbo Inlet Seal)".to_string(), "A 642 180 00 09 (Oil Cooler Housing)".to_string()],
                affected_chassis: vec!["W211 / S211 E280/E300/E320 CDI (OM642 3.0L V6)".to_string(), "W164 ML320 CDI / GL320 CDI".to_string(), "W221 S320 CDI".to_string(), "Sprinter NCV3 (OM642)".to_string()],
            })
        } else {
            None
        }
    }
}
