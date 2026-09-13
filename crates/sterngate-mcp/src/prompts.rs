use serde_json::{json, Value};

pub fn get_prompts_list() -> Value {
    json!([
        {
            "name": "diagnose_w211_drivetrain",
            "description": "Step-by-step automotive diagnosis for OM646 engine and 722.6 automatic transmission issues.",
            "arguments": [
                {
                    "name": "symptom",
                    "description": "User described symptom (e.g. rough idle, shift flare, limp mode)",
                    "required": true
                }
            ]
        },
        {
            "name": "safe_flashing_checklist",
            "description": "Runbook to confirm battery maintainer, local ROM staging, and safety interlocks prior to flashing.",
            "arguments": []
        }
    ])
}

pub fn get_prompt_messages(name: &str, _arguments: &Value) -> Result<Value, String> {
    match name {
        "diagnose_w211_drivetrain" => Ok(json!({
            "description": "Drivetrain Diagnostic Procedure",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Perform a complete diagnostic sweep using Sterngate:\n1. Call `sterngate_read_dtc` on EDC16 and EGS52.\n2. Call `sterngate_read_telemetry` to check transmission fluid temp (aiming for 80°C) and injector smooth running.\n3. Identify any cylinder deviations outside ±2.0 mm³/stroke.\n4. Synthesize diagnostic conclusions and suggest physical maintenance steps."
                    }
                }
            ]
        })),
        "safe_flashing_checklist" => Ok(json!({
            "description": "Pre-Flash Verification Procedure",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Before flashing the ECU:\n1. Call `sterngate_verify_flash_staging` with the target HW ID and checksums.\n2. Ensure battery voltage is >= 12.5V.\n3. Verify the file is staged locally and API lockout is prepared."
                    }
                }
            ]
        })),
        _ => Err(format!("Unknown prompt: {}", name)),
    }
}
