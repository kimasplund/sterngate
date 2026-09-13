use serde::{Deserialize, Serialize};

/// Telemetry reading for pneumatic suspension (ENR / AIRMATIC)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SuspensionSample {
    pub timestamp_ms: u64,
    pub left_rear_height_mm: f64,
    pub right_rear_height_mm: f64,
    pub compressor_active: bool,
    pub compressor_run_duration_s: f64,
    #[serde(default)]
    pub reservoir_pressure_bar: Option<f64>,
    #[serde(default)]
    pub compressor_temp_c: Option<f64>,
}

/// Evaluation verdict for suspension health
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspensionStatus {
    Healthy,
    AttentionNeeded,
    Warning,
    CriticalLeak,
}

/// Detailed predictive suspension diagnostic health report
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspensionHealthReport {
    pub status: SuspensionStatus,
    pub height_drop_rate_mm_per_hour: f64,
    pub max_height_asymmetry_mm: f64,
    pub max_compressor_continuous_run_s: f64,
    pub compressor_duty_cycle_pct: f64,
    pub findings: Vec<String>,
    pub recommendations: Vec<String>,
}

/// Predictive suspension leak detector (specifically optimized for S211 ENR & W211 AIRMATIC)
#[derive(Debug, Clone, Default)]
pub struct SuspensionLeakDetector {
    samples: Vec<SuspensionSample>,
}

impl SuspensionLeakDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_sample(&mut self, sample: SuspensionSample) {
        self.samples.push(sample);
    }

    /// Evaluate the captured samples for air spring leaks and compressor strain
    pub fn evaluate(&self) -> SuspensionHealthReport {
        if self.samples.is_empty() {
            return SuspensionHealthReport {
                status: SuspensionStatus::Healthy,
                height_drop_rate_mm_per_hour: 0.0,
                max_height_asymmetry_mm: 0.0,
                max_compressor_continuous_run_s: 0.0,
                compressor_duty_cycle_pct: 0.0,
                findings: vec!["No suspension telemetry samples recorded.".into()],
                recommendations: vec![],
            };
        }

        let first = &self.samples[0];
        let last = &self.samples[self.samples.len() - 1];
        let duration_ms = last.timestamp_ms.saturating_sub(first.timestamp_ms);
        let duration_hours = (duration_ms as f64) / 3_600_000.0;

        let mut max_asymmetry = 0.0f64;
        let mut max_continuous_run = 0.0f64;
        let mut compressor_active_count = 0usize;

        for s in &self.samples {
            let diff = (s.left_rear_height_mm - s.right_rear_height_mm).abs();
            if diff > max_asymmetry {
                max_asymmetry = diff;
            }
            if s.compressor_run_duration_s > max_continuous_run {
                max_continuous_run = s.compressor_run_duration_s;
            }
            if s.compressor_active {
                compressor_active_count += 1;
            }
        }

        let duty_cycle = (compressor_active_count as f64 / self.samples.len() as f64) * 100.0;

        // Calculate height drop rate over elapsed time
        let left_drop = first.left_rear_height_mm - last.left_rear_height_mm;
        let right_drop = first.right_rear_height_mm - last.right_rear_height_mm;
        let max_drop = left_drop.max(right_drop);

        let drop_rate_per_hour = if duration_hours > 0.01 {
            (max_drop / duration_hours).max(0.0)
        } else {
            0.0
        };

        let mut findings = Vec::new();
        let mut recommendations = Vec::new();
        let mut status = SuspensionStatus::Healthy;

        // Check 1: Continuous compressor run time
        if max_continuous_run > 60.0 {
            status = SuspensionStatus::CriticalLeak;
            findings.push(format!(
                "Compressor ran continuously for {:.1}s (OEM safety threshold: 45s). Risk of thermal cutout.",
                max_continuous_run
            ));
            recommendations.push("Inspect air supply lines and check compressor relay Hella 002 542 72 19 for sticking contacts.".into());
        } else if max_continuous_run > 40.0 {
            if status != SuspensionStatus::CriticalLeak {
                status = SuspensionStatus::Warning;
            }
            findings.push(format!(
                "Compressor continuous run time elevated at {:.1}s (normal: 10–25s).",
                max_continuous_run
            ));
        }

        // Check 2: Height drop rate (Stationary leak detection)
        if drop_rate_per_hour > 10.0 {
            status = SuspensionStatus::CriticalLeak;
            findings.push(format!(
                "Severe pneumatic leak detected: height dropping at {:.1} mm/hour.",
                drop_rate_per_hour
            ));
            recommendations.push("Inspect rear pneumatic bellows (A 211 320 09 25) with soapy water around bottom fold.".into());
        } else if drop_rate_per_hour > 4.0 {
            if status != SuspensionStatus::CriticalLeak {
                status = SuspensionStatus::Warning;
            }
            findings.push(format!(
                "Moderate air leak detected: height dropping at {:.1} mm/hour.",
                drop_rate_per_hour
            ));
            recommendations.push(
                "Check valve block brass fittings and air distribution lines for micro-leaks."
                    .into(),
            );
        }

        // Check 3: Left vs Right rear asymmetry
        if max_asymmetry > 20.0 {
            if status == SuspensionStatus::Healthy {
                status = SuspensionStatus::AttentionNeeded;
            }
            findings.push(format!(
                "Rear height imbalance detected: {:.1} mm difference between Left and Right.",
                max_asymmetry
            ));
            recommendations.push("Perform ENR level sensor calibration via UDS routine or inspect linkage ball joint.".into());
        }

        // Check 4: Compressor duty cycle
        if duty_cycle > 25.0 {
            if status != SuspensionStatus::CriticalLeak {
                status = SuspensionStatus::Warning;
            }
            findings.push(format!(
                "High compressor duty cycle: {:.1}% active time during observation.",
                duty_cycle
            ));
            recommendations.push("Inspect compressor air intake filter (A 220 320 00 04) and valve block; high duty cycle causes piston ring wear.".into());
        }

        if findings.is_empty() {
            findings
                .push("Pneumatic suspension holding pressure normally. No leaks detected.".into());
            findings.push(format!(
                "Compressor duty cycle healthy at {:.1}%.",
                duty_cycle
            ));
        }

        SuspensionHealthReport {
            status,
            height_drop_rate_mm_per_hour: drop_rate_per_hour,
            max_height_asymmetry_mm: max_asymmetry,
            max_compressor_continuous_run_s: max_continuous_run,
            compressor_duty_cycle_pct: duty_cycle,
            findings,
            recommendations,
        }
    }
}

/// Instantaneous drive telemetry sample
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DriveSample {
    pub timestamp_ms: u64,
    pub speed_kmh: f64,
    pub engine_rpm: f64,
    pub injection_mass_mg_str: f64,
    pub boost_pressure_hpa: f64,
    pub rail_pressure_bar: f64,
    pub coolant_temp_c: f64,
    pub tcc_slip_rpm: f64,
    #[serde(default)]
    pub gear: u8,
}

impl DriveSample {
    /// Calculate instant fuel rate in Liters per Hour (for OM646 4-cyl 4-stroke diesel, density ~0.835 kg/L)
    pub fn instant_fuel_rate_lph(&self) -> f64 {
        if self.engine_rpm <= 0.0 || self.injection_mass_mg_str <= 0.0 {
            return 0.0;
        }
        // 4 cylinders, 4-stroke engine -> 2 revolutions per power stroke per cylinder
        // Total strokes per minute = (RPM / 2) * 4 = RPM * 2
        let strokes_per_min = self.engine_rpm * 2.0;
        let mg_per_min = strokes_per_min * self.injection_mass_mg_str;
        let grams_per_hour = (mg_per_min * 60.0) / 1000.0;
        // Diesel density ~ 835 g/L
        grams_per_hour / 835.0
    }

    /// Calculate instant fuel consumption in Liters per 100km
    pub fn instant_consumption_l_per_100km(&self) -> f64 {
        if self.speed_kmh < 5.0 {
            return 0.0; // Stationary or creeping
        }
        let lph = self.instant_fuel_rate_lph();
        (lph / self.speed_kmh) * 100.0
    }
}

/// Aggregated drive statistics for a trip or benchmark run
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveSummary {
    pub duration_seconds: f64,
    pub distance_km: f64,
    pub average_speed_kmh: f64,
    pub average_consumption_l_per_100km: f64,
    pub average_rpm: f64,
    pub max_boost_hpa: f64,
    pub average_rail_pressure_bar: f64,
    pub average_tcc_slip_rpm: f64,
    pub final_coolant_temp_c: f64,
    pub seconds_to_reach_85c: Option<f64>,
}

/// A/B comparative benchmark report comparing two drive runs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DriveComparison {
    pub run_a_name: String,
    pub run_b_name: String,
    pub consumption_delta_l_per_100km: f64,
    pub consumption_pct_change: f64,
    pub tcc_slip_delta_rpm: f64,
    pub boost_delta_hpa: f64,
    pub warmup_time_delta_seconds: Option<f64>,
    pub verdict: String,
    pub details: Vec<String>,
}

/// In-flight drive data processor and A/B benchmark engine
#[derive(Debug, Clone, Default)]
pub struct DriveBenchmark {
    samples: Vec<DriveSample>,
}

impl DriveBenchmark {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_sample(&mut self, sample: DriveSample) {
        self.samples.push(sample);
    }

    /// Generate summary metrics for the recorded drive
    pub fn summarize(&self) -> DriveSummary {
        if self.samples.is_empty() {
            return DriveSummary {
                duration_seconds: 0.0,
                distance_km: 0.0,
                average_speed_kmh: 0.0,
                average_consumption_l_per_100km: 0.0,
                average_rpm: 0.0,
                max_boost_hpa: 0.0,
                average_rail_pressure_bar: 0.0,
                average_tcc_slip_rpm: 0.0,
                final_coolant_temp_c: 0.0,
                seconds_to_reach_85c: None,
            };
        }

        let first_ts = self.samples[0].timestamp_ms;
        let last_ts = self.samples[self.samples.len() - 1].timestamp_ms;
        let duration_s = (last_ts.saturating_sub(first_ts) as f64) / 1000.0;

        let mut total_speed = 0.0;
        let mut total_rpm = 0.0;
        let mut total_rail = 0.0;
        let mut total_slip = 0.0;
        let mut max_boost = 0.0;
        let mut total_fuel_l = 0.0;
        let mut total_dist_km = 0.0;
        let mut time_to_85c = None;

        for i in 0..self.samples.len() {
            let s = &self.samples[i];
            total_speed += s.speed_kmh;
            total_rpm += s.engine_rpm;
            total_rail += s.rail_pressure_bar;
            total_slip += s.tcc_slip_rpm;
            if s.boost_pressure_hpa > max_boost {
                max_boost = s.boost_pressure_hpa;
            }

            if time_to_85c.is_none() && s.coolant_temp_c >= 85.0 {
                time_to_85c = Some((s.timestamp_ms.saturating_sub(first_ts) as f64) / 1000.0);
            }

            if i > 0 {
                let dt_hours = (s
                    .timestamp_ms
                    .saturating_sub(self.samples[i - 1].timestamp_ms)
                    as f64)
                    / 3_600_000.0;
                let step_dist = s.speed_kmh * dt_hours;
                total_dist_km += step_dist;
                let step_fuel = s.instant_fuel_rate_lph() * dt_hours;
                total_fuel_l += step_fuel;
            }
        }

        let count = self.samples.len() as f64;
        let avg_speed = total_speed / count;
        let avg_consumption = if total_dist_km > 0.05 {
            (total_fuel_l / total_dist_km) * 100.0
        } else {
            0.0
        };

        DriveSummary {
            duration_seconds: duration_s,
            distance_km: total_dist_km,
            average_speed_kmh: avg_speed,
            average_consumption_l_per_100km: avg_consumption,
            average_rpm: total_rpm / count,
            max_boost_hpa: max_boost,
            average_rail_pressure_bar: total_rail / count,
            average_tcc_slip_rpm: total_slip / count,
            final_coolant_temp_c: self.samples.last().map(|s| s.coolant_temp_c).unwrap_or(0.0),
            seconds_to_reach_85c: time_to_85c,
        }
    }

    /// Compare Run A (Baseline) vs Run B (Modified / After change)
    pub fn compare(
        run_a: &DriveSummary,
        run_b: &DriveSummary,
        run_a_name: &str,
        run_b_name: &str,
    ) -> DriveComparison {
        let consumption_delta =
            run_b.average_consumption_l_per_100km - run_a.average_consumption_l_per_100km;
        let consumption_pct = if run_a.average_consumption_l_per_100km > 0.0 {
            (consumption_delta / run_a.average_consumption_l_per_100km) * 100.0
        } else {
            0.0
        };

        let slip_delta = run_b.average_tcc_slip_rpm - run_a.average_tcc_slip_rpm;
        let boost_delta = run_b.max_boost_hpa - run_a.max_boost_hpa;

        let warmup_delta = match (run_a.seconds_to_reach_85c, run_b.seconds_to_reach_85c) {
            (Some(a), Some(b)) => Some(b - a),
            _ => None,
        };

        let mut details = Vec::new();
        if consumption_delta < -0.2 {
            details.push(format!(
                "Fuel efficiency improved by {:.1}% ({:.2} L/100km reduction).",
                consumption_pct.abs(),
                consumption_delta.abs()
            ));
        } else if consumption_delta > 0.2 {
            details.push(format!(
                "Fuel consumption increased by {:.1}% (+{:.2} L/100km).",
                consumption_pct, consumption_delta
            ));
        } else {
            details.push(
                "Fuel consumption remained virtually unchanged (within ±0.2 L/100km).".into(),
            );
        }

        if slip_delta < -5.0 {
            details.push(format!(
                "Torque converter lockup slip decreased by {:.1} RPM (improved mechanical coupling).",
                slip_delta.abs()
            ));
        } else if slip_delta > 5.0 {
            details.push(format!(
                "Torque converter clutch slip increased by {:.1} RPM.",
                slip_delta
            ));
        }

        if let Some(wd) = warmup_delta {
            if wd < -30.0 {
                details.push(format!(
                    "Engine warmed up to 85°C operating temp {:.0}s faster.",
                    wd.abs()
                ));
            } else if wd > 30.0 {
                details.push(format!(
                    "Engine took {:.0}s longer to reach 85°C operating temp (check thermostat).",
                    wd
                ));
            }
        }

        let verdict = if consumption_delta < -0.2 || slip_delta < -8.0 {
            "Beneficial: Changes improved efficiency or transmission coupling.".into()
        } else if consumption_delta > 0.5 || slip_delta > 15.0 {
            "Detrimental: Observed increased fuel consumption or transmission slip.".into()
        } else {
            "Neutral: No statistically significant change in fuel efficiency or slip.".into()
        };

        DriveComparison {
            run_a_name: run_a_name.to_string(),
            run_b_name: run_b_name.to_string(),
            consumption_delta_l_per_100km: consumption_delta,
            consumption_pct_change: consumption_pct,
            tcc_slip_delta_rpm: slip_delta,
            boost_delta_hpa: boost_delta,
            warmup_time_delta_seconds: warmup_delta,
            verdict,
            details,
        }
    }
}

/// Operational state of the suspension air compressor
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompressorOperationalState {
    /// Compressor is off and ready
    Idle,
    /// Compressor is currently active
    Running { continuous_run_seconds: f64 },
    /// Manually or diagnostics inhibited (safe mode)
    Inhibited { reason: String },
    /// Autonomous thermal watchdog triggered cutoff to prevent burnout
    ThermalCutoffTriggered {
        continuous_run_seconds: f64,
        cooldown_remaining_seconds: f64,
    },
}

/// Action commanded by the compressor watchdog
#[derive(Debug, Clone, PartialEq)]
pub enum CompressorGuardAction {
    None,
    /// Must send UDS Routine 0x0210 / 0x0211 to cut power immediately
    TripCutoff {
        run_duration_s: f64,
        reason: String,
    },
    /// Normal operation can safely resume
    RestoreAllowed,
}

/// Compressor protection watchdog preventing thermal overload and relay welding
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressorProtectionGuard {
    pub max_continuous_run_seconds: f64,
    pub cooldown_period_seconds: f64,
    pub watchdog_enabled: bool,
    pub is_inhibited: bool,
    pub inhibit_reason: Option<String>,
    pub current_state: CompressorOperationalState,
    #[serde(skip)]
    active_start_timestamp_ms: Option<u64>,
    #[serde(skip)]
    last_trip_timestamp_ms: Option<u64>,
}

impl Default for CompressorProtectionGuard {
    fn default() -> Self {
        Self::new(40.0, 180.0)
    }
}

impl CompressorProtectionGuard {
    pub fn new(max_continuous_s: f64, cooldown_s: f64) -> Self {
        Self {
            max_continuous_run_seconds: max_continuous_s,
            cooldown_period_seconds: cooldown_s,
            watchdog_enabled: true,
            is_inhibited: false,
            inhibit_reason: None,
            current_state: CompressorOperationalState::Idle,
            active_start_timestamp_ms: None,
            last_trip_timestamp_ms: None,
        }
    }

    /// Update with live CAN telemetry sample
    pub fn update(&mut self, timestamp_ms: u64, is_active: bool) -> CompressorGuardAction {
        // If manually inhibited, remain inhibited
        if self.is_inhibited {
            self.current_state = CompressorOperationalState::Inhibited {
                reason: self
                    .inhibit_reason
                    .clone()
                    .unwrap_or_else(|| "Safe mode manual inhibit".into()),
            };
            return CompressorGuardAction::None;
        }

        // Check cooldown from previous thermal trip
        if let Some(trip_ts) = self.last_trip_timestamp_ms {
            let elapsed_s = (timestamp_ms.saturating_sub(trip_ts) as f64) / 1000.0;
            if elapsed_s < self.cooldown_period_seconds {
                let remaining = (self.cooldown_period_seconds - elapsed_s).max(0.0);
                self.current_state = CompressorOperationalState::ThermalCutoffTriggered {
                    continuous_run_seconds: self.max_continuous_run_seconds,
                    cooldown_remaining_seconds: remaining,
                };
                return CompressorGuardAction::None;
            } else {
                // Cooldown elapsed
                self.last_trip_timestamp_ms = None;
                self.current_state = CompressorOperationalState::Idle;
            }
        }

        if is_active {
            let start = *self.active_start_timestamp_ms.get_or_insert(timestamp_ms);
            let duration_s = (timestamp_ms.saturating_sub(start) as f64) / 1000.0;
            self.current_state = CompressorOperationalState::Running {
                continuous_run_seconds: duration_s,
            };

            // Thermal watchdog trip check
            if self.watchdog_enabled && duration_s >= self.max_continuous_run_seconds {
                self.last_trip_timestamp_ms = Some(timestamp_ms);
                self.active_start_timestamp_ms = None;
                self.current_state = CompressorOperationalState::ThermalCutoffTriggered {
                    continuous_run_seconds: duration_s,
                    cooldown_remaining_seconds: self.cooldown_period_seconds,
                };
                return CompressorGuardAction::TripCutoff {
                    run_duration_s: duration_s,
                    reason: format!(
                        "Compressor exceeded OEM thermal continuous threshold ({:.1}s >= {:.1}s). Cutoff triggered to prevent motor burnout and relay welding.",
                        duration_s, self.max_continuous_run_seconds
                    ),
                };
            }
        } else {
            self.active_start_timestamp_ms = None;
            if self.last_trip_timestamp_ms.is_none() {
                self.current_state = CompressorOperationalState::Idle;
            }
        }

        CompressorGuardAction::None
    }

    /// Manually inhibit compressor (Safe Mode / Transport Mode)
    pub fn manual_inhibit(&mut self, reason: &str) -> CompressorGuardAction {
        self.is_inhibited = true;
        self.inhibit_reason = Some(reason.to_string());
        self.active_start_timestamp_ms = None;
        self.current_state = CompressorOperationalState::Inhibited {
            reason: reason.to_string(),
        };
        CompressorGuardAction::TripCutoff {
            run_duration_s: 0.0,
            reason: reason.to_string(),
        }
    }

    /// Restore normal automatic leveling compressor operation
    pub fn manual_restore(&mut self) -> CompressorGuardAction {
        self.is_inhibited = false;
        self.inhibit_reason = None;
        self.last_trip_timestamp_ms = None;
        self.active_start_timestamp_ms = None;
        self.current_state = CompressorOperationalState::Idle;
        CompressorGuardAction::RestoreAllowed
    }
}
