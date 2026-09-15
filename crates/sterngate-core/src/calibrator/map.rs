use serde::{Deserialize, Serialize};

/// Categorization of calibration maps in the engine management software
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapCategory {
    Torque,
    Boost,
    Fueling,
    Timing,
    Emissions,
    Diagnostics,
}

impl MapCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Torque => "Torque & Driver Request",
            Self::Boost => "Turbocharger & Boost Control",
            Self::Fueling => "Fuel Delivery & Rail Pressure",
            Self::Timing => "Injection Timing & Duration",
            Self::Emissions => "Emissions (EGR / DPF / AdBlue)",
            Self::Diagnostics => "Diagnostics & Fault Suppression",
        }
    }
}

/// Where a detected map's `address` and `raw_bytes` came from.
///
/// Only `Scanned` maps may ever be turned into flash writes. Every other
/// value marks data with no proven relation to the ROM it claims to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MapProvenance {
    /// Field absent or origin unknown (legacy packages). Never executable.
    #[default]
    Unverified,
    /// Hard-coded placeholder; address and bytes were never read from the ROM.
    Synthetic,
    /// A scan ran, found nothing, and a hard-coded default was substituted.
    Fallback,
    /// `raw_bytes` were read from `rom[address..]` by a pattern scan.
    Scanned,
}

impl MapProvenance {
    /// True for the serde default, used by `skip_serializing_if`.
    pub const fn is_unverified(&self) -> bool {
        matches!(self, Self::Unverified)
    }

    /// True only for maps located in the target's own ROM.
    pub const fn is_scanned(self) -> bool {
        matches!(self, Self::Scanned)
    }
}

/// Axis metadata for a 1D, 2D, or 3D calibration table
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapAxis {
    pub name: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub raw_address: u32,
}

/// A parsed 1D, 2D, or 3D ECU calibration map
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcuMap {
    pub name: String,
    pub category: MapCategory,
    pub provenance: MapProvenance,
    pub address: u32,
    pub rows: usize,
    pub cols: usize,
    pub axis_x: Option<MapAxis>,
    pub axis_y: Option<MapAxis>,
    pub data: Vec<f64>,
    pub raw_bytes: Vec<u8>,
    pub factor: f64,
    pub offset: f64,
    pub unit: String,
    pub is_16bit: bool,
    pub is_signed: bool,
}

impl EcuMap {
    /// Independent evidence check: true only when `raw_bytes` is non-empty and
    /// equals the ROM slice at `address`. The provenance label is not trusted.
    pub fn is_rom_backed(&self, rom: &[u8]) -> bool {
        if self.raw_bytes.is_empty() {
            return false;
        }
        let Ok(start) = usize::try_from(self.address) else {
            return false;
        };
        let Some(end) = start.checked_add(self.raw_bytes.len()) else {
            return false;
        };
        rom.get(start..end)
            .is_some_and(|slice| slice == self.raw_bytes.as_slice())
    }

    /// Apply a percentage modification across the whole map (e.g. +10% -> 1.10)
    /// with an optional ceiling/max clamp
    pub fn modify_percentage(&mut self, factor: f64, max_clamp: Option<f64>) {
        for val in &mut self.data {
            let mut new_val = *val * factor;
            if let Some(clamp) = max_clamp {
                if new_val > clamp {
                    new_val = clamp;
                }
            }
            *val = new_val;
        }
        self.recompute_raw_bytes();
    }

    /// Recompute raw 8-bit or 16-bit big-endian bytes from physical values
    pub fn recompute_raw_bytes(&mut self) {
        let mut raw = Vec::with_capacity(self.data.len() * if self.is_16bit { 2 } else { 1 });
        for &val in &self.data {
            let unscaled = ((val - self.offset) / self.factor).round();
            if self.is_16bit {
                let clamped = if self.is_signed {
                    unscaled.clamp(i16::MIN as f64, i16::MAX as f64) as i16 as u16
                } else {
                    unscaled.clamp(0.0, u16::MAX as f64) as u16
                };
                raw.extend_from_slice(&clamped.to_be_bytes());
            } else {
                let clamped = if self.is_signed {
                    unscaled.clamp(i8::MIN as f64, i8::MAX as f64) as i8 as u8
                } else {
                    unscaled.clamp(0.0, u8::MAX as f64) as u8
                };
                raw.push(clamped);
            }
        }
        self.raw_bytes = raw;
    }

    /// Format table as ASCII matrix for terminal inspection
    pub fn format_ascii_table(&self) -> String {
        let mut out = format!(
            "Map: {} (0x{:06X}, {}x{}, Unit: {})\n",
            self.name, self.address, self.rows, self.cols, self.unit
        );

        if let Some(ref y_axis) = self.axis_y {
            out.push_str(&format!("  Y-Axis ({} [{}])\n", y_axis.name, y_axis.unit));
        }

        // Column header
        out.push_str("        ");
        if let Some(ref x_axis) = self.axis_x {
            for v in &x_axis.values {
                out.push_str(&format!("{:>8.0} ", v));
            }
        } else {
            for c in 0..self.cols {
                out.push_str(&format!(" Col {:>2}  ", c + 1));
            }
        }
        out.push('\n');

        // Rows
        for r in 0..self.rows {
            if let Some(ref y_axis) = self.axis_y {
                let y_val = y_axis.values.get(r).copied().unwrap_or(r as f64);
                out.push_str(&format!("{:>7.1} ", y_val));
            } else {
                out.push_str(&format!("Row {:>2}  ", r + 1));
            }

            for c in 0..self.cols {
                let idx = r * self.cols + c;
                let val = self.data.get(idx).copied().unwrap_or(0.0);
                out.push_str(&format!("{:>8.1} ", val));
            }
            out.push('\n');
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_at(address: u32, raw_bytes: Vec<u8>) -> EcuMap {
        EcuMap {
            name: "t".into(),
            category: MapCategory::Boost,
            provenance: MapProvenance::Scanned,
            address,
            rows: 1,
            cols: 1,
            axis_x: None,
            axis_y: None,
            data: vec![],
            raw_bytes,
            factor: 1.0,
            offset: 0.0,
            unit: String::new(),
            is_16bit: true,
            is_signed: false,
        }
    }

    #[test]
    fn is_rom_backed_rejects_out_of_range_and_mismatch() {
        let mut rom = vec![0xFF; 64];
        rom[10..12].copy_from_slice(&[0x12, 0x34]);
        assert!(map_at(10, vec![0x12, 0x34]).is_rom_backed(&rom));
        assert!(!map_at(10, vec![0x12, 0x35]).is_rom_backed(&rom));
        assert!(!map_at(63, vec![0x12, 0x34]).is_rom_backed(&rom));
        assert!(!map_at(u32::MAX, vec![0x12]).is_rom_backed(&rom));
        assert!(!map_at(10, vec![]).is_rom_backed(&rom));
    }

    #[test]
    fn provenance_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&MapProvenance::Scanned).unwrap(),
            "\"scanned\""
        );
        assert_eq!(MapProvenance::default(), MapProvenance::Unverified);
        assert!(MapProvenance::Unverified.is_unverified());
        assert!(MapProvenance::Scanned.is_scanned());
        assert!(!MapProvenance::Fallback.is_scanned());
    }
}
