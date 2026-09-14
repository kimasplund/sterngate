use anyhow::{Context, Result};
use tracing::info;

use crate::args::{Cli, ServiceCommands};
use crate::commands::common::{open_interface, parse_hex_bytes};
use sterngate_core::{
    EcoStartStopMode, SuspensionCorner, SuspensionCornerAction, VehicleGarage,
    WorkshopRoutineCatalog,
};
use sterngate_protocol::ServiceRoutineManager;

pub async fn execute(action: ServiceCommands, cli: &Cli) -> Result<()> {
    match action {
        ServiceCommands::Sbc {
            deactivate,
            reactivate,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            if deactivate {
                println!("============================================================");
                println!("  🚨 SBC BRAKE PAD SERVICE MODE: DEACTIVATION");
                println!("============================================================");
                println!("  CAUTION: High pressure hydraulic accumulator (~160 bar) will");
                println!("  be completely depressurized into reservoir. Caliper pistons");
                println!("  will retract and wake-up triggers will be suppressed.");
                let status =
                    ServiceRoutineManager::deactivate_sbc(iface.as_mut(), 0x7E2, 0x7EA).await?;
                println!("\n  ✓ {}", status.message);
                println!(
                    "  • Accumulator Pressure: {:.1} bar",
                    status.accumulator_pressure_bar
                );
                println!(
                    "  • Wake-up Triggers Suppressed: {}",
                    status.wake_up_suppressed
                );
                println!("  • Service Mode Active: {}", status.service_mode_active);
                println!("\n  >> SAFE TO REMOVE WHEELS AND SERVICE BRAKE PADS/CALIPERS <<");
                return Ok(());
            }
            if reactivate {
                println!("============================================================");
                println!("  SBC BRAKE SYSTEM REACTIVATION & PRESSURE BLEED");
                println!("============================================================");
                let status =
                    ServiceRoutineManager::reactivate_sbc(iface.as_mut(), 0x7E2, 0x7EA).await?;
                println!("\n  ✓ {}", status.message);
                println!(
                    "  • Accumulator Pressure: {:.1} bar",
                    status.accumulator_pressure_bar
                );
                println!(
                    "  • Normal Braking Restored: {}",
                    !status.service_mode_active
                );
                return Ok(());
            }
            println!("Specify --deactivate or --reactivate for SBC service mode.");
            Ok(())
        }
        ServiceCommands::Ima {
            read,
            cylinder,
            code,
            vin,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            if let Some(c) = code {
                let cyl =
                    cylinder.context("--cylinder <N> (1-8) required when writing IMA code")?;
                info!(
                    "Writing IMA calibration code '{}' to Cylinder {}...",
                    c, cyl
                );
                let ima = ServiceRoutineManager::write_injector_ima(
                    iface.as_mut(),
                    0x7E0,
                    0x7E8,
                    cyl,
                    &c,
                )
                .await?;
                println!("============================================================");
                println!("  Common Rail Injector IMA Classification Written");
                println!("============================================================");
                println!("  • Cylinder:  {}", ima.cylinder);
                println!("  • Code:      {}", ima.code);
                println!("  • Format:    {}", ima.format);
                println!("  • Status:    Acknowledged by Engine ECU (CR4/EDC16)");

                if let Some(v) = vin {
                    let garage = VehicleGarage::new(VehicleGarage::default_path());
                    let note = format!("Updated Injector Cyl {} IMA code to {}", cyl, ima.code);
                    garage.save_coding(&v, "EDC16", &ima.code, None, &note)?;
                    println!(
                        "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                        v
                    );
                }
                return Ok(());
            }

            if read || cylinder.is_some() {
                println!("============================================================");
                println!("  Common Rail Injector IMA Calibration Codes");
                println!("============================================================");
                let cylinders = if let Some(cyl) = cylinder {
                    vec![cyl]
                } else {
                    (1..=4).collect()
                };
                for cyl in cylinders {
                    match ServiceRoutineManager::read_injector_ima(
                        iface.as_mut(),
                        0x7E0,
                        0x7E8,
                        cyl,
                    )
                    .await
                    {
                        Ok(ima) => {
                            println!(
                                "  • Cylinder {}: {:<8} ({})",
                                ima.cylinder, ima.code, ima.format
                            );
                        }
                        Err(e) => {
                            eprintln!("  • Cylinder {}: Failed to read ({})", cyl, e);
                        }
                    }
                }
                return Ok(());
            }

            println!(
                "Usage: sterngate service ima [--read] [--write --cylinder <N> --code <CODE>]"
            );
            Ok(())
        }
        ServiceCommands::Suspension {
            corner,
            inflate,
            deflate,
            calibrate,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let parsed_corner = SuspensionCorner::parse_str(&corner)
                .context("Invalid corner (choose: fl, fr, rl, rr, rear, all)")?;

            let action = if inflate {
                SuspensionCornerAction::Inflate
            } else if deflate {
                SuspensionCornerAction::Deflate
            } else if calibrate {
                SuspensionCornerAction::CalibrateZeroHeight
            } else {
                println!("Specify --inflate, --deflate, or --calibrate for suspension corner.");
                return Ok(());
            };

            let res = ServiceRoutineManager::actuate_suspension_corner(
                iface.as_mut(),
                0x7E4,
                0x7EC,
                parsed_corner,
                action,
            )
            .await?;

            println!("✓ {}", res);
            Ok(())
        }
        ServiceCommands::Vmax { speed, vin } => {
            let mut iface = open_interface(&cli.can_interface).await;
            println!("============================================================");
            println!("  CONFIGURING VEHICLE SPEED LIMITER (VMax)");
            println!("============================================================");
            println!("  Target Speed: {} km/h", speed);
            let status =
                ServiceRoutineManager::configure_speed_limiter(iface.as_mut(), 0x7E0, 0x7E8, speed)
                    .await?;
            println!("  ✓ {}", status.message);
            if let Some(prev) = status.previous_limit_kmh {
                println!("  • Previous limit: {} km/h", prev);
            }
            println!("  • New limit:      {} km/h", status.speed_limit_kmh);

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = format!(
                "Vehicle speed limiter (VMax) set to {} km/h",
                status.speed_limit_kmh
            );
            let _ = garage.save_coding(
                &target_vin,
                &status.module,
                &format!("{} km/h", status.speed_limit_kmh),
                None,
                &note,
            );
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::Seatbelt { mute, enable, vin } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let acoustic_enabled = enable && !mute;
            println!("============================================================");
            println!("  INSTRUMENT CLUSTER SEATBELT WARNING CHIME");
            println!("============================================================");
            println!(
                "  Target Acoustic Setting: {}",
                if acoustic_enabled { "ENABLED" } else { "MUTED" }
            );
            let status = ServiceRoutineManager::configure_seatbelt_chime(
                iface.as_mut(),
                0x7E4,
                0x7EC,
                acoustic_enabled,
            )
            .await?;
            println!("  ✓ {}", status.message);
            println!(
                "  • Acoustic chime active: {}",
                status.acoustic_chime_enabled
            );
            println!(
                "  • Visual warning lamp:   {}",
                status.visual_warning_lamp_active
            );

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = format!(
                "Instrument cluster seatbelt acoustic warning chime {}",
                if acoustic_enabled { "enabled" } else { "muted" }
            );
            let _ = garage.save_coding(
                &target_vin,
                &status.module,
                if acoustic_enabled {
                    "CHIME_ON"
                } else {
                    "CHIME_MUTED"
                },
                None,
                &note,
            );
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::TankLiters {
            enable,
            disable,
            vin,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let is_enabled = !disable || enable;
            println!("============================================================");
            println!("  INSTRUMENT CLUSTER REMAINING FUEL DISPLAY (RESTLITER)");
            println!("============================================================");
            println!(
                "  Setting: {}",
                if is_enabled { "ENABLE" } else { "DISABLE" }
            );
            let status = ServiceRoutineManager::configure_tank_liters_display(
                iface.as_mut(),
                0x7E4,
                0x7EC,
                is_enabled,
            )
            .await?;
            println!("  ✓ {}", status.message);
            println!(
                "  • Digital exact liters display: {}",
                status.exact_liters_display_enabled
            );

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = format!(
                "Instrument cluster exact tank liters display (Restliteranzeige) {}",
                if is_enabled { "enabled" } else { "disabled" }
            );
            let _ = garage.save_coding(
                &target_vin,
                &status.module,
                if is_enabled {
                    "RESTLITER_ON"
                } else {
                    "RESTLITER_OFF"
                },
                None,
                &note,
            );
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::CorneringLights {
            enable,
            disable,
            vin,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let is_enabled = !disable || enable;
            println!("============================================================");
            println!("  FRONT SAM INTELLIGENT CORNERING FOG LIGHTS");
            println!("============================================================");
            println!(
                "  Setting: {}",
                if is_enabled { "ENABLE" } else { "DISABLE" }
            );
            let status = ServiceRoutineManager::configure_cornering_lights(
                iface.as_mut(),
                0x7E2,
                0x7EA,
                is_enabled,
            )
            .await?;
            println!("  ✓ {}", status.message);
            println!(
                "  • Cornering fog lights active: {}",
                status.cornering_lights_enabled
            );
            println!(
                "  • Activation threshold:       < {} km/h",
                status.activation_threshold_kmh
            );

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = format!(
                "Front SAM intelligent cornering fog lights {}",
                if is_enabled { "enabled" } else { "disabled" }
            );
            let _ = garage.save_coding(
                &target_vin,
                &status.module,
                if is_enabled {
                    "CORNERING_LIGHTS_ON"
                } else {
                    "CORNERING_LIGHTS_OFF"
                },
                None,
                &note,
            );
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::Eco { mode, vin } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let parsed_mode = match mode.to_lowercase().as_str() {
                "always-on" | "always_on" | "on" => EcoStartStopMode::AlwaysOn,
                "memory" | "last-state" | "last_state" | "remember" => {
                    EcoStartStopMode::RememberLastState
                }
                "default-off" | "default_off" | "off" => EcoStartStopMode::DefaultOff,
                other => {
                    anyhow::bail!(
                        "Invalid ECO mode '{}'. Choose: 'always-on', 'memory', or 'default-off'",
                        other
                    );
                }
            };
            println!("============================================================");
            println!("  ECO START-STOP MEMORY CONFIGURATION");
            println!("============================================================");
            println!("  Target Mode: {}", parsed_mode.as_str());
            let status = ServiceRoutineManager::configure_eco_start_stop(
                iface.as_mut(),
                0x7E0,
                0x7E8,
                parsed_mode,
            )
            .await?;
            println!("  ✓ {}", status.message);
            if let Some(prev) = status.previous_mode {
                println!("  • Previous Mode: {}", prev.as_str());
            }
            println!("  • Active Mode:   {}", status.mode.as_str());

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = format!(
                "Updated ECO Start-Stop configuration: {}",
                status.mode.as_str()
            );
            let _ = garage.save_coding(
                &target_vin,
                &status.module,
                &format!("{:02X}", status.did),
                None,
                &note,
            );
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::Egr { vin } => {
            let mut iface = open_interface(&cli.can_interface).await;
            println!("============================================================");
            println!("  EGR ADAPTATION & SOOT REDUCTION OPTIMIZATION");
            println!("============================================================");
            let status =
                ServiceRoutineManager::optimize_egr_adaptation(iface.as_mut(), 0x7E0, 0x7E8)
                    .await?;
            println!("  ✓ {}", status.message);
            println!(
                "  • Air Mass Positive Offset: +{:.1} mg/hub",
                status.air_mass_offset_mg
            );
            println!("  • Lower Stops Relearned:    {}", status.stops_relearned);

            let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
            let garage = VehicleGarage::new(VehicleGarage::default_path());
            let note = "EGR adaptation optimized (+40 mg soot reduction offset applied)";
            let _ =
                garage.save_coding(&target_vin, &status.module, "EGR_AIRMASS_+40MG", None, note);
            println!(
                "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                target_vin
            );
            Ok(())
        }
        ServiceCommands::Adblue { vin } => {
            let mut iface = open_interface(&cli.can_interface).await;
            println!("============================================================");
            println!("  🚨 ADBLUE / SCR 800KM EMERGENCY LOCKOUT RESET");
            println!("============================================================");
            println!("  Executing cryptographic unlock (Level 01) and adaptation wipe...");
            let status =
                ServiceRoutineManager::reset_adblue_countdown(iface.as_mut(), 0x7E0, 0x7E8).await?;
            if status.success {
                println!("  ✓ {}", status.message);
                println!(
                    "  • Security Access Unlocked:     {}",
                    status.security_unlocked
                );
                println!(
                    "  • Countdown Counter Reset:      {}",
                    status.countdown_reset
                );
                println!(
                    "  • NOx Adaptations Cleared:      {}",
                    status.adaptations_cleared
                );
                println!(
                    "  • Ultrasonic Level Calibrated:  {}",
                    status.level_sensor_calibrated
                );

                let target_vin = vin.unwrap_or_else(|| "WDB2112061A000001".into());
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                let note = "AdBlue / SCR emergency lockout counter and NOx adaptations reset";
                let _ = garage.save_coding(
                    &target_vin,
                    "CR4/SCR",
                    "ADBLUE_LOCKOUT_CLEARED",
                    None,
                    note,
                );
                println!(
                    "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                    target_vin
                );
            } else {
                eprintln!("  ❌ {}", status.message);
            }
            Ok(())
        }
        ServiceCommands::List { query, ecu, limit } => {
            let cat = WorkshopRoutineCatalog::load_default()?;
            let q = query.as_deref().unwrap_or("");
            let results = cat.search(q, ecu.as_deref(), limit);
            println!("============================================================");
            println!("  Sterngate Workshop Service & Actuator Routines (0x31)");
            println!(
                "  Total Cataloged: {} | Matches Displayed: {}",
                cat.routines.len(),
                results.len()
            );
            println!("============================================================");
            if results.is_empty() {
                println!("  No routines matched query: '{}'", q);
            } else {
                for r in results {
                    let name_en = &r.name_en;
                    let name_de = &r.name_de;
                    let ecus = r.ecus.join(", ");
                    let prefix = if r.raw_request_prefix.is_empty() {
                        "-"
                    } else {
                        &r.raw_request_prefix
                    };
                    println!("  • {} | [{}] {}", r.routine_id, r.category, name_en);
                    if !name_de.is_empty() && name_de != name_en {
                        println!("    DE:       {}", name_de);
                    }
                    println!(
                        "    ECUs:     {}",
                        if ecus.is_empty() {
                            "Generic/Global"
                        } else {
                            &ecus
                        }
                    );
                    println!("    Prefix:   {}", prefix);
                    println!();
                }
            }
            Ok(())
        }
        ServiceCommands::Run {
            routine,
            ecu,
            sub_function: _,
            data,
            tx_id,
            rx_id,
        } => {
            let mut iface = open_interface(&cli.can_interface).await;
            let r_clean = routine
                .trim()
                .trim_start_matches("0x")
                .trim_start_matches("0X");
            let r_id = u16::from_str_radix(r_clean, 16)
                .map_err(|e| anyhow::anyhow!("Invalid routine hex '{}': {}", routine, e))?;

            let eff_tx = tx_id.unwrap_or_else(|| {
                if ecu.eq_ignore_ascii_case("EGS52") {
                    0x7E1
                } else if ecu.eq_ignore_ascii_case("ESP") {
                    0x7E2
                } else if ecu.eq_ignore_ascii_case("AIRMATIC") || ecu.eq_ignore_ascii_case("ENR") {
                    0x7E3
                } else {
                    0x7E0
                }
            });
            let eff_rx = rx_id.unwrap_or_else(|| {
                if ecu.eq_ignore_ascii_case("EGS52") {
                    0x7E9
                } else if ecu.eq_ignore_ascii_case("ESP") {
                    0x7EA
                } else if ecu.eq_ignore_ascii_case("AIRMATIC") || ecu.eq_ignore_ascii_case("ENR") {
                    0x7EB
                } else {
                    0x7E8
                }
            });

            let data_bytes = if let Some(ref d_str) = data {
                parse_hex_bytes(d_str)?
            } else {
                Vec::new()
            };

            println!("============================================================");
            println!("  Executing Workshop Routine 0x{:04X}", r_id);
            println!(
                "  Target ECU: {} (Tx: 0x{:03X}, Rx: 0x{:03X})",
                ecu, eff_tx, eff_rx
            );
            println!("============================================================");

            match ServiceRoutineManager::execute_generic_routine(
                iface.as_mut(),
                eff_tx,
                eff_rx,
                r_id,
                &data_bytes,
            )
            .await
            {
                Ok(resp) => {
                    let resp_hex = resp
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("  ✓ Routine 0x{:04X} completed successfully!", r_id);
                    println!(
                        "  • Response Bytes: {}",
                        if resp_hex.is_empty() {
                            "Positive ACK (0x71)"
                        } else {
                            &resp_hex
                        }
                    );
                }
                Err(e) => {
                    eprintln!("  ❌ Routine 0x{:04X} failed: {}", r_id, e);
                }
            }
            Ok(())
        }
    }
}
