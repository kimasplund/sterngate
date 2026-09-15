pub fn hex_to_bytes(s: &str) -> Result<Vec<u8>, String> {
    let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if !clean.len().is_multiple_of(2) {
        return Err("Hex string must have an even length".into());
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex byte at position {}: {}", i, e))
        })
        .collect()
}

use sterngate_core::Result as SterngateResult;
use sterngate_hal::VehicleInterface;

/// A client-supplied voltage may not disagree with the adapter's own reading by more than this.
pub const VOLTAGE_CROSS_CHECK_TOLERANCE: f64 = 1.0;

/// Decide which voltage gates a write. The adapter's measurement wins; a client
/// value is accepted only when the adapter cannot measure, and refused when it
/// contradicts a measurement.
pub fn choose_voltage(
    measured: SterngateResult<Option<f32>>,
    client: Option<f64>,
) -> std::result::Result<f64, String> {
    match measured {
        Ok(Some(v)) => {
            let v = f64::from(v);
            if let Some(c) = client {
                if (c - v).abs() > VOLTAGE_CROSS_CHECK_TOLERANCE {
                    return Err(format!(
                        "Refusing: client-supplied battery voltage {c:.2} V disagrees with the adapter measurement {v:.2} V"
                    ));
                }
            }
            Ok(v)
        }
        Ok(None) => client.ok_or_else(|| {
            "Refusing: no measured battery voltage. This adapter cannot measure; send 'measured_voltage' from a real hardware reading.".to_string()
        }),
        Err(e) => Err(format!("Refusing: battery voltage read failed: {e}")),
    }
}

pub async fn resolve_flash_voltage(
    iface: &mut dyn VehicleInterface,
    client_value: Option<f64>,
) -> std::result::Result<f64, String> {
    choose_voltage(iface.measure_battery_voltage().await, client_value)
}

#[cfg(test)]
mod tests {
    use super::choose_voltage;
    use sterngate_core::SterngateError;

    #[test]
    fn measured_value_wins_and_cross_checks_client() {
        assert_eq!(
            choose_voltage(Ok(Some(12.7)), None).unwrap(),
            f64::from(12.7f32)
        );
        assert_eq!(
            choose_voltage(Ok(Some(12.7)), Some(12.9)).unwrap(),
            f64::from(12.7f32)
        );
        let err = choose_voltage(Ok(Some(12.7)), Some(14.5)).unwrap_err();
        assert!(err.contains("disagrees"), "{err}");
    }

    #[test]
    fn unmeasurable_adapter_requires_client_value() {
        assert_eq!(choose_voltage(Ok(None), Some(12.8)).unwrap(), 12.8);
        let err = choose_voltage(Ok(None), None).unwrap_err();
        assert!(err.contains("no measured battery voltage"), "{err}");
    }

    #[test]
    fn adapter_error_refuses() {
        let err =
            choose_voltage(Err(SterngateError::HalError("usb".into())), Some(12.8)).unwrap_err();
        assert!(err.contains("usb"), "{err}");
    }
}
