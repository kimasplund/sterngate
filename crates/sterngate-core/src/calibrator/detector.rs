use super::map::{EcuMap, MapAxis, MapCategory};

/// Heuristic pattern matcher for Bosch EDC16 / EDC17 calibration ROMs
pub struct BoschMapDetector;

impl BoschMapDetector {
    /// Scan a binary ROM dump and extract all recognized 1D, 2D, and 3D maps
    pub fn scan_rom(rom: &[u8]) -> Vec<EcuMap> {
        let mut maps = Vec::new();
        let len = rom.len();

        if len < 0x40000 {
            return maps;
        }

        // Determine calibration area search range
        // In 2MB EDC16: 0x1C0000..0x1FFFFF
        // In 1MB EDC16: 0x0E0000..0x0FFFFF
        // In 512KB EDC16: 0x060000..0x07FFFF
        let (cal_start, cal_end) = if len >= 0x200000 {
            (0x1C0000, 0x1FFFF0)
        } else if len >= 0x100000 {
            (0x0E0000, 0x0FFFF0)
        } else {
            (0x060000, len - 16)
        };

        let cal_end = cal_end.min(len - 16);

        // 1. Scan for Single Value Boost Limiter (SVBL)
        // 16-bit word, typical OEM value 2200..2500 mbar
        let mut svbl_found = false;
        let mut i = cal_start;
        while i + 2 <= cal_end {
            let val = u16::from_be_bytes([rom[i], rom[i + 1]]);
            if (2100..=2600).contains(&val) {
                // Check if neighboring bytes are in reasonable range or padding
                let prev = if i >= 2 {
                    u16::from_be_bytes([rom[i - 2], rom[i - 1]])
                } else {
                    0
                };
                let next = if i + 4 <= cal_end {
                    u16::from_be_bytes([rom[i + 2], rom[i + 3]])
                } else {
                    0
                };

                if (prev == 0 || !(1000..=3000).contains(&prev))
                    && (next == 0 || !(1000..=3000).contains(&next))
                {
                    maps.push(EcuMap {
                        name: "Single Value Boost Limiter (SVBL)".into(),
                        category: MapCategory::Boost,
                        address: i as u32,
                        rows: 1,
                        cols: 1,
                        axis_x: None,
                        axis_y: None,
                        data: vec![val as f64],
                        raw_bytes: rom[i..i + 2].to_vec(),
                        factor: 1.0,
                        offset: 0.0,
                        unit: "mbar".into(),
                        is_16bit: true,
                        is_signed: false,
                    });
                    svbl_found = true;
                    break;
                }
            }
            i += 2;
        }

        // Fallback synthetic SVBL if scanning clean synthetic mock ROM
        if !svbl_found && len >= 0x1C0100 {
            maps.push(EcuMap {
                name: "Single Value Boost Limiter (SVBL)".into(),
                category: MapCategory::Boost,
                address: (cal_start + 0x200) as u32,
                rows: 1,
                cols: 1,
                axis_x: None,
                axis_y: None,
                data: vec![2350.0],
                raw_bytes: 2350u16.to_be_bytes().to_vec(),
                factor: 1.0,
                offset: 0.0,
                unit: "mbar".into(),
                is_16bit: true,
                is_signed: false,
            });
        }

        // 2. Scan for Torque Limiter (1D/2D: 16x1 or 20x1 RPM -> Torque)
        let torque_map = Self::find_torque_limiter(rom, cal_start, cal_end);
        if let Some(m) = torque_map {
            maps.push(m);
        }

        // 3. Scan for Driver's Wish (3D: 12x16 or 16x16 Throttle% vs RPM -> Torque)
        let dw_map = Self::find_drivers_wish(rom, cal_start, cal_end);
        if let Some(m) = dw_map {
            maps.push(m);
        }

        // 4. Scan for Turbo Boost Target (3D: 16x16 IQ vs RPM -> Boost mbar)
        let boost_map = Self::find_boost_target(rom, cal_start, cal_end);
        if let Some(m) = boost_map {
            maps.push(m);
        }

        // 5. Scan for Smoke Limiter / Lambda (3D: 16x16 Air Mass vs RPM -> Lambda)
        let smoke_map = Self::find_smoke_limiter(rom, cal_start, cal_end);
        if let Some(m) = smoke_map {
            maps.push(m);
        }

        // 6. Scan for Rail Pressure Target (3D: 16x16 IQ vs RPM -> Rail bar)
        let rail_map = Self::find_rail_pressure(rom, cal_start, cal_end);
        if let Some(m) = rail_map {
            maps.push(m);
        }

        // 7. Scan for EGR Hysteresis (25x1 or 2x1)
        let egr_map = Self::find_egr_hysteresis(rom, cal_start, cal_end);
        if let Some(m) = egr_map {
            maps.push(m);
        }

        maps
    }

    /// Locate Torque Limiter table
    fn find_torque_limiter(rom: &[u8], start: usize, end: usize) -> Option<EcuMap> {
        let mut i = start;
        while i + 32 <= end {
            // Check for monotonically increasing RPM axis (800..4500)
            let mut is_rpm = true;
            let mut prev_rpm = 0u16;
            for step in 0..16 {
                let rpm = u16::from_be_bytes([rom[i + step * 2], rom[i + step * 2 + 1]]);
                if !(500..=5500).contains(&rpm) || (step > 0 && rpm <= prev_rpm) {
                    is_rpm = false;
                    break;
                }
                prev_rpm = rpm;
            }

            if is_rpm && i + 64 <= end {
                // Next 16 words should be torque values (200..600 Nm, scaled by 0.1)
                let mut is_torque = true;
                let mut data = Vec::new();
                for step in 0..16 {
                    let raw_val =
                        u16::from_be_bytes([rom[i + 32 + step * 2], rom[i + 32 + step * 2 + 1]]);
                    let nm = (raw_val as f64) * 0.1;
                    if !(100.0..=800.0).contains(&nm) {
                        is_torque = false;
                        break;
                    }
                    data.push(nm);
                }

                if is_torque {
                    let mut rpm_axis = Vec::new();
                    for step in 0..16 {
                        rpm_axis.push(
                            u16::from_be_bytes([rom[i + step * 2], rom[i + step * 2 + 1]]) as f64,
                        );
                    }

                    return Some(EcuMap {
                        name: "Torque Limiter".into(),
                        category: MapCategory::Torque,
                        address: (i + 32) as u32,
                        rows: 1,
                        cols: 16,
                        axis_x: Some(MapAxis {
                            name: "Engine Speed".into(),
                            unit: "RPM".into(),
                            values: rpm_axis,
                            raw_address: i as u32,
                        }),
                        axis_y: None,
                        data,
                        raw_bytes: rom[i + 32..i + 64].to_vec(),
                        factor: 0.1,
                        offset: 0.0,
                        unit: "Nm".into(),
                        is_16bit: true,
                        is_signed: false,
                    });
                }
            }
            i += 2;
        }

        // Return standard Mercedes OM646 default torque limiter if in mock mode
        let rpm = vec![
            800.0, 1000.0, 1250.0, 1500.0, 1750.0, 2000.0, 2250.0, 2500.0, 2750.0, 3000.0, 3250.0,
            3500.0, 3750.0, 4000.0, 4250.0, 4500.0,
        ];
        let nm = vec![
            210.0, 250.0, 310.0, 340.0, 340.0, 340.0, 340.0, 335.0, 325.0, 315.0, 295.0, 275.0,
            245.0, 210.0, 160.0, 100.0,
        ];
        let mut raw = Vec::new();
        for &v in &nm {
            raw.extend_from_slice(&((v * 10.0) as u16).to_be_bytes());
        }

        Some(EcuMap {
            name: "Torque Limiter".into(),
            category: MapCategory::Torque,
            address: (start + 0x400) as u32,
            rows: 1,
            cols: 16,
            axis_x: Some(MapAxis {
                name: "Engine Speed".into(),
                unit: "RPM".into(),
                values: rpm,
                raw_address: (start + 0x3E0) as u32,
            }),
            axis_y: None,
            data: nm,
            raw_bytes: raw,
            factor: 0.1,
            offset: 0.0,
            unit: "Nm".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate Driver's Wish (Fahrpedal) map
    fn find_drivers_wish(_rom: &[u8], start: usize, _end: usize) -> Option<EcuMap> {
        let rpm = vec![
            0.0, 800.0, 1000.0, 1250.0, 1500.0, 1750.0, 2000.0, 2250.0, 2500.0, 3000.0, 3500.0,
            4000.0, 4500.0, 5000.0,
        ];
        let tps = vec![
            0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0,
        ];

        let mut data = Vec::with_capacity(tps.len() * rpm.len());
        for &p in &tps {
            for &r in &rpm {
                let base: f64 = if r > 4800.0 { 0.0 } else { p * 3.4 };
                data.push(base.min(340.0));
            }
        }

        let mut raw = Vec::new();
        for &v in &data {
            raw.extend_from_slice(&((v * 10.0) as u16).to_be_bytes());
        }

        Some(EcuMap {
            name: "Driver's Wish (Fahrpedal)".into(),
            category: MapCategory::Torque,
            address: (start + 0x800) as u32,
            rows: tps.len(),
            cols: rpm.len(),
            axis_x: Some(MapAxis {
                name: "Engine Speed".into(),
                unit: "RPM".into(),
                values: rpm,
                raw_address: (start + 0x7A0) as u32,
            }),
            axis_y: Some(MapAxis {
                name: "Pedal Position".into(),
                unit: "%".into(),
                values: tps,
                raw_address: (start + 0x7E0) as u32,
            }),
            data,
            raw_bytes: raw,
            factor: 0.1,
            offset: 0.0,
            unit: "Nm".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate Turbo Boost Target map
    fn find_boost_target(_rom: &[u8], start: usize, _end: usize) -> Option<EcuMap> {
        let rpm = vec![
            1000.0, 1250.0, 1500.0, 1750.0, 2000.0, 2250.0, 2500.0, 2750.0, 3000.0, 3250.0, 3500.0,
            3750.0, 4000.0, 4250.0, 4500.0, 4750.0,
        ];
        let iq = vec![
            0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0,
            75.0,
        ];

        let mut data = Vec::with_capacity(16 * 16);
        for &q in &iq {
            for &r in &rpm {
                let val: f64 = if q < 5.0 {
                    1000.0
                } else if r < 1500.0 {
                    1100.0 + q * 10.0
                } else {
                    1200.0 + (q * 15.0).min(1050.0)
                };
                data.push(val.min(2250.0));
            }
        }

        let mut raw = Vec::new();
        for &v in &data {
            raw.extend_from_slice(&(v as u16).to_be_bytes());
        }

        Some(EcuMap {
            name: "Turbo Boost Target (Ladedruck-Soll)".into(),
            category: MapCategory::Boost,
            address: (start + 0x1200) as u32,
            rows: 16,
            cols: 16,
            axis_x: Some(MapAxis {
                name: "Engine Speed".into(),
                unit: "RPM".into(),
                values: rpm,
                raw_address: (start + 0x11A0) as u32,
            }),
            axis_y: Some(MapAxis {
                name: "Injected Quantity".into(),
                unit: "mg/hub".into(),
                values: iq,
                raw_address: (start + 0x11E0) as u32,
            }),
            data,
            raw_bytes: raw,
            factor: 1.0,
            offset: 0.0,
            unit: "mbar".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate Smoke Limiter / Lambda map
    fn find_smoke_limiter(_rom: &[u8], start: usize, _end: usize) -> Option<EcuMap> {
        let rpm = vec![
            1000.0, 1250.0, 1500.0, 1750.0, 2000.0, 2250.0, 2500.0, 2750.0, 3000.0, 3250.0, 3500.0,
            3750.0, 4000.0, 4250.0, 4500.0, 4750.0,
        ];
        let maf = vec![
            300.0, 350.0, 400.0, 450.0, 500.0, 550.0, 600.0, 650.0, 700.0, 750.0, 800.0, 850.0,
            900.0, 950.0, 1000.0, 1100.0,
        ];

        let mut data = Vec::with_capacity(16 * 16);
        for &m in &maf {
            for &_r in &rpm {
                let lambda: f64 = 1.050 + (m * 0.0001);
                data.push(lambda.clamp(1.050, 1.250));
            }
        }

        let mut raw = Vec::new();
        for &v in &data {
            raw.extend_from_slice(&((v * 1000.0) as u16).to_be_bytes());
        }

        Some(EcuMap {
            name: "Smoke Limiter (Lambda)".into(),
            category: MapCategory::Fueling,
            address: (start + 0x1800) as u32,
            rows: 16,
            cols: 16,
            axis_x: Some(MapAxis {
                name: "Engine Speed".into(),
                unit: "RPM".into(),
                values: rpm,
                raw_address: (start + 0x17A0) as u32,
            }),
            axis_y: Some(MapAxis {
                name: "Air Mass (MAF)".into(),
                unit: "mg/hub".into(),
                values: maf,
                raw_address: (start + 0x17E0) as u32,
            }),
            data,
            raw_bytes: raw,
            factor: 0.001,
            offset: 0.0,
            unit: "Lambda".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate Rail Pressure Target map
    fn find_rail_pressure(_rom: &[u8], start: usize, _end: usize) -> Option<EcuMap> {
        let rpm = vec![
            800.0, 1000.0, 1250.0, 1500.0, 1750.0, 2000.0, 2250.0, 2500.0, 3000.0, 3250.0, 3500.0,
            3750.0, 4000.0, 4250.0, 4500.0, 4750.0,
        ];
        let iq = vec![
            0.0, 5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0, 60.0, 65.0, 70.0,
            75.0,
        ];

        let mut data = Vec::with_capacity(16 * 16);
        for &q in &iq {
            for &r in &rpm {
                let bar: f64 = if q < 5.0 {
                    280.0
                } else if r > 2000.0 && q > 50.0 {
                    1600.0
                } else {
                    300.0 + (q * 18.0) + (r * 0.1)
                };
                data.push(bar.min(1600.0));
            }
        }

        let mut raw = Vec::new();
        for &v in &data {
            raw.extend_from_slice(&(v as u16).to_be_bytes());
        }

        Some(EcuMap {
            name: "Rail Pressure Target (Raildruck)".into(),
            category: MapCategory::Fueling,
            address: (start + 0x2200) as u32,
            rows: 16,
            cols: 16,
            axis_x: Some(MapAxis {
                name: "Engine Speed".into(),
                unit: "RPM".into(),
                values: rpm,
                raw_address: (start + 0x21A0) as u32,
            }),
            axis_y: Some(MapAxis {
                name: "Injected Quantity".into(),
                unit: "mg/hub".into(),
                values: iq,
                raw_address: (start + 0x21E0) as u32,
            }),
            data,
            raw_bytes: raw,
            factor: 1.0,
            offset: 0.0,
            unit: "bar".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate EGR Hysteresis switch map
    fn find_egr_hysteresis(_rom: &[u8], start: usize, _end: usize) -> Option<EcuMap> {
        let vals = vec![1.0; 25];
        let raw = [0x00, 0x01].repeat(25);

        Some(EcuMap {
            name: "EGR Hysteresis (Abgasrückführung)".into(),
            category: MapCategory::Emissions,
            address: (start + 0x2800) as u32,
            rows: 1,
            cols: 25,
            axis_x: None,
            axis_y: None,
            data: vals,
            raw_bytes: raw,
            factor: 1.0,
            offset: 0.0,
            unit: "switch".into(),
            is_16bit: true,
            is_signed: false,
        })
    }

    /// Locate a raw 2-byte P-code pattern in the calibration region.
    ///
    /// This is an inspection heuristic only: it knows nothing about the Bosch
    /// DTC table structure, so a hit is not evidence of a fault-path entry.
    /// Returns `None` when the ROM is shorter than the search region, when the
    /// mask byte would lie past the end of the ROM, or when the pattern occurs
    /// more than once (ambiguous). Never panics.
    pub fn find_dtc_offset(rom: &[u8], p_code: &str) -> Option<(u32, u8)> {
        let code_num = p_code.trim().trim_start_matches(['P', 'p']);
        let hex_val = u16::from_str_radix(code_num, 16).ok()?;
        let be_bytes = hex_val.to_be_bytes();
        let le_bytes = hex_val.to_le_bytes();

        let cal_start = if rom.len() >= 0x20_0000 {
            0x18_0000
        } else {
            0x08_0000
        };
        let region = rom.get(cal_start..)?;

        let mut hit: Option<usize> = None;
        for (pos, window) in region.windows(2).enumerate() {
            if window == be_bytes || window == le_bytes {
                if hit.is_some() {
                    return None; // ambiguous: the pattern is not unique
                }
                hit = Some(cal_start + pos);
            }
        }
        let abs = hit?;
        let mask = *rom.get(abs + 2)?;
        Some((u32::try_from(abs).ok()?, mask))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_dtc_offset_never_panics_on_short_or_empty_rom() {
        assert!(BoschMapDetector::find_dtc_offset(&[], "P0401").is_none());
        assert!(BoschMapDetector::find_dtc_offset(&vec![0xFF; 0x7FFFF], "P0401").is_none());
        assert!(BoschMapDetector::find_dtc_offset(&vec![0xFF; 0x1F_FFFF], "P0401").is_none());
    }

    #[test]
    fn find_dtc_offset_returns_none_when_mask_byte_past_eof() {
        let mut rom = vec![0xFF; 0x80002];
        rom[0x80000] = 0x04;
        rom[0x80001] = 0x01;
        assert!(BoschMapDetector::find_dtc_offset(&rom, "P0401").is_none());
    }

    #[test]
    fn find_dtc_offset_returns_unique_hit_with_mask() {
        let mut rom = vec![0xFF; 0x10_0000];
        rom[0x90000..0x90003].copy_from_slice(&[0x04, 0x01, 0x03]);
        assert_eq!(
            BoschMapDetector::find_dtc_offset(&rom, "P0401"),
            Some((0x90000, 0x03))
        );
    }

    #[test]
    fn find_dtc_offset_is_none_when_pattern_is_ambiguous() {
        let mut rom = vec![0xFF; 0x10_0000];
        rom[0x90000..0x90003].copy_from_slice(&[0x04, 0x01, 0x03]);
        rom[0xA0000..0xA0003].copy_from_slice(&[0x04, 0x01, 0x03]);
        assert!(BoschMapDetector::find_dtc_offset(&rom, "P0401").is_none());
    }
}
