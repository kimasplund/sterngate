use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterValue {
    pub id: String,
    pub name: String,
    pub module: String,
    pub value: f64,
    pub raw_hex: String,
    pub unit: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub timestamp_ms: u64,
    /// `None` when nothing actually measured the battery. Consumers must treat
    /// an absent reading as a failed interlock, never as a passing default.
    pub battery_voltage: Option<f64>,
    pub engine_rpm: Option<f64>,
    pub coolant_temp: Option<f64>,
    pub trans_fluid_temp: Option<f64>,
    pub rail_pressure: Option<f64>,
    pub boost_pressure: Option<f64>,
    pub tcc_slip_rpm: Option<f64>,
    pub inj_corr_cyl1: Option<f64>,
    pub inj_corr_cyl2: Option<f64>,
    pub inj_corr_cyl3: Option<f64>,
    pub inj_corr_cyl4: Option<f64>,
    pub parameters: Vec<ParameterValue>,
}
