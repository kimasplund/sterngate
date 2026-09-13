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

/// Look up common automotive Diagnostic Trouble Code descriptions by language
pub fn lookup_dtc_description(code: &str, lang: Language) -> String {
    match lang {
        Language::En => match code {
            "P0100" => "Mass Air Flow (MAF) Sensor Circuit Malfunction".into(),
            "P0105" => "Manifold Absolute Pressure (MAP) Sensor Circuit Malfunction".into(),
            "P0115" => "Engine Coolant Temperature Circuit Malfunction".into(),
            "P0234" => "Turbocharger/Supercharger Overboost Condition".into(),
            "P0235" => "Turbocharger Boost Sensor A Circuit Malfunction".into(),
            "P0300" => "Random/Multiple Cylinder Misfire Detected".into(),
            "P0700" => "Transmission Control System (MIL Request)".into(),
            "P0715" => "Input/Turbine Speed Sensor Circuit Malfunction".into(),
            "P0730" => "Incorrect Gear Ratio (Transmission Slip)".into(),
            "P0740" => "Torque Converter Clutch Circuit Malfunction".into(),
            "U0100" => "Lost Communication With Engine Control Module (ECM/PCM)".into(),
            "U0101" => "Lost Communication with Transmission Control Module (TCM)".into(),
            "C1500" => "Air Suspension Central Reservoir Plausibility Error".into(),
            _ => format!("Manufacturer or Standard DTC {}", code),
        },
        Language::De => match code {
            "P0100" => "Luftmassenmesser (LMM) Schaltkreis Fehlfunktion".into(),
            "P0105" => "Saugrohrdrucksensor (MAP) Schaltkreis Fehlfunktion".into(),
            "P0115" => "Kühlmitteltemperatursensor Schaltkreis Fehlfunktion".into(),
            "P0234" => "Ladedruck-Regelung: Regelgrenze überschritten (Überdruck)".into(),
            "P0235" => "Ladedrucksensor A Schaltkreis Fehlfunktion".into(),
            "P0300" => "Verbrennungsaussetzer auf mehreren Zylindern erkannt".into(),
            "P0700" => "Getriebesteuerungssystem (Fehlerleuchten-Anforderung)".into(),
            "P0715" => "Eingangsdrehzahl-/Turbinendrehzahlsensor Schaltkreis Fehlfunktion".into(),
            "P0730" => "Unplausible Gangübersetzung (Getriebeschlupf erkannt)".into(),
            "P0740" => "Wandlerüberbrückungskupplung (WÜK) Fehlfunktion".into(),
            "U0100" => "Kommunikation mit Motorsteuergerät (MSG) verloren".into(),
            "U0101" => "Kommunikation mit Getriebesteuergerät (EGS/VGS) verloren".into(),
            "C1500" => "Luftfederung Zentralspeicher Plausibilitätsfehler".into(),
            _ => format!("Hersteller- oder Standard-Fehlercode {}", code),
        },
        Language::Sv => match code {
            "P0100" => "Luftmassemätare (LMM) Strömkretsfel".into(),
            "P0105" => "Insugstrycksgivare (MAP) Strömkretsfel".into(),
            "P0115" => "Motorkylvätsketemperaturgivare Strömkretsfel".into(),
            "P0234" => "Laddtrycksreglering: Reglergräns överskriden (Övertryck)".into(),
            "P0235" => "Laddtrycksgivare A Strömkretsfel".into(),
            "P0300" => "Slumpmässiga/flera cylinderfeltändningar upptäckta".into(),
            "P0700" => "Växellådsstyrsystem (MIL-begäran)".into(),
            "P0715" => "Ingående varvtalssensor/turbinvarvtalssensor Strömkretsfel".into(),
            "P0730" => "Felaktigt utväxlingsförhållande (Växellådsslir)".into(),
            "P0740" => "Momentomvandlarkoppling (WÜK) Strömkretsfel".into(),
            "U0100" => "Förlorad kommunikation med motorstyrenhet (ECM)".into(),
            "U0101" => "Förlorad kommunikation med växellådsstyrenhet (TCM)".into(),
            "C1500" => "Luftfjädring centralreservoar rimlighetsfel".into(),
            _ => format!("Tillverkarspecifik eller standard felkod {}", code),
        },
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
            0xFF00 => "Erase Flash Memory Routine",
            _ => "Diagnostic Routine Control",
        },
        Language::De => match routine_id {
            0xFF01 => "Kraftstoffpumpe vorfördern & Entlüftung",
            0x0201 => "Nullmengenkalibrierung (NMK) Injektoranpassung zurücksetzen",
            0x0202 => "DPF-Partikelfilter Service-Regeneration starten",
            0x0203 => "Drosselklappe / AGR-Ventil Anschlag einlernen",
            0x0205 => "SBC Bremsen-Hydraulik Entlüftungsroutine",
            0xFF00 => "Flash-Speicher Löschroutine",
            _ => "Diagnose-Routine-Steuerung",
        },
        Language::Sv => match routine_id {
            0xFF01 => "Bränslepump grundning och urluftning",
            0x0201 => "Nollmängdskalibrering (NMK) återställ spridaranpassning",
            0x0202 => "Starta DPF-partikelfilterregenerering",
            0x0203 => "Inlärning av spjällhus / EGR-ändläge",
            0x0205 => "SBC Bromshydraulik avluftningsrutin",
            0xFF00 => "Radera flashminne rutin",
            _ => "Diagnostisk rutinstyrning",
        },
    }
}
