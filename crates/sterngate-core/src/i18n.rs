use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Supported languages across Sterngate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    De,
    Sv,
}

impl Language {
    /// Return the two-letter ISO 639-1 code
    pub fn code(&self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
            Language::Sv => "sv",
        }
    }

    /// Return full native display name
    pub fn display_name(&self) -> &'static str {
        match self {
            Language::En => "English",
            Language::De => "Deutsch",
            Language::Sv => "Svenska",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl FromStr for Language {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let clean = s.trim().to_lowercase();
        Ok(match clean.as_str() {
            "de" | "de-de" | "de-at" | "de-ch" | "german" | "deutsch" => Language::De,
            "sv" | "sv-se" | "swedish" | "svenska" => Language::Sv,
            _ => Language::En,
        })
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Diagnostic Trouble Code dictionary entry with translations and raw hex value
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DtcRecord {
    #[serde(default)]
    pub hex: Option<String>,
    #[serde(default)]
    pub en: Option<String>,
    #[serde(default)]
    pub de: Option<String>,
    #[serde(default)]
    pub sv: Option<String>,
}

static DTC_DATABASE: OnceLock<HashMap<String, DtcRecord>> = OnceLock::new();

/// Retrieve the global DTC database (lazy loaded on first access)
pub fn get_dtc_database() -> &'static HashMap<String, DtcRecord> {
    DTC_DATABASE.get_or_init(|| {
        if let Ok(env_path) = std::env::var("STERNGATE_DTC_DATABASE") {
            let p = PathBuf::from(env_path);
            if p.exists() {
                if let Ok(content) = std::fs::read_to_string(&p) {
                    if let Ok(map) = serde_json::from_str::<HashMap<String, DtcRecord>>(&content) {
                        return map;
                    }
                }
            }
        }

        let candidates = [
            Path::new("data/dtc_database_mb.json"),
            Path::new("../../data/dtc_database_mb.json"),
            Path::new("../data/dtc_database_mb.json"),
        ];

        for &cand in &candidates {
            if cand.exists() {
                if let Ok(content) = std::fs::read_to_string(cand) {
                    if let Ok(map) = serde_json::from_str::<HashMap<String, DtcRecord>>(&content) {
                        return map;
                    }
                }
            }
        }

        // Fallback: embedded compile-time OEM database
        const EMBEDDED_DTC: &str = include_str!("../../../data/dtc_database_mb.json");
        serde_json::from_str::<HashMap<String, DtcRecord>>(EMBEDDED_DTC).unwrap_or_default()
    })
}

/// Return total number of indexed DTCs in the active database
pub fn dtc_database_count() -> usize {
    get_dtc_database().len()
}

/// Look up common automotive Diagnostic Trouble Code descriptions by language
pub fn lookup_dtc_description(code: &str, lang: Language) -> String {
    let norm_code = code.trim().to_uppercase();

    // 1. High-priority standard / curated translations
    let static_desc = match lang {
        Language::En => match norm_code.as_str() {
            "P0100" => Some("Mass Air Flow (MAF) Sensor Circuit Malfunction"),
            "P0105" => Some("Manifold Absolute Pressure (MAP) Sensor Circuit Malfunction"),
            "P0115" => Some("Engine Coolant Temperature Circuit Malfunction"),
            "P0234" => Some("Turbocharger/Supercharger Overboost Condition"),
            "P0235" => Some("Turbocharger Boost Sensor A Circuit Malfunction"),
            "P0300" => Some("Random/Multiple Cylinder Misfire Detected"),
            "P0700" => Some("Transmission Control System (MIL Request)"),
            "P0715" => Some("Input/Turbine Speed Sensor Circuit Malfunction"),
            "P0730" => Some("Incorrect Gear Ratio (Transmission Slip)"),
            "P0740" => Some("Torque Converter Clutch Circuit Malfunction"),
            "C1500" => Some("Air Suspension Central Reservoir Plausibility Error"),
            _ => None,
        },
        Language::De => match norm_code.as_str() {
            "P0100" => Some("Luftmassenmesser (LMM) Schaltkreis Fehlfunktion"),
            "P0105" => Some("Saugrohrdrucksensor (MAP) Schaltkreis Fehlfunktion"),
            "P0115" => Some("Kühlmitteltemperatursensor Schaltkreis Fehlfunktion"),
            "P0234" => Some("Ladedruck-Regelung: Regelgrenze überschritten (Überdruck)"),
            "P0235" => Some("Ladedrucksensor A Schaltkreis Fehlfunktion"),
            "P0300" => Some("Verbrennungsaussetzer auf mehreren Zylindern erkannt"),
            "P0700" => Some("Getriebesteuerungssystem (Fehlerleuchten-Anforderung)"),
            "P0715" => Some("Eingangsdrehzahl-/Turbinendrehzahlsensor Schaltkreis Fehlfunktion"),
            "P0730" => Some("Unplausible Gangübersetzung (Getriebeschlupf erkannt)"),
            "P0740" => Some("Wandlerüberbrückungskupplung (WÜK) Fehlfunktion"),
            "C1500" => Some("Luftfederung Zentralspeicher Plausibilitätsfehler"),
            _ => None,
        },
        Language::Sv => match norm_code.as_str() {
            "P0100" => Some("Luftmassemätare (LMM) Strömkretsfel"),
            "P0105" => Some("Insugstrycksgivare (MAP) Strömkretsfel"),
            "P0115" => Some("Motorkylvätsketemperaturgivare Strömkretsfel"),
            "P0234" => Some("Laddtrycksreglering: Reglergräns överskriden (Övertryck)"),
            "P0235" => Some("Laddtrycksgivare A Strömkretsfel"),
            "P0300" => Some("Slumpmässiga/flera cylinderfeltändningar upptäckta"),
            "P0700" => Some("Växellådsstyrsystem (MIL-begäran)"),
            "P0715" => Some("Ingående varvtalssensor/turbinvarvtalssensor Strömkretsfel"),
            "P0730" => Some("Felaktigt utväxlingsförhållande (Växellådsslir)"),
            "P0740" => Some("Momentomvandlarkoppling (WÜK) Strömkretsfel"),
            "U0100" => Some("Förlorad kommunikation med motorstyrenhet (ECM)"),
            "U0101" => Some("Förlorad kommunikation med växellådsstyrenhet (TCM)"),
            "C1500" => Some("Luftfjädring centralreservoar rimlighetsfel"),
            _ => None,
        },
    };

    if let Some(desc) = static_desc {
        return desc.to_string();
    }

    // 2. Query OEM DTC Database (check exact code, then 5-character prefix alias e.g. P1644 from P164456)
    let db = get_dtc_database();
    let entry = db.get(&norm_code).or_else(|| {
        if norm_code.len() > 5 {
            db.get(&norm_code[..5])
        } else {
            None
        }
    });

    if let Some(rec) = entry {
        match lang {
            Language::De => {
                if let Some(de) = &rec.de {
                    return de.clone();
                } else if let Some(en) = &rec.en {
                    return en.clone();
                }
            }
            Language::Sv => {
                if let Some(sv) = &rec.sv {
                    return sv.clone();
                } else if let Some(en) = &rec.en {
                    return en.clone();
                }
            }
            Language::En => {
                if let Some(en) = &rec.en {
                    return en.clone();
                } else if let Some(de) = &rec.de {
                    return de.clone();
                }
            }
        }
    }

    // 3. Fallback generic description
    match lang {
        Language::En => format!("Manufacturer or Standard DTC {}", code),
        Language::De => format!("Hersteller- oder Standard-Fehlercode {}", code),
        Language::Sv => format!("Tillverkarspecifik eller standard felkod {}", code),
    }
}

/// Look up localized UDS Service 0x31 RoutineControl names
pub fn lookup_routine_name(routine_id: u16, lang: Language) -> &'static str {
    match lang {
        Language::En => match routine_id {
            0xFF01 => "Fuel Pump Prime & Rail Bleed",
            0x0201 => "Reset NMK Injector Zero-Quantity Adaptations",
            0x0202 => "Trigger DPF Service Regeneration",
            0x0203 => "Throttle Valve / EGR Lower Stop Relearn",
            0x0205 => "SBC Brake Hydraulic Bleed Routine",
            0x0210 => "Pneumatic Compressor Relay Inhibit (Burnout Safe Mode)",
            0x0211 => "Air Suspension Workshop / Transport Mode (Leveling Inhibit)",
            0x0212 => "Air Suspension Normal Operation Restore",
            0xFF00 => "Erase Flash Memory Routine",
            _ => "Diagnostic Routine Control",
        },
        Language::De => match routine_id {
            0xFF01 => "Kraftstoffpumpe vorfördern & Entlüftung",
            0x0201 => "Nullmengenkalibrierung (NMK) Injektoranpassung zurücksetzen",
            0x0202 => "DPF-Partikelfilter Service-Regeneration starten",
            0x0203 => "Drosselklappe / AGR-Ventil Anschlag einlernen",
            0x0205 => "SBC Bremsen-Hydraulik Entlüftungsroutine",
            0x0210 => "Kompressor-Relais Abschaltung (Überhitzungsschutz)",
            0x0211 => "Luftfederung Werkstatt- / Transportmodus (Regelung gesperrt)",
            0x0212 => "Luftfederung Normalbetrieb wiederherstellen",
            0xFF00 => "Flash-Speicher Löschroutine",
            _ => "Diagnose-Routine-Steuerung",
        },
        Language::Sv => match routine_id {
            0xFF01 => "Bränslepump grundning och urluftning",
            0x0201 => "Nollmängdskalibrering (NMK) återställ spridaranpassning",
            0x0202 => "Starta DPF-partikelfilterregenerering",
            0x0203 => "Inlärning av spjällhus / EGR-ändläge",
            0x0205 => "SBC Bromshydraulik avluftningsrutin",
            0x0210 => "Kompressorrelä avstängning (Överhettningsskydd)",
            0x0211 => "Luftfjädring verkstads- / transportläge (Nivåreglering spärrad)",
            0x0212 => "Luftfjädring normal drift återställd",
            0xFF00 => "Radera flashminne rutin",
            _ => "Diagnostisk rutinstyrning",
        },
    }
}
