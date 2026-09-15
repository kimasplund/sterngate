use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperatingMode {
    /// Standalone SBC: Local CAN + Web UI dashboard
    Local,
    /// In-Car Diagnostic Bridge & P2P Host: CAN + Iroh Endpoint P2P listener
    #[value(alias = "car", alias = "host")]
    Bridge,
    /// Remote Technician Client: Iroh Dialer + Technician Web UI
    #[value(alias = "remote")]
    Tech,
}

#[derive(Parser, Debug)]
#[command(
    name = "sterngate",
    version = "0.1.0",
    about = "High-performance modular automotive telemetry, diagnostics, and safe flashing platform"
)]
pub struct Cli {
    #[arg(short, long, value_enum)]
    pub mode: Option<OperatingMode>,

    /// Local standalone mode shortcut
    #[arg(long)]
    pub local: bool,

    /// In-Car Diagnostic Bridge & P2P Host shortcut (generates ticket and listens)
    #[arg(long, alias = "car", alias = "host")]
    pub bridge: bool,

    /// Remote technician client shortcut (dials car node via --ticket)
    #[arg(long, alias = "remote")]
    pub tech: bool,

    /// Shortcut to use native Linux Tactrix OpenPort 2.0 interface
    #[arg(long)]
    pub openport: bool,

    /// Target CAN interface (e.g. can0, vcan0, openport, mock)
    #[arg(long, default_value = "can0")]
    pub can_interface: String,

    /// Listening port for Web UI dashboard
    #[arg(short, long, default_value_t = 8080)]
    pub port: u16,

    /// Root directory for the local firmware vault. Vault scans and firmware
    /// staging are confined to it. Overrides STERNGATE_VAULT_ROOT, which in
    /// turn overrides the default of ./firmware_vault
    #[arg(long, global = true)]
    pub vault: Option<PathBuf>,

    /// Address to bind the Web UI and diagnostic API to. Defaults to loopback:
    /// the API is unauthenticated and can actuate the vehicle, so expose it on
    /// 0.0.0.0 only deliberately.
    #[arg(long, default_value = "127.0.0.1", global = true)]
    pub bind: std::net::IpAddr,

    /// P2P node ticket to dial (Technician mode)
    #[arg(short, long)]
    pub ticket: Option<String>,

    /// Path to vehicle definition profile JSON
    #[arg(long, default_value = "profiles/mercedes/w211_om646_edc16.json")]
    pub profile: PathBuf,

    /// UI and diagnostic output language (en, de, sv)
    #[arg(long, default_value = "en")]
    pub lang: String,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run as Model Context Protocol (MCP) server over stdio for AI agents
    Mcp,
    /// Launch in offline mock simulation mode with virtual Mercedes W211
    Mock {
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Quick command-line diagnostic utilities
    Diag {
        #[command(subcommand)]
        action: DiagCommands,
    },
    /// Safe ECU firmware flashing and calibration suite
    Flash {
        #[command(subcommand)]
        action: FlashCommands,
    },
    /// Workshop mechanical procedures and safety routines (SBC, IMA coding, suspension)
    Service {
        #[command(subcommand)]
        action: ServiceCommands,
    },
    /// Direct ECU variant coding and Git configuration manager
    Coding {
        #[command(subcommand)]
        action: CodingCommands,
    },
    /// Vehicle garage and configuration versioning (git-backed)
    Vehicle {
        #[command(subcommand)]
        action: VehicleCommands,
    },
    /// Predictive analytics and in-flight drive telemetry benchmarking
    Analyze {
        #[command(subcommand)]
        action: AnalyzeCommands,
    },
    /// Vehicle profile management
    Profile {
        #[command(subcommand)]
        action: ProfileCommands,
    },
    /// Automotive ECU database inspection and catalog
    Ecu {
        #[command(subcommand)]
        action: EcuCommands,
    },
    /// Community mods and shareable parameter packages (.sgmod)
    Mod {
        #[command(subcommand)]
        action: ModCommands,
    },
    /// Performance tuning, map analysis, WinOLS features, and stage generation
    Tune {
        #[command(subcommand)]
        action: TuneCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum DiagCommands {
    /// Execute a bus-wide vehicle diagnostic quick scan and health report
    Scan {
        /// Generate and save full Markdown diagnostic report
        #[arg(long, default_value_t = true)]
        report: bool,
        /// Export standalone HTML diagnostic report to specified file path
        #[arg(long)]
        export_html: Option<PathBuf>,
        /// Save vehicle to garage by VIN with git tracking
        #[arg(long, default_value_t = true)]
        save_vehicle: bool,
        /// Output language (en, de, sv)
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// Interrogate CAN bus to discover active ECUs, part numbers, and identification DIDs
    Discover {
        /// Start of CAN arbitration request ID range (hex, e.g. 0x700)
        #[arg(long, default_value = "0x700")]
        start_id: String,
        /// End of CAN arbitration request ID range (hex, e.g. 0x7EF)
        #[arg(long, default_value = "0x7EF")]
        end_id: String,
        /// Timeout per ID in milliseconds
        #[arg(long, default_value_t = 25)]
        timeout_ms: u64,
        /// Optional path to export auto-generated vehicle profile JSON
        #[arg(long)]
        generate_profile: Option<PathBuf>,
    },
    /// Read DTCs from target module
    Dtc {
        #[arg(long, default_value = "EDC16")]
        module: String,
        /// Language for DTC descriptions (en, de, sv)
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// Snapshot of live powertrain telemetry
    Live,
    /// Clear DTC fault memory
    Clear {
        #[arg(long, default_value = "EDC16")]
        module: String,
    },
    /// Execute UDS Service 0x31 RoutineControl (actuators, adaptations, bleeds)
    Routine {
        /// Target ECU module (e.g. EDC16, EGS52)
        #[arg(long, default_value = "EDC16")]
        module: String,
        /// Routine identifier hex (e.g. 0xFF01, 0x0201, 0x0202, 0x0203, 0x0205)
        #[arg(long, default_value = "0xFF01")]
        routine: String,
        /// Routine sub-function (1=startRoutine, 2=stopRoutine, 3=requestResults)
        #[arg(long, default_value_t = 1)]
        sub_function: u8,
        /// Language for routine descriptions (en, de, sv)
        #[arg(long, default_value = "en")]
        lang: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum FlashCommands {
    /// Stage a firmware package locally with cryptographic verification
    Stage {
        /// Path to flash manifest JSON
        #[arg(long)]
        manifest: PathBuf,
        /// Path to raw ROM/bin firmware file
        #[arg(long)]
        rom: PathBuf,
    },
    /// Run pre-flight safety checks (voltage >= 12.5V, SHA-256, CRC32, HW ID)
    Preflight {
        /// Path to flash manifest JSON
        #[arg(long)]
        manifest: PathBuf,
        /// Path to raw ROM/bin firmware file
        #[arg(long)]
        rom: PathBuf,
        /// Optional override for battery voltage (V)
        #[arg(long)]
        voltage: Option<f64>,
    },
    /// Start the detached safe ECU flash sequence
    Start {
        /// Path to flash manifest JSON
        #[arg(long)]
        manifest: PathBuf,
        /// Path to raw ROM/bin firmware file
        #[arg(long)]
        rom: PathBuf,
        /// Skip interactive confirmation prompt
        #[arg(long)]
        force_yes: bool,
    },
    /// Query current flashing engine status and API lock
    Status,
    /// Scan local firmware vault for matching flash files
    VaultScan {
        /// Vault directory path (defaults to --vault, then STERNGATE_VAULT_ROOT)
        #[arg(short, long)]
        path: Option<PathBuf>,
        /// Filter by target ECU Hardware ID
        #[arg(long)]
        hw_id: Option<String>,
        /// Filter by target ECU Software Calibration ID
        #[arg(long)]
        sw_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    /// Sensotronic Brake Control (SBC) safety mode (deactivate for pad changes, reactivate)
    Sbc {
        /// Deactivate SBC (dump 160 bar accumulator, suppress wake-up triggers for pad change)
        #[arg(long)]
        deactivate: bool,
        /// Reactivate SBC & run high-pressure bleed check
        #[arg(long)]
        reactivate: bool,
    },
    /// Common Rail injector IMA classification coding (solenoid production tolerance compensation)
    Ima {
        /// Read current IMA classification codes
        #[arg(long)]
        read: bool,
        /// Cylinder number (1 to 8)
        #[arg(short, long)]
        cylinder: Option<u8>,
        /// Alphanumeric calibration code (e.g. "7B8HNA" or "A8B12FG")
        #[arg(short, long)]
        code: Option<String>,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Air suspension (ENR / AIRMATIC) corner actuation and height calibration
    Suspension {
        /// Suspension corner (fl, fr, rl, rr, rear, all)
        #[arg(short, long, default_value = "rl")]
        corner: String,
        /// Inflate air spring corner
        #[arg(long)]
        inflate: bool,
        /// Deflate air spring corner
        #[arg(long)]
        deflate: bool,
        /// Store zero-height driving calibration level
        #[arg(long)]
        calibrate: bool,
    },
    /// Configure vehicle maximum road speed limiter (VMax)
    Vmax {
        /// Desired speed limit in km/h (e.g. 250, 300)
        #[arg(short, long)]
        speed: u16,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Configure instrument cluster acoustic seatbelt warning chime
    Seatbelt {
        /// Mute acoustic seatbelt warning chime
        #[arg(long)]
        mute: bool,
        /// Enable acoustic seatbelt warning chime
        #[arg(long)]
        enable: bool,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Configure instrument cluster remaining fuel in liters display (Restliteranzeige)
    TankLiters {
        /// Enable remaining fuel display in liters
        #[arg(long)]
        enable: bool,
        /// Disable remaining fuel display in liters
        #[arg(long)]
        disable: bool,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Configure Front SAM intelligent cornering fog lights (Abbiegelicht)
    CorneringLights {
        /// Enable intelligent cornering fog lights
        #[arg(long)]
        enable: bool,
        /// Disable intelligent cornering fog lights
        #[arg(long)]
        disable: bool,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Configure ECO Start-Stop memory behavior
    Eco {
        /// ECO Start-Stop mode ("always-on", "memory" / "last-state", "default-off")
        #[arg(short, long, default_value = "memory")]
        mode: String,
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Optimize EGR adaptation (+40 mg soot reduction offset and stop relearn)
    Egr {
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Reset AdBlue / SCR 800km emergency start lockout counter & adaptations
    Adblue {
        /// Target vehicle VIN for Git garage commit
        #[arg(long)]
        vin: Option<String>,
    },
    /// Search and list workshop service & actuator routines (0x31 RoutineControl)
    List {
        /// Search query (routine ID, German/English description, ECU name)
        #[arg(short, long)]
        query: Option<String>,
        /// Filter routines by ECU module name (e.g. CR4, EDC16, ESP, AIRMATIC)
        #[arg(short, long)]
        ecu: Option<String>,
        /// Maximum number of results to display
        #[arg(short, long, default_value_t = 25)]
        limit: usize,
    },
    /// Execute a generic workshop actuator / service routine by ID
    Run {
        /// Routine ID in hex (e.g. 0x0305, 0xFF01)
        #[arg(short, long)]
        routine: String,
        /// Target ECU name (e.g. CR4, EDC16, ESP)
        #[arg(short, long, default_value = "EDC16")]
        ecu: String,
        /// Routine sub-function (1=start, 2=stop, 3=requestResults)
        #[arg(short, long, default_value_t = 1)]
        sub_function: u8,
        /// Optional hex payload data bytes (e.g. "01FF")
        #[arg(short, long)]
        data: Option<String>,
        /// Optional CAN Tx arbitration ID override
        #[arg(long)]
        tx_id: Option<u32>,
        /// Optional CAN Rx arbitration ID override
        #[arg(long)]
        rx_id: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
pub enum CodingCommands {
    /// Read variant coding DID from ECU
    Read {
        /// Target ECU module (e.g. EDC16, EGS52)
        #[arg(short, long, default_value = "EDC16")]
        module: String,
        /// Data Identifier hex (e.g. 0x0100, 0x2030)
        #[arg(short, long)]
        did: String,
    },
    /// Write variant coding DID to ECU and commit to vehicle garage git
    Write {
        /// Target ECU module (e.g. EDC16, EGS52)
        #[arg(short, long, default_value = "EDC16")]
        module: String,
        /// Data Identifier hex (e.g. 0x2030)
        #[arg(short, long)]
        did: String,
        /// Hex bytes payload to write (e.g. "7B8HNA" ASCII hex or raw bytes)
        #[arg(short, long)]
        data: String,
        /// Vehicle VIN for Git tracking
        #[arg(long)]
        vin: Option<String>,
        /// Commit message / reason for variant coding change
        #[arg(
            short,
            long,
            default_value = "Variant coding modification via Sterngate CLI"
        )]
        note: String,
    },
    /// Backup current ECU variant coding to local Git garage
    Backup {
        /// Vehicle VIN
        #[arg(long)]
        vin: String,
        /// ECU module name (e.g. EDC16)
        #[arg(short, long, default_value = "EDC16")]
        module: String,
    },
    /// Show diff between current ECU coding and historical Git commit
    Diff {
        /// Vehicle VIN
        #[arg(long)]
        vin: String,
        /// Commit hash to compare against (defaults to HEAD~1)
        #[arg(long)]
        commit: Option<String>,
    },
    /// Search and list variant coding Data Identifiers (0x2E WriteDataByIdentifier)
    ListDids {
        /// Search query (DID hex, parameter name, ECU module)
        #[arg(short, long)]
        query: Option<String>,
        /// Filter DIDs by ECU module name
        #[arg(short, long)]
        ecu: Option<String>,
        /// Maximum number of results to display
        #[arg(short, long, default_value_t = 25)]
        limit: usize,
    },
    /// Adapt donor replacement ECU VIN (SecurityAccess unlock, 0x2E write, verification)
    Revin {
        /// Target replacement ECU module (e.g. CR4, EDC16, MED17)
        #[arg(short, long)]
        ecu: String,
        /// New 17-character vehicle identification number (VIN)
        #[arg(short, long)]
        vin: String,
        /// Optional SecurityAccess level override (e.g. 1, 3, 5, 9, 11)
        #[arg(short, long)]
        security_level: Option<u8>,
        /// Optional CAN Tx arbitration ID override
        #[arg(long)]
        tx_id: Option<u32>,
        /// Optional CAN Rx arbitration ID override
        #[arg(long)]
        rx_id: Option<u32>,
    },
}

#[derive(Subcommand, Debug)]
pub enum VehicleCommands {
    /// List all recognized vehicles in the garage
    List,
    /// Inspect a specific vehicle by VIN
    Inspect { vin: String },
    /// View Git commit history of vehicle coding and diagnostics
    History { vin: String },
    /// Rollback vehicle configuration to a previous Git commit
    Rollback {
        vin: String,
        #[arg(long)]
        commit: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AnalyzeCommands {
    /// Analyze S211 rear air suspension (ENR) for leaks and compressor strain
    Suspension {
        /// Path to recorded telemetry CSV log (optional, runs live simulation if omitted)
        #[arg(long)]
        log: Option<PathBuf>,
        /// Inhibit compressor to prevent motor burnout (Routine 0x0210 Safe Mode)
        #[arg(long)]
        inhibit: bool,
        /// Restore normal compressor operation and automatic leveling (Routine 0x0212)
        #[arg(long)]
        restore: bool,
        /// Set suspension into workshop / transport mode (Routine 0x0211)
        #[arg(long)]
        workshop: bool,
    },
    /// Compare two drive runs (A/B testing for fuel consumption and performance)
    Compare {
        /// Baseline drive run CSV log (Run A)
        #[arg(long)]
        run_a: Option<PathBuf>,
        /// Modified drive run CSV log (Run B)
        #[arg(long)]
        run_b: Option<PathBuf>,
    },
    /// Evaluate vehicle vitals against known Mercedes-Benz 'Cascade of Death' failure modes
    Cascades {
        /// Optional path to telemetry JSON input
        #[arg(long)]
        input: Option<PathBuf>,
    },
    /// Active ABC (Active Body Control) hydraulic surge protection routines
    Abc {
        /// Actuate pressure fallback dump to 120 bar safe mode (Routine 0x0220)
        #[arg(long)]
        dump: bool,
        /// Lock strut isolation valves to contain hydraulic bursts (Routine 0x0221)
        #[arg(long)]
        lock: bool,
        /// Restore normal active dynamic body control (Routine 0x0222)
        #[arg(long)]
        restore: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProfileCommands {
    /// List available vehicle profiles
    List,
    /// Inspect a specific profile
    Inspect { path: PathBuf },
    /// Generate a new vehicle profile JSON by interrogating the CAN bus
    Generate {
        /// Output path for generated profile JSON
        #[arg(
            short,
            long,
            default_value = "profiles/discovered/vehicle_profile.json"
        )]
        out: PathBuf,
        /// Manufacturer OEM name
        #[arg(long, default_value = "Mercedes-Benz")]
        oem: String,
        /// Vehicle chassis code (e.g. W211, S211, W204)
        #[arg(long, default_value = "Discovered")]
        chassis: String,
        /// Profile identifier name
        #[arg(long, default_value = "discovered_profile")]
        name: String,
    },
    /// Ingest and convert OEM database archives (SMR-D, CBF) into Sterngate JSON profiles
    Import {
        /// Source path to CBF/SMR-D file or directory containing OEM diagnostics
        #[arg(short, long)]
        input: PathBuf,
        /// Target directory to output Sterngate JSON profiles
        #[arg(short, long, default_value = "profiles")]
        output: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum EcuCommands {
    /// Show summary statistics of the Automotive ECU database and deduplication
    Stats,
    /// Search for ECUs by name or chassis keyword
    Search { query: String },
    /// Inspect details of a specific ECU in the catalog
    Inspect { ecu: String },
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ModCommands {
    /// Inspect and validate a community mod file (.sgmod) or ASCII armored text
    Inspect {
        /// Path to .sgmod file, or '-' for stdin, or raw armored text
        input: String,
        /// Expected VIN for compatibility check (optional)
        #[arg(long)]
        vin: Option<String>,
    },
    /// Safely apply a community mod to the vehicle
    Apply {
        /// Path to .sgmod file, or '-' for stdin, or raw armored text
        input: String,
        /// VIN of the connected vehicle (required: the chassis fingerprint is checked against it)
        #[arg(long)]
        vin: String,
        /// Relax the chassis and hardware-whitelist fingerprint checks only. Voltage, map
        /// provenance and byte preconditions stay enforced; refused for packages that write flash.
        #[arg(long)]
        force: bool,
    },
    /// Create a new shareable community mod package (.sgmod and armored text)
    Create {
        /// Mod display name (e.g. "AMG Needle Sweep & Logo")
        #[arg(long)]
        name: String,
        /// Author name or handle
        #[arg(long)]
        author: String,
        /// Description of the modification
        #[arg(long)]
        description: String,
        /// Target chassis code (e.g. "W211", "W204", or "Universal")
        #[arg(long, default_value = "W211")]
        chassis: String,
        /// Target ECU module identifier (e.g. "IC_211", "EDC16", "EGS52")
        #[arg(long)]
        ecu: String,
        /// Target Data Identifier in hex (e.g. "0x01B0" or "01B0")
        #[arg(long)]
        did: String,
        /// Target payload bytes in hex (e.g. "01FF02")
        #[arg(long)]
        data: String,
        /// Optional bitmask in hex (e.g. "00FF00")
        #[arg(long)]
        mask: Option<String>,
        /// Optional expected precondition bytes in hex
        #[arg(long)]
        expected: Option<String>,
        /// Category (e.g. "Appearance", "Performance", "Comfort", "Drivetrain", "Lighting")
        #[arg(long, default_value = "Appearance")]
        category: String,
        /// Risk level ("safe", "moderate", "expert")
        #[arg(long, default_value = "safe")]
        risk: String,
        /// Minimum safe battery voltage
        #[arg(long, default_value_t = 12.0)]
        min_voltage: f64,
        /// Output path for the generated .sgmod file
        #[arg(long)]
        out: Option<PathBuf>,
        /// Output copy-pasteable ASCII armored text to stdout
        #[arg(long, default_value_t = true)]
        armor: bool,
    },
    /// List available community mods in local directory
    List {
        /// Directory containing .sgmod files
        #[arg(long, default_value = "profiles/mods")]
        dir: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum TuneCommands {
    /// Scan an ECU ROM binary dump to detect 1D, 2D, and 3D calibration maps
    Scan {
        /// Path to ROM binary file (.bin / .ori)
        #[arg(short, long)]
        rom: PathBuf,
        /// Optional map name filter (e.g. "Torque", "Boost", "SVBL")
        #[arg(long)]
        filter: Option<String>,
        /// Display ASCII grid visualization of calibration tables
        #[arg(long, default_value_t = true)]
        table: bool,
    },
    /// Generate a safe, verified Stage 1 calibration package (.sgmod) from a ROM dump
    Stage1 {
        /// Path to stock ROM binary file (.bin / .ori)
        #[arg(short, long)]
        rom: PathBuf,
        /// Target vehicle chassis (e.g. "W211 E280 CDI")
        #[arg(long, default_value = "W211 E280 CDI")]
        chassis: String,
        /// Target ECU name (e.g. "EDC16CP31")
        #[arg(long, default_value = "EDC16CP31")]
        ecu: String,
        /// Author name
        #[arg(long, default_value = "Sterngate Community Tuner")]
        author: String,
        /// Optional path to save generated .sgmod file
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output copy-pasteable ASCII armored text block
        #[arg(long, default_value_t = true)]
        armor: bool,
    },
    /// Generate a Stage 2 race package (.sgmod) with DPF/EGR delete and DTC suppression
    Stage2 {
        /// Path to stock ROM binary file (.bin / .ori)
        #[arg(short, long)]
        rom: PathBuf,
        /// Target vehicle chassis (e.g. "W211 E280 CDI")
        #[arg(long, default_value = "W211 E280 CDI")]
        chassis: String,
        /// Target ECU name (e.g. "EDC16CP31")
        #[arg(long, default_value = "EDC16CP31")]
        ecu: String,
        /// Author name
        #[arg(long, default_value = "Sterngate Community Tuner")]
        author: String,
        /// Optional path to save generated .sgmod file
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output copy-pasteable ASCII armored text block
        #[arg(long, default_value_t = true)]
        armor: bool,
    },
    /// Generate a standalone DTC suppression (P-code kill) .sgmod package
    DtcKill {
        /// Path to stock ROM binary file (.bin / .ori)
        #[arg(short, long)]
        rom: PathBuf,
        /// P-codes to kill, separated by comma (e.g. "P0401,P2002")
        #[arg(short, long)]
        codes: String,
        /// Target vehicle chassis
        #[arg(long, default_value = "W211")]
        chassis: String,
        /// Target ECU name
        #[arg(long, default_value = "EDC16")]
        ecu: String,
        /// Author name
        #[arg(long, default_value = "Sterngate Tuner")]
        author: String,
        /// Optional path to save generated .sgmod file
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Output copy-pasteable ASCII armored text block
        #[arg(long, default_value_t = true)]
        armor: bool,
    },
    /// Verify and solve Bosch MPC5xx flash block checksums
    Checksum {
        /// Path to ROM binary file (.bin / .ori)
        #[arg(short, long)]
        rom: PathBuf,
        /// Recalculate and update all block checksums in-place or write to output
        #[arg(long)]
        fix: bool,
        /// Output path for fixed ROM (if not specified with --fix, updates in-place)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn mod_apply_requires_vin() {
        assert!(Cli::try_parse_from(["sterngate", "mod", "apply", "m.sgmod"]).is_err());
        let cli = Cli::try_parse_from([
            "sterngate",
            "mod",
            "apply",
            "m.sgmod",
            "--vin",
            "WDB2112061A123456",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Commands::Mod { action: ModCommands::Apply { ref vin, .. } }) if vin == "WDB2112061A123456"
        ));
    }
}
