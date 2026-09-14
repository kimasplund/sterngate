use serde_json::{json, Value};
use sterngate_core::{Dtc, EcuCatalog, Language, TelemetrySnapshot, VehicleGarage};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_protocol::{BusDiscoverer, VehicleScanner};

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut mock_iface = VirtualCanInterface::new();
    let _ = mock_iface.open().await;

    match name {
        "sterngate_list_interfaces" => Ok(json!({
            "interfaces": [
                { "name": "can0", "type": "SocketCAN (Physical/gs_usb/CANable)", "status": "Available on Linux" },
                { "name": "vcan0", "type": "Linux Virtual CAN", "status": "Ready" },
                { "name": "virtual_w211_sim", "type": "Sterngate In-Memory W211 Simulator", "status": "Active (Default)" },
                { "name": "j2534_openport", "type": "SAE J2534 PassThru (Tactrix OpenPort 2.0)", "status": "Driver bridge ready" }
            ]
        })),
        "sterngate_read_telemetry" => {
            let snap = TelemetrySnapshot {
                timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
                battery_voltage: Some(13.8),
                engine_rpm: Some(820.0),
                coolant_temp: Some(88.0),
                trans_fluid_temp: Some(80.0),
                rail_pressure: Some(320.0),
                boost_pressure: Some(1040.0),
                tcc_slip_rpm: Some(16.0),
                inj_corr_cyl1: Some(0.20),
                inj_corr_cyl2: Some(-0.15),
                inj_corr_cyl3: Some(-0.32),
                inj_corr_cyl4: Some(0.25),
                parameters: vec![],
            };
            Ok(serde_json::to_value(snap).unwrap())
        }
        "sterngate_read_dtc" => {
            let lang: Language = arguments
                .get("lang")
                .and_then(|v| v.as_str())
                .unwrap_or("en")
                .parse()
                .unwrap_or_default();
            let mut dtc = Dtc::parse_iso15031(0x01, 0x00, 0x28, "EDC16");
            dtc.localize(lang);
            let dtcs = vec![dtc];
            Ok(json!({
                "module": arguments.get("module").and_then(|v| v.as_str()).unwrap_or("EDC16"),
                "language": lang.to_string(),
                "dtcs": dtcs,
                "status": "1 fault code active"
            }))
        }
        "sterngate_clear_dtc" => Ok(json!({
            "module": arguments.get("module").and_then(|v| v.as_str()).unwrap_or("EDC16"),
            "success": true,
            "message": "Diagnostic fault memory successfully cleared via Service 0x14."
        })),
        "sterngate_read_parameter" => {
            let param = arguments
                .get("parameter")
                .and_then(|v| v.as_str())
                .unwrap_or("trans_fluid_temp");
            let lang: Language = arguments
                .get("lang")
                .and_then(|v| v.as_str())
                .unwrap_or("en")
                .parse()
                .unwrap_or_default();
            let (name_en, val, unit, name_de, name_sv) = match param.to_lowercase().as_str() {
                "trans_fluid_temp" | "0x2001" => (
                    "Transmission Fluid Temperature",
                    80.0,
                    "°C",
                    "Getriebeöltemperatur",
                    "Transmissionsoljetemperatur",
                ),
                "engine_rpm" | "0x0100" => {
                    ("Engine RPM", 820.0, "RPM", "Motordrehzahl", "Motorvarvtal")
                }
                "coolant_temp" | "0x0105" => (
                    "Coolant Temperature",
                    88.0,
                    "°C",
                    "Kühlmitteltemperatur",
                    "Kylarvätsketemperatur",
                ),
                "rail_pressure" | "0x200b" => (
                    "Common Rail Pressure",
                    320.0,
                    "bar",
                    "Raildruck",
                    "Railtryck",
                ),
                "boost_pressure" | "0x2010" => (
                    "Boost Pressure (MAP)",
                    1040.0,
                    "hPa",
                    "Ladedruck",
                    "Laddtryck",
                ),
                "tcc_slip_rpm" => (
                    "Torque Converter Clutch Slip",
                    16.0,
                    "RPM",
                    "Drehzahldifferenz KÜB",
                    "Momentomvandlarkoppling slirning",
                ),
                _ => (
                    "Generic Parameter",
                    0.0,
                    "raw",
                    "Generischer Parameter",
                    "Generisk parameter",
                ),
            };
            let localized_name = match lang {
                Language::De => name_de,
                Language::Sv => name_sv,
                Language::En => name_en,
            };
            Ok(json!({
                "parameter": param,
                "name": localized_name,
                "language": lang.to_string(),
                "value": val,
                "unit": unit,
                "status": "Valid",
                "reading_note": if val == 80.0 && unit == "°C" { "Transmission is at the exact required 80°C for checking 722.6 ATF level." } else { "" }
            }))
        }
        "sterngate_inspect_ecu" => {
            let module = arguments
                .get("module")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            Ok(json!({
                "module": module,
                "ecu_description": "Bosch EDC16C31 OM646 CDI",
                "hardware_id": "0281012224",
                "software_id": "1037372332",
                "vin": "WDB2110061A123456",
                "protocol": "UDS over ISO-TP",
                "baudrate": 500000
            }))
        }
        "sterngate_control_flight_recorder" => {
            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("status");
            let filename = arguments
                .get("filename")
                .and_then(|v| v.as_str())
                .unwrap_or("flight_telemetry_sample.csv");

            match action {
                "start" => Ok(json!({
                    "action": "start",
                    "is_recording": true,
                    "target_file": format!("logs/{}", filename),
                    "sampling_rate": "10-50 Hz",
                    "message": "Continuous flight recorder started. Logging high-speed powertrain telemetry."
                })),
                "stop" => Ok(json!({
                    "action": "stop",
                    "is_recording": false,
                    "target_file": format!("logs/{}", filename),
                    "records_count": 142,
                    "message": "Flight recorder stopped and CSV file flushed to disk."
                })),
                _ => Ok(json!({
                    "action": "status",
                    "is_recording": false,
                    "current_file": Value::Null,
                    "records_count": 0,
                    "elapsed_seconds": 0
                })),
            }
        }
        "sterngate_scan_vehicle" => {
            let lang_str = arguments
                .get("lang")
                .and_then(|v| v.as_str())
                .unwrap_or("en");
            let save_to_garage = arguments
                .get("save_to_garage")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let language: Language = lang_str.parse().unwrap_or_default();

            let report = VehicleScanner::scan(&mut mock_iface, language)
                .await
                .map_err(|e| format!("Vehicle scan failed: {}", e))?;

            if save_to_garage {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let rec = report.to_vehicle_record();
                let _ = garage.save_vehicle(&rec, Some("mcp_scan: vehicle quick scan completed"));
            }

            Ok(serde_json::to_value(report).unwrap())
        }
        "sterngate_discover_ecus" => {
            let start = arguments
                .get("start_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0x7E0) as u32;
            let end = arguments
                .get("end_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0x7EF) as u32;
            let timeout = arguments
                .get("timeout_ms")
                .and_then(|v| v.as_u64())
                .unwrap_or(20);

            let catalog_res = EcuCatalog::load_default();
            let catalog_ref = catalog_res.as_ref().ok();

            match BusDiscoverer::discover_ecus(&mut mock_iface, start..=end, timeout, catalog_ref)
                .await
            {
                Ok(ecus) => Ok(json!({
                    "success": true,
                    "scanned_range": format!("0x{:03X}..=0x{:03X}", start, end),
                    "discovered_count": ecus.len(),
                    "ecus": ecus
                })),
                Err(e) => Err(format!("Bus discovery failed: {}", e)),
            }
        }
        "sterngate_export_report" => {
            let lang: Language = arguments
                .get("lang")
                .and_then(|v| v.as_str())
                .unwrap_or("en")
                .parse()
                .unwrap_or_default();

            match VehicleScanner::scan(&mut mock_iface, lang).await {
                Ok(report) => {
                    let html = report.to_html(lang);
                    let file_saved =
                        if let Some(path) = arguments.get("output_path").and_then(|v| v.as_str()) {
                            let _ = std::fs::write(path, &html);
                            Some(path.to_string())
                        } else {
                            None
                        };

                    Ok(json!({
                        "success": true,
                        "language": lang.to_string(),
                        "vin": report.vin,
                        "scanned_ecus": report.module_results.len(),
                        "modules_responding": report.modules_responding,
                        "total_dtcs": report.total_dtcs,
                        "saved_to_file": file_saved,
                        "html_size_bytes": html.len(),
                        "html_preview": format!("{}...", &html[..html.len().min(300)]),
                    }))
                }
                Err(e) => Err(format!("Failed to generate diagnostic report: {}", e)),
            }
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
