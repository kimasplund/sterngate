use serde_json::{json, Value};
use sterngate_core::{
    lookup_routine_name, EcoStartStopMode, Language, SuspensionCorner, SuspensionCornerAction,
    VariantCodingCatalog, VehicleGarage, WorkshopRoutineCatalog,
};
use sterngate_hal::{VehicleInterface, VirtualCanInterface};
use sterngate_protocol::{ServiceRoutineManager, VinAdaptationManager};

use super::helpers::parse_hex_slice;

pub async fn handle(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut mock_iface = VirtualCanInterface::new();
    let _ = mock_iface.open().await;

    match name {
        "sterngate_trigger_routine" => {
            let r_str = arguments
                .get("routine_id")
                .and_then(|v| v.as_str())
                .unwrap_or("0xFF01");
            let r_id = u16::from_str_radix(r_str.trim_start_matches("0x"), 16).unwrap_or(0xFF01);
            let module = arguments
                .get("module")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            let sub_fn = arguments
                .get("sub_function")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8;
            let lang: Language = arguments
                .get("lang")
                .and_then(|v| v.as_str())
                .unwrap_or("en")
                .parse()
                .unwrap_or_default();

            let (tx_id, rx_id) = if module.eq_ignore_ascii_case("EGS52") {
                (0x7E1, 0x7E9)
            } else {
                (0x7E0, 0x7E8)
            };

            let mut uds = sterngate_protocol::UdsClient::new(&mut mock_iface, tx_id, rx_id);
            match uds.routine_control(sub_fn, r_id, &[]).await {
                Ok(resp) => {
                    let desc = lookup_routine_name(r_id, lang);
                    Ok(json!({
                        "success": true,
                        "module": module,
                        "routine_id": format!("0x{:04X}", r_id),
                        "routine_name": desc,
                        "language": lang.to_string(),
                        "sub_function": sub_fn,
                        "status": "Completed successfully",
                        "raw_response_hex": resp.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ")
                    }))
                }
                Err(e) => Err(format!("Routine 0x{:04X} failed: {}", r_id, e)),
            }
        }
        "sterngate_service_routine" => {
            let routine = arguments
                .get("routine")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match routine {
                "sbc_deactivate" => {
                    match ServiceRoutineManager::deactivate_sbc(&mut mock_iface, 0x7E2, 0x7EA).await {
                        Ok(status) => Ok(json!({
                            "success": true,
                            "routine": "sbc_deactivate",
                            "status": status,
                        })),
                        Err(e) => Err(format!("SBC deactivation failed: {}", e)),
                    }
                }
                "sbc_reactivate" => {
                    match ServiceRoutineManager::reactivate_sbc(&mut mock_iface, 0x7E2, 0x7EA).await {
                        Ok(status) => Ok(json!({
                            "success": true,
                            "routine": "sbc_reactivate",
                            "status": status,
                        })),
                        Err(e) => Err(format!("SBC reactivation failed: {}", e)),
                    }
                }
                "read_ima" => {
                    let cylinder = arguments.get("cylinder").and_then(|v| v.as_u64()).unwrap_or(1) as u8;
                    match ServiceRoutineManager::read_injector_ima(&mut mock_iface, 0x7E0, 0x7E8, cylinder).await {
                        Ok(ima) => Ok(json!({
                            "success": true,
                            "routine": "read_ima",
                            "injector": ima,
                        })),
                        Err(e) => Err(format!("Read IMA failed: {}", e)),
                    }
                }
                "write_ima" => {
                    let cylinder = arguments.get("cylinder").and_then(|v| v.as_u64()).unwrap_or(1) as u8;
                    let code = arguments.get("code").and_then(|v| v.as_str()).unwrap_or("");
                    match ServiceRoutineManager::write_injector_ima(&mut mock_iface, 0x7E0, 0x7E8, cylinder, code).await {
                        Ok(ima) => {
                            let vin = arguments.get("vin").and_then(|v| v.as_str()).unwrap_or("WDB2112061A000001");
                            let garage = VehicleGarage::new(VehicleGarage::default_path());
                            let note = format!("Calibrated injector IMA code for cylinder {}: {}", cylinder, ima.code);
                            let _ = garage.save_coding(vin, "EDC16", &ima.code, None, &note);

                            Ok(json!({
                                "success": true,
                                "routine": "write_ima",
                                "injector": ima,
                                "git_recorded": true,
                                "message": format!("Successfully programmed cylinder {} IMA code to {}", cylinder, ima.code),
                            }))
                        }
                        Err(e) => Err(format!("Write IMA failed: {}", e)),
                    }
                }
                "suspension_corner" => {
                    let corner_str = arguments.get("corner").and_then(|v| v.as_str()).unwrap_or("BothRear");
                    let corner = SuspensionCorner::parse_str(corner_str).unwrap_or(SuspensionCorner::BothRear);
                    let action_str = arguments.get("action").and_then(|v| v.as_str()).unwrap_or("inflate");
                    let action = match action_str.to_lowercase().as_str() {
                        "deflate" => SuspensionCornerAction::Deflate,
                        "calibrate_zero" | "calibrate" => SuspensionCornerAction::CalibrateZeroHeight,
                        _ => SuspensionCornerAction::Inflate,
                    };
                    match ServiceRoutineManager::actuate_suspension_corner(&mut mock_iface, 0x7E3, 0x7EB, corner, action).await {
                        Ok(msg) => Ok(json!({
                            "success": true,
                            "routine": "suspension_corner",
                            "corner": corner,
                            "action": action,
                            "message": msg,
                        })),
                        Err(e) => Err(format!("Suspension corner actuation failed: {}", e)),
                    }
                }
                _ => Err(format!("Unknown service routine: '{}'. Valid options: sbc_deactivate, sbc_reactivate, read_ima, write_ima, suspension_corner", routine)),
            }
        }
        "sterngate_guided_workflow" => {
            let workflow = arguments
                .get("workflow")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let vin = arguments
                .get("vin")
                .and_then(|v| v.as_str())
                .unwrap_or("WDB2112061A000001");
            let garage = VehicleGarage::new(VehicleGarage::default_path());

            match workflow {
                "vmax" => {
                    let speed = arguments
                        .get("speed_limit_kmh")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(250) as u16;
                    match ServiceRoutineManager::configure_speed_limiter(
                        &mut mock_iface,
                        0x7E0,
                        0x7E8,
                        speed,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note = format!(
                                "VMax speed limiter configured to {} km/h via MCP",
                                speed
                            );
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                &format!("VMAX_{}KMH", speed),
                                None,
                                &note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("VMax configuration failed: {}", e)),
                    }
                }
                "seatbelt_chime" => {
                    let enabled = arguments
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    match ServiceRoutineManager::configure_seatbelt_chime(
                        &mut mock_iface,
                        0x7E4,
                        0x7EC,
                        enabled,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note = format!(
                                "Instrument cluster seatbelt acoustic warning chime {} via MCP",
                                if enabled { "enabled" } else { "muted" }
                            );
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                if enabled { "CHIME_ON" } else { "CHIME_MUTED" },
                                None,
                                &note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("Seatbelt chime configuration failed: {}", e)),
                    }
                }
                "tank_liters" => {
                    let enabled = arguments
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    match ServiceRoutineManager::configure_tank_liters_display(
                        &mut mock_iface,
                        0x7E4,
                        0x7EC,
                        enabled,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note = format!(
                                "Instrument cluster exact tank liters display {} via MCP",
                                if enabled { "enabled" } else { "disabled" }
                            );
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                if enabled {
                                    "RESTLITER_ON"
                                } else {
                                    "RESTLITER_OFF"
                                },
                                None,
                                &note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("Tank liters configuration failed: {}", e)),
                    }
                }
                "cornering_lights" => {
                    let enabled = arguments
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    match ServiceRoutineManager::configure_cornering_lights(
                        &mut mock_iface,
                        0x7E2,
                        0x7EA,
                        enabled,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note = format!(
                                "Front SAM intelligent cornering fog lights {} via MCP",
                                if enabled { "enabled" } else { "disabled" }
                            );
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                if enabled {
                                    "CORNERING_FOG_ON"
                                } else {
                                    "CORNERING_FOG_OFF"
                                },
                                None,
                                &note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("Cornering lights configuration failed: {}", e)),
                    }
                }
                "eco_start_stop" => {
                    let mode_str = arguments
                        .get("eco_mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("remember");
                    let mode = EcoStartStopMode::parse_str(mode_str)
                        .ok_or_else(|| format!("Invalid ECO mode '{}'", mode_str))?;
                    match ServiceRoutineManager::configure_eco_start_stop(
                        &mut mock_iface,
                        0x7E0,
                        0x7E8,
                        mode,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note = format!(
                                "Updated ECO Start-Stop configuration: {} via MCP",
                                mode.as_str()
                            );
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                &format!("{:02X}", status.did),
                                None,
                                &note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("ECO Start-Stop configuration failed: {}", e)),
                    }
                }
                "egr_optimize" => {
                    match ServiceRoutineManager::optimize_egr_adaptation(
                        &mut mock_iface,
                        0x7E0,
                        0x7E8,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note =
                                "EGR adaptation optimized (+40 mg soot reduction offset applied) via MCP";
                            let _ = garage.save_coding(
                                vin,
                                &status.module,
                                "EGR_AIRMASS_+40MG",
                                None,
                                note,
                            );
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("EGR optimization failed: {}", e)),
                    }
                }
                "adblue_reset" => {
                    match ServiceRoutineManager::reset_adblue_countdown(
                        &mut mock_iface,
                        0x7E0,
                        0x7E8,
                    )
                    .await
                    {
                        Ok(status) => {
                            let note =
                                "AdBlue / SCR emergency 800km countdown and lockout reset executed via MCP";
                            let _ =
                                garage.save_coding(vin, "SCR_DIAG", "0x0218_RESET_OK", None, note);
                            Ok(serde_json::to_value(status).unwrap())
                        }
                        Err(e) => Err(format!("AdBlue reset procedure failed: {}", e)),
                    }
                }
                _ => Err(format!(
                    "Unknown workflow: '{}'. Valid options: 'vmax', 'seatbelt_chime', 'tank_liters', 'cornering_lights', 'eco_start_stop', 'egr_optimize', 'adblue_reset'",
                    workflow
                )),
            }
        }
        "sterngate_search_workshop_routines" => {
            let cat = WorkshopRoutineCatalog::load_default()
                .map_err(|e| format!("Failed to load workshop routine catalog: {}", e))?;
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ecu = arguments.get("ecu").and_then(|v| v.as_str());
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(25) as usize;
            let results = cat.search(query, ecu, limit);

            Ok(json!({
                "query": query,
                "ecu_filter": ecu,
                "total_cataloged": cat.routines.len(),
                "count": results.len(),
                "routines": results,
            }))
        }
        "sterngate_execute_service_routine" => {
            let r_str = arguments
                .get("routine_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'routine_id'".to_string())?;
            let r_clean = r_str
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            let r_id = u16::from_str_radix(r_clean, 16)
                .map_err(|e| format!("Invalid hex routine_id '{}': {}", r_str, e))?;

            let ecu = arguments
                .get("ecu")
                .and_then(|v| v.as_str())
                .unwrap_or("EDC16");
            let eff_tx = arguments
                .get("tx_id")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| {
                    if ecu.eq_ignore_ascii_case("EGS52") {
                        0x7E1
                    } else if ecu.eq_ignore_ascii_case("ESP") {
                        0x7E2
                    } else if ecu.eq_ignore_ascii_case("AIRMATIC")
                        || ecu.eq_ignore_ascii_case("ENR")
                    {
                        0x7E3
                    } else {
                        0x7E0
                    }
                }) as u32;
            let eff_rx = arguments
                .get("rx_id")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(|| {
                    if ecu.eq_ignore_ascii_case("EGS52") {
                        0x7E9
                    } else if ecu.eq_ignore_ascii_case("ESP") {
                        0x7EA
                    } else if ecu.eq_ignore_ascii_case("AIRMATIC")
                        || ecu.eq_ignore_ascii_case("ENR")
                    {
                        0x7EB
                    } else {
                        0x7E8
                    }
                }) as u32;

            let data_bytes = if let Some(d_hex) = arguments.get("data_hex").and_then(|v| v.as_str())
            {
                parse_hex_slice(d_hex)?
            } else {
                Vec::new()
            };

            let resp_bytes = ServiceRoutineManager::execute_generic_routine(
                &mut mock_iface,
                eff_tx,
                eff_rx,
                r_id,
                &data_bytes,
            )
            .await
            .map_err(|e| format!("Routine execution failed: {}", e))?;

            let resp_hex = resp_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");
            Ok(json!({
                "success": true,
                "routine_id": format!("0x{:04X}", r_id),
                "target_ecu": ecu,
                "tx_id": eff_tx,
                "rx_id": eff_rx,
                "response_hex": resp_hex,
                "message": format!("Routine 0x{:04X} executed successfully", r_id),
            }))
        }
        "sterngate_search_variant_coding_dids" => {
            let cat = VariantCodingCatalog::load_default()
                .map_err(|e| format!("Failed to load variant coding catalog: {}", e))?;
            let query = arguments
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ecu = arguments.get("ecu").and_then(|v| v.as_str());
            let limit = arguments
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(25) as usize;
            let results = cat.search(query, ecu, limit);

            Ok(json!({
                "query": query,
                "ecu_filter": ecu,
                "total_cataloged": cat.coding_dids.len(),
                "count": results.len(),
                "coding_dids": results,
            }))
        }
        "sterngate_adapt_donor_ecu_vin" => {
            let ecu = arguments
                .get("ecu")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'ecu'".to_string())?;
            let new_vin = arguments
                .get("new_vin")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required parameter 'new_vin'".to_string())?;

            let eff_tx = arguments
                .get("tx_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0x7E0) as u32;
            let eff_rx = arguments
                .get("rx_id")
                .and_then(|v| v.as_u64())
                .unwrap_or(0x7E8) as u32;
            let sec_lvl = arguments
                .get("security_level")
                .and_then(|v| v.as_u64())
                .map(|l| l as u8);

            let res = VinAdaptationManager::adapt_donor_ecu_vin(
                &mut mock_iface,
                eff_tx,
                eff_rx,
                ecu,
                new_vin,
                sec_lvl,
            )
            .await
            .map_err(|e| format!("Donor ECU Re-VIN adaptation failed: {}", e))?;

            if res.success {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let note = format!(
                    "Donor ECU {} Re-VIN adaptation: programmed to {}",
                    ecu, new_vin
                );
                let _ = garage.save_coding(new_vin, ecu, new_vin, None, &note);
            }

            Ok(serde_json::to_value(res).unwrap())
        }
        _ => Err(format!("Unknown tool name: {}", name)),
    }
}
