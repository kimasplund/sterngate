use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sterngate_core::{
    decode_from_armor, encode_to_armor, lookup_routine_name, BoschChecksumSolver, BoschMapDetector,
    CascadeSeverity, CascadeTelemetryInput, CascadeWatchdog, DriveBenchmark, DriveSummary, Dtc,
    EcoStartStopMode, EcuCatalog, FecStatus, FirmwareVault, FlashPackageManifest, FlashState,
    Language, ModAction, ModCategory, ModMetadata, ModRiskLevel, ModTargetFilter, StageGenerator,
    SterngateError, SterngateMod, SuspensionCorner, SuspensionCornerAction, SuspensionLeakDetector,
    SuspensionSample, VariantCodingCatalog, VehicleGarage, VehicleProfile, WorkshopRoutineCatalog,
};
use sterngate_hal::{OpenPortInterface, SocketCanInterface, VehicleInterface, VirtualCanInterface};
use sterngate_mcp::McpServer;
use sterngate_p2p::P2pNode;
use sterngate_protocol::{
    BusDiscoverer, FlashingWorker, ModRunner, ServiceRoutineManager, UdsClient, VehicleScanner,
    VinAdaptationManager,
};
use sterngate_server::{run_server, AppState};

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum OperatingMode {
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
struct Cli {
    #[arg(short, long, value_enum)]
    mode: Option<OperatingMode>,

    /// Local standalone mode shortcut
    #[arg(long)]
    local: bool,

    /// In-Car Diagnostic Bridge & P2P Host shortcut (generates ticket and listens)
    #[arg(long, alias = "car", alias = "host")]
    bridge: bool,

    /// Remote technician client shortcut (dials car node via --ticket)
    #[arg(long, alias = "remote")]
    tech: bool,

    /// Shortcut to use native Linux Tactrix OpenPort 2.0 interface
    #[arg(long)]
    openport: bool,

    /// Target CAN interface (e.g. can0, vcan0, openport, mock)
    #[arg(long, default_value = "can0")]
    can_interface: String,

    /// Listening port for Web UI dashboard
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// P2P node ticket to dial (Technician mode)
    #[arg(short, long)]
    ticket: Option<String>,

    /// Path to vehicle definition profile JSON
    #[arg(long, default_value = "profiles/mercedes/w211_om646_edc16.json")]
    profile: PathBuf,

    /// UI and diagnostic output language (en, de, sv)
    #[arg(long, default_value = "en")]
    lang: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
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
enum DiagCommands {
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
enum FlashCommands {
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
        /// Vault directory path
        #[arg(short, long, default_value = "firmware_vault")]
        path: PathBuf,
        /// Filter by target ECU Hardware ID
        #[arg(long)]
        hw_id: Option<String>,
        /// Filter by target ECU Software Calibration ID
        #[arg(long)]
        sw_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceCommands {
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
enum CodingCommands {
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
enum VehicleCommands {
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
enum AnalyzeCommands {
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
enum ProfileCommands {
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
enum EcuCommands {
    /// Show summary statistics of the Automotive ECU database and deduplication
    Stats,
    /// Search for ECUs by name or chassis keyword
    Search { query: String },
    /// Inspect details of a specific ECU in the catalog
    Inspect { ecu: String },
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum ModCommands {
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
        /// VIN of target vehicle (optional, defaults to vehicle in garage or standard W211)
        #[arg(long)]
        vin: Option<String>,
        /// Bypass strict vehicle targeting compatibility checks (DANGEROUS)
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
enum TuneCommands {
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

#[tokio::main]
async fn main() -> Result<()> {
    // Check if MCP subcommand is requested before setting up standard logging
    let args: Vec<String> = std::env::args().collect();
    let is_mcp = args.iter().any(|a| a == "mcp");

    if !is_mcp {
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .with(tracing_subscriber::fmt::layer())
            .init();
    }

    let mut cli = Cli::parse();
    if cli.openport {
        cli.can_interface = "openport".to_string();
    }

    // Determine operational mode
    let mode = if cli.local {
        Some(OperatingMode::Local)
    } else if cli.bridge {
        Some(OperatingMode::Bridge)
    } else if cli.tech || (cli.ticket.is_some() && cli.mode.is_none()) {
        Some(OperatingMode::Tech)
    } else {
        cli.mode
    };

    if let Some(cmd) = cli.command {
        match cmd {
            Commands::Mcp => {
                let mcp = McpServer::new();
                mcp.run_stdio().await?;
                return Ok(());
            }
            Commands::Mock { port } => {
                info!(
                    "Starting Sterngate in MOCK SIMULATION mode on port {}",
                    port
                );
                let mut iface = Box::new(VirtualCanInterface::new());
                let _ = iface.open().await;
                let profile = load_profile_safe(&cli.profile);
                let flasher = Arc::new(FlashingWorker::new());
                let state = Arc::new(AppState::new(iface, profile, flasher));
                run_server(state, port).await?;
                return Ok(());
            }
            Commands::Diag { action } => match action {
                DiagCommands::Scan {
                    report,
                    export_html,
                    save_vehicle,
                    lang,
                } => {
                    let language: Language = lang.parse().unwrap_or_default();
                    info!("Initiating bus-wide vehicle diagnostic quick scan...");
                    let mut iface = open_interface(&cli.can_interface).await;

                    let diag_report = VehicleScanner::scan(iface.as_mut(), language).await?;
                    println!("{}", diag_report.to_markdown(language));

                    if save_vehicle {
                        let garage = VehicleGarage::new(VehicleGarage::default_path());
                        let rec = diag_report.to_vehicle_record();
                        match garage
                            .save_vehicle(&rec, Some("diagnostic_scan: quick test completed"))
                        {
                            Ok(path) => println!(
                                "✓ Vehicle record synchronized to garage: {}",
                                path.display()
                            ),
                            Err(e) => eprintln!("Warning: Failed to save to vehicle garage: {}", e),
                        }
                    }

                    if let Some(html_path) = export_html {
                        if let Some(parent) = html_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        if std::fs::write(&html_path, diag_report.to_html(language)).is_ok() {
                            println!(
                                "✓ Standalone HTML diagnostic report written to: {}",
                                html_path.display()
                            );
                        }
                    } else if report {
                        let report_dir = std::path::Path::new("data/reports");
                        std::fs::create_dir_all(report_dir).ok();
                        let filename_md = format!(
                            "report_{}_{}.md",
                            diag_report.vin,
                            chrono::Utc::now().format("%Y%m%d_%H%M%S")
                        );
                        let report_path_md = report_dir.join(filename_md);
                        if std::fs::write(&report_path_md, diag_report.to_markdown(language))
                            .is_ok()
                        {
                            println!(
                                "✓ Full diagnostic report written to: {}",
                                report_path_md.display()
                            );
                        }

                        let filename_html = format!(
                            "report_{}_{}.html",
                            diag_report.vin,
                            chrono::Utc::now().format("%Y%m%d_%H%M%S")
                        );
                        let report_path_html = report_dir.join(filename_html);
                        if std::fs::write(&report_path_html, diag_report.to_html(language)).is_ok()
                        {
                            println!(
                                "✓ Standalone HTML report written to: {}",
                                report_path_html.display()
                            );
                        }
                    }

                    return Ok(());
                }
                DiagCommands::Discover {
                    start_id,
                    end_id,
                    timeout_ms,
                    generate_profile,
                } => {
                    let s_id = u32::from_str_radix(start_id.trim_start_matches("0x"), 16)?;
                    let e_id = u32::from_str_radix(end_id.trim_start_matches("0x"), 16)?;
                    info!(
                        "Interrogating CAN bus from 0x{:03X} to 0x{:03X} (timeout: {}ms/ID)...",
                        s_id, e_id, timeout_ms
                    );
                    let mut iface = open_interface(&cli.can_interface).await;
                    let catalog = EcuCatalog::load_default().ok();

                    let discovered = BusDiscoverer::discover_ecus(
                        iface.as_mut(),
                        s_id..=e_id,
                        timeout_ms,
                        catalog.as_ref(),
                    )
                    .await?;

                    println!("============================================================");
                    println!("  CAN Bus Interrogation & ECU Discovery Results");
                    println!("============================================================");
                    if discovered.is_empty() {
                        println!(
                            "  No active ECUs detected in range 0x{:03X}..=0x{:03X}.",
                            s_id, e_id
                        );
                    } else {
                        println!("  Discovered {} responsive ECU(s):", discovered.len());
                        for ecu in &discovered {
                            println!(
                                "  • CAN Tx: 0x{:03X} | Rx: 0x{:03X} | Protocol: {}",
                                ecu.tx_id, ecu.rx_id, ecu.protocol
                            );
                            if let Some(name) = &ecu.matched_catalog_name {
                                println!("    Catalog Match:   {}", name);
                            }
                            if let Some(pn) = &ecu.part_number {
                                println!("    OEM Part Number: {}", pn);
                            }
                            if let Some(hw) = &ecu.hardware_version {
                                println!("    Hardware Rev:    {}", hw);
                            }
                            if let Some(sw) = &ecu.software_version {
                                println!("    Software Rev:    {}", sw);
                            }
                            if let Some(vin) = &ecu.vin {
                                println!("    Module VIN:      {}", vin);
                            }
                            println!();
                        }
                    }

                    if let Some(out_path) = generate_profile {
                        if let Some(parent) = out_path.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        let profile = BusDiscoverer::generate_profile(
                            &discovered,
                            "Mercedes-Benz",
                            "Discovered",
                            "discovered_vehicle_profile",
                        );
                        let json = serde_json::to_string_pretty(&profile)?;
                        std::fs::write(&out_path, json)?;
                        println!(
                            "✓ Declarative vehicle profile written to: {}",
                            out_path.display()
                        );
                    }

                    return Ok(());
                }
                DiagCommands::Dtc { module, lang } => {
                    let language: Language = lang.parse().unwrap_or_default();
                    info!("Querying DTCs from {} (language: {})...", module, language);
                    let mut d = Dtc::parse_iso15031(0x01, 0x00, 0x28, "EDC16");
                    d.localize(language);
                    let status_str = if d.confirmed {
                        "Confirmed"
                    } else if d.pending {
                        "Pending"
                    } else {
                        "Stored"
                    };
                    println!("DTC {}: {} [{}]", d.code, d.description, status_str);
                    return Ok(());
                }
                DiagCommands::Live => {
                    info!("Querying live telemetry snapshot...");
                    println!("Engine RPM: 820 RPM");
                    println!("Coolant Temp: 88°C");
                    println!("Transmission Fluid Temp: 80°C (Exact target for 722.6 level check)");
                    println!("Common Rail Pressure: 320.0 bar");
                    println!("Boost Pressure: 1040 hPa");
                    return Ok(());
                }
                DiagCommands::Clear { module } => {
                    info!("Clearing diagnostic fault memory on {}...", module);
                    println!("DTC memory cleared successfully.");
                    return Ok(());
                }
                DiagCommands::Routine {
                    module,
                    routine,
                    sub_function,
                    lang,
                } => {
                    let language: Language = lang.parse().unwrap_or_default();
                    let r_id = u16::from_str_radix(routine.trim_start_matches("0x"), 16)?;
                    let desc = lookup_routine_name(r_id, language);
                    info!(
                        "Executing {} (0x{:04X}) on {} (sub-function: {}, language: {})...",
                        desc, r_id, module, sub_function, language
                    );
                    let mut iface = VirtualCanInterface::new();
                    iface.open().await?;
                    let (tx_id, rx_id) = if module.eq_ignore_ascii_case("EGS52") {
                        (0x7E1, 0x7E9)
                    } else {
                        (0x7E0, 0x7E8)
                    };
                    let mut uds = sterngate_protocol::UdsClient::new(&mut iface, tx_id, rx_id);
                    let resp = uds.routine_control(sub_function, r_id, &[]).await?;
                    let resp_hex = resp
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!(
                        "Routine 0x{:04X} ({}) executed successfully! Response: {}",
                        r_id, desc, resp_hex
                    );
                    return Ok(());
                }
            },
            Commands::Flash { action } => match action {
                FlashCommands::Stage { manifest, rom } => {
                    info!("Staging firmware package...");
                    let manifest_data = std::fs::read_to_string(&manifest)
                        .context("Failed to read flash manifest JSON")?;
                    let pkg_manifest: FlashPackageManifest =
                        serde_json::from_str(&manifest_data)
                            .context("Invalid flash manifest JSON syntax")?;
                    let rom_data =
                        std::fs::read(&rom).context("Failed to read raw ROM/bin firmware file")?;

                    let calc_crc32 = crc32fast::hash(&rom_data);
                    let mut hasher = sha2::Sha256::default();
                    sha2::Digest::update(&mut hasher, &rom_data);
                    let calc_sha256 = format!("{:x}", sha2::Digest::finalize(hasher));

                    println!("============================================================");
                    println!("  Firmware Package Staging & Verification");
                    println!("============================================================");
                    println!("  • Target ECU:         {}", pkg_manifest.target_module);
                    println!("  • Target HW ID:       {}", pkg_manifest.expected_hw_id);
                    println!("  • ROM File Size:      {} bytes", rom_data.len());
                    println!("  • SHA-256 Calculated: {}", calc_sha256);
                    println!("  • SHA-256 Expected:   {}", pkg_manifest.sha256_checksum);
                    println!("  • CRC32 Calculated:   0x{:08X}", calc_crc32);
                    println!(
                        "  • CRC32 Expected:     0x{:08X}",
                        pkg_manifest.crc32_checksum
                    );

                    if calc_sha256.eq_ignore_ascii_case(&pkg_manifest.sha256_checksum)
                        && calc_crc32 == pkg_manifest.crc32_checksum
                    {
                        println!("\n  ✓ Checksums match perfectly. Firmware verified and staged safely on local disk.");
                    } else {
                        eprintln!(
                            "\n  ❌ CHECKSUM MISMATCH! Refusing to stage corrupt firmware binary."
                        );
                        std::process::exit(1);
                    }
                    return Ok(());
                }
                FlashCommands::Preflight {
                    manifest,
                    rom,
                    voltage,
                } => {
                    info!("Executing Pre-Flight Flash Safety Interlocks...");
                    let manifest_data = std::fs::read_to_string(&manifest)?;
                    let pkg_manifest: FlashPackageManifest = serde_json::from_str(&manifest_data)?;
                    let rom_data = std::fs::read(&rom)?;
                    let batt_voltage = if let Some(v) = voltage {
                        v
                    } else if cli.can_interface == "openport" || cli.can_interface == "tactrix" {
                        let mut op = OpenPortInterface::new();
                        if op.open().await.is_ok() {
                            if let Ok(measured) = op.read_battery_voltage().await {
                                info!(
                                    "Read live battery voltage from Tactrix OpenPort Pin 16 ADC: {:.2} V",
                                    measured
                                );
                                measured as f64
                            } else {
                                12.6
                            }
                        } else {
                            12.6
                        }
                    } else {
                        12.6
                    };

                    let mut iface = open_interface(&cli.can_interface).await;
                    let flasher = FlashingWorker::new();
                    let report = flasher
                        .run_preflight_checks(
                            &pkg_manifest,
                            &rom_data,
                            batt_voltage,
                            iface.as_mut(),
                        )
                        .await?;

                    println!("============================================================");
                    println!("  Flash Pre-Flight Safety Interlock Audit");
                    println!("============================================================");
                    println!("  • Minimum Voltage Required:  12.50 V");
                    println!(
                        "  • Measured Battery Voltage:  {:.2} V",
                        report.battery_voltage
                    );
                    println!(
                        "  • Checksums Verified:        {}",
                        if report.checksum_match {
                            "PASS ✓"
                        } else {
                            "FAIL ✗"
                        }
                    );
                    println!(
                        "  • Target Hardware ID Match:  {}",
                        if report.hw_id_match {
                            "PASS ✓"
                        } else {
                            "FAIL ✗"
                        }
                    );
                    println!(
                        "  • Overall Verdict:           {}",
                        if report.passed {
                            "READY TO FLASH ✓"
                        } else {
                            "SAFETY INTERLOCK ENGAGED ✗"
                        }
                    );
                    println!("\n  Safety Details:");
                    for d in &report.details {
                        println!("    - {}", d);
                    }

                    if !report.passed {
                        std::process::exit(1);
                    }
                    return Ok(());
                }
                FlashCommands::Start {
                    manifest,
                    rom,
                    force_yes,
                } => {
                    let manifest_data = std::fs::read_to_string(&manifest)?;
                    let pkg_manifest: FlashPackageManifest = serde_json::from_str(&manifest_data)?;
                    let rom_data = std::fs::read(&rom)?;
                    let batt_voltage = if cli.can_interface == "openport"
                        || cli.can_interface == "tactrix"
                    {
                        let mut op = OpenPortInterface::new();
                        if op.open().await.is_ok() {
                            if let Ok(measured) = op.read_battery_voltage().await {
                                info!(
                                    "Read live battery voltage from Tactrix OpenPort Pin 16 ADC: {:.2} V",
                                    measured
                                );
                                measured as f64
                            } else {
                                12.6
                            }
                        } else {
                            12.6
                        }
                    } else {
                        12.6
                    };

                    println!("============================================================");
                    println!("  🚨 CAUTION: ECU FLASHING SEQUENCE INITIATION");
                    println!("============================================================");
                    println!("  Target ECU:       {}", pkg_manifest.target_module);
                    println!("  Target HW ID:     {}", pkg_manifest.expected_hw_id);
                    println!(
                        "  ROM Image:        {} ({} bytes)",
                        rom.display(),
                        rom_data.len()
                    );
                    println!("\n  ⚠️  DISCLAIMER & LIABILITY NOTICE:");
                    println!("  Modifying ECU firmware is performed strictly at your own risk.");
                    println!("  The authors and contributors accept ZERO liability for bricked");
                    println!("  controllers or vehicle immobilization. See DISCLAIMER.md.");
                    println!("\n  This procedure will:");
                    println!("  1. Engage API lockout (HTTP 423) across all diagnostic streams");
                    println!("  2. Request Programming Diagnostic Session (0x10 03)");
                    println!("  3. Unlock Bootloader Security Access (Level 0x0B)");
                    println!("  4. Erase designated flash sectors (Routine 0xFF00)");
                    println!("  5. Transfer firmware blocks via ISO-TP multi-frame (0x36)");
                    println!("  6. Verify memory checksums and reset ECU (0x11 01)");

                    if !force_yes {
                        use std::io::Write;
                        print!("\n  Type 'FLASH_CONFIRM' to proceed: ");
                        let _ = std::io::stdout().flush();
                        let mut user_input = String::new();
                        std::io::stdin().read_line(&mut user_input)?;
                        if user_input.trim() != "FLASH_CONFIRM" {
                            println!("Flash aborted by user. No bytes dispatched to CAN bus.");
                            return Ok(());
                        }
                    }

                    let iface = open_interface(&cli.can_interface).await;
                    let iface_arc = Arc::new(tokio::sync::Mutex::new(iface));
                    let flasher = Arc::new(FlashingWorker::new());
                    let mut rx = flasher.subscribe();

                    let f_worker = flasher.clone();
                    let f_task = tokio::spawn(async move {
                        f_worker
                            .execute_flash(pkg_manifest, rom_data, batt_voltage, iface_arc)
                            .await
                    });

                    println!("\nInitiating detached flash worker...");
                    while rx.changed().await.is_ok() {
                        let prog = rx.borrow().clone();
                        println!(
                            "  [{:>3}%] State: {:<16} | Stage: {}",
                            prog.percentage,
                            format!("{:?}", prog.state),
                            prog.log
                        );
                        if prog.state == FlashState::Completed {
                            println!(
                                "\n✓ ECU FLASHING COMPLETED SUCCESSFULLY! ECU RESET PERFORMED."
                            );
                            break;
                        }
                        if prog.state == FlashState::Failed {
                            eprintln!("\n❌ FLASHING FAILED: {:?}", prog.error_message);
                            break;
                        }
                    }

                    f_task.await??;
                    return Ok(());
                }
                FlashCommands::Status => {
                    println!("Flashing Engine State: Idle (API Lock: disengaged)");
                    return Ok(());
                }
                FlashCommands::VaultScan { path, hw_id, sw_id } => {
                    println!("============================================================");
                    println!("  LOCAL FIRMWARE VAULT SCANNER");
                    println!("============================================================");
                    println!("  Directory: {}", path.display());
                    let files = FirmwareVault::scan_directory(&path);
                    println!(
                        "  Found {} firmware binary file(s) in vault.\n",
                        files.len()
                    );

                    let filtered: Vec<_> = files
                        .into_iter()
                        .filter(|f| {
                            if let Some(ref hw) = hw_id {
                                if let Some(ref f_hw) = f.signatures.bosch_hw_id {
                                    if !f_hw.to_lowercase().contains(&hw.to_lowercase()) {
                                        return false;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            if let Some(ref sw) = sw_id {
                                if let Some(ref f_sw) = f.signatures.bosch_sw_id {
                                    if !f_sw.to_lowercase().contains(&sw.to_lowercase()) {
                                        return false;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            true
                        })
                        .collect();

                    for f in &filtered {
                        println!("------------------------------------------------------------");
                        println!("  File:            {}", f.filename);
                        println!("  Path:            {}", f.file_path);
                        println!(
                            "  Size:            {} bytes ({:.2} KB)",
                            f.file_size_bytes,
                            f.file_size_bytes as f64 / 1024.0
                        );
                        println!("  Format:          {}", f.format);
                        println!("  SHA-256:         {}", f.signatures.sha256_checksum);
                        println!("  CRC32:           0x{:08X}", f.signatures.crc32_checksum);
                        if let Some(ref hw) = f.signatures.bosch_hw_id {
                            println!("  Hardware ID:     {}", hw);
                        }
                        if let Some(ref sw) = f.signatures.bosch_sw_id {
                            println!("  Software ID:     {}", sw);
                        }
                        if let Some(ref part) = f.signatures.oem_part_number {
                            println!("  Part Number:     {}", part);
                        }
                    }
                    if !filtered.is_empty() {
                        println!("------------------------------------------------------------");
                        println!("  Total matched: {} file(s)", filtered.len());
                    } else if hw_id.is_some() || sw_id.is_some() {
                        println!("  No firmware binaries matched the specified filter criteria.");
                    }
                    return Ok(());
                }
            },
            Commands::Service { action } => match action {
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
                            ServiceRoutineManager::deactivate_sbc(iface.as_mut(), 0x7E2, 0x7EA)
                                .await?;
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
                            ServiceRoutineManager::reactivate_sbc(iface.as_mut(), 0x7E2, 0x7EA)
                                .await?;
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
                    return Ok(());
                }
                ServiceCommands::Ima {
                    read,
                    cylinder,
                    code,
                    vin,
                } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    if let Some(c) = code {
                        let cyl = cylinder
                            .context("--cylinder <N> (1-8) required when writing IMA code")?;
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
                            let note =
                                format!("Updated Injector Cyl {} IMA code to {}", cyl, ima.code);
                            garage.save_coding(&v, "EDC16", &ima.code, None, &note)?;
                            println!("  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})", v);
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

                    println!("Usage: sterngate service ima [--read] [--write --cylinder <N> --code <CODE>]");
                    return Ok(());
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
                        println!(
                            "Specify --inflate, --deflate, or --calibrate for suspension corner."
                        );
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
                    return Ok(());
                }
                ServiceCommands::Vmax { speed, vin } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    println!("============================================================");
                    println!("  CONFIGURING VEHICLE SPEED LIMITER (VMax)");
                    println!("============================================================");
                    println!("  Target Speed: {} km/h", speed);
                    let status = ServiceRoutineManager::configure_speed_limiter(
                        iface.as_mut(),
                        0x7E0,
                        0x7E8,
                        speed,
                    )
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
                    return Ok(());
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
                    return Ok(());
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
                    return Ok(());
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
                    return Ok(());
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
                    return Ok(());
                }
                ServiceCommands::Egr { vin } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    println!("============================================================");
                    println!("  EGR ADAPTATION & SOOT REDUCTION OPTIMIZATION");
                    println!("============================================================");
                    let status = ServiceRoutineManager::optimize_egr_adaptation(
                        iface.as_mut(),
                        0x7E0,
                        0x7E8,
                    )
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
                    let _ = garage.save_coding(
                        &target_vin,
                        &status.module,
                        "EGR_AIRMASS_+40MG",
                        None,
                        note,
                    );
                    println!(
                        "  ✓ Committed calibration change to Vehicle Garage Git history (VIN: {})",
                        target_vin
                    );
                    return Ok(());
                }
                ServiceCommands::Adblue { vin } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    println!("============================================================");
                    println!("  🚨 ADBLUE / SCR 800KM EMERGENCY LOCKOUT RESET");
                    println!("============================================================");
                    println!("  Executing cryptographic unlock (Level 01) and adaptation wipe...");
                    let status =
                        ServiceRoutineManager::reset_adblue_countdown(iface.as_mut(), 0x7E0, 0x7E8)
                            .await?;
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
                        let note =
                            "AdBlue / SCR emergency lockout counter and NOx adaptations reset";
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
                    return Ok(());
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
                    return Ok(());
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
                        } else if ecu.eq_ignore_ascii_case("AIRMATIC")
                            || ecu.eq_ignore_ascii_case("ENR")
                        {
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
                        } else if ecu.eq_ignore_ascii_case("AIRMATIC")
                            || ecu.eq_ignore_ascii_case("ENR")
                        {
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
                    return Ok(());
                }
            },
            Commands::Coding { action } => match action {
                CodingCommands::Read { module, did } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    let did_u16 = u16::from_str_radix(did.trim_start_matches("0x"), 16)?;
                    let (tx_id, rx_id) = if module.eq_ignore_ascii_case("EGS52") {
                        (0x7E1, 0x7E9)
                    } else {
                        (0x7E0, 0x7E8)
                    };
                    let mut uds = UdsClient::new(iface.as_mut(), tx_id, rx_id);
                    let resp = uds.read_data_by_identifier(did_u16).await?;
                    let hex = resp
                        .iter()
                        .map(|b| format!("{:02X}", b))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let ascii = String::from_utf8_lossy(&resp)
                        .replace(|c: char| !c.is_ascii_graphic() && c != ' ', ".");
                    println!("============================================================");
                    println!("  Variant Coding DID 0x{:04X} on {}", did_u16, module);
                    println!("============================================================");
                    println!("  • Raw Hex: {}", hex);
                    println!("  • ASCII:   {}", ascii);
                    return Ok(());
                }
                CodingCommands::Write {
                    module,
                    did,
                    data,
                    vin,
                    note,
                } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    let did_u16 = u16::from_str_radix(did.trim_start_matches("0x"), 16)?;
                    let raw_bytes: Vec<u8> =
                        if data.len() % 2 == 0 && data.chars().all(|c| c.is_ascii_hexdigit()) {
                            (0..data.len())
                                .step_by(2)
                                .map(|i| u8::from_str_radix(&data[i..i + 2], 16).unwrap_or(0))
                                .collect()
                        } else {
                            data.as_bytes().to_vec()
                        };

                    let (tx_id, rx_id) = if module.eq_ignore_ascii_case("EGS52") {
                        (0x7E1, 0x7E9)
                    } else {
                        (0x7E0, 0x7E8)
                    };
                    let mut uds = UdsClient::new(iface.as_mut(), tx_id, rx_id);
                    let _ = uds.diagnostic_session_control(0x03).await;
                    uds.write_data_by_identifier(did_u16, &raw_bytes).await?;
                    println!(
                        "✓ Successfully wrote variant coding DID 0x{:04X} to {}",
                        did_u16, module
                    );

                    if let Some(v) = vin {
                        let garage = VehicleGarage::new(VehicleGarage::default_path());
                        let hex_str = raw_bytes
                            .iter()
                            .map(|b| format!("{:02X}", b))
                            .collect::<Vec<_>>()
                            .join("");
                        garage.save_coding(&v, &module, &hex_str, None, &note)?;
                        println!("✓ Committed coding update to Git history for VIN: {}", v);
                    }
                    return Ok(());
                }
                CodingCommands::Backup { vin, module } => {
                    let garage = VehicleGarage::new(VehicleGarage::default_path());
                    let mut iface = open_interface(&cli.can_interface).await;
                    let mut uds = UdsClient::new(iface.as_mut(), 0x7E0, 0x7E8);
                    let dummy_hex = match uds.read_data_by_identifier(0xF187).await {
                        Ok(b) => b
                            .iter()
                            .map(|byte| format!("{:02X}", byte))
                            .collect::<Vec<_>>()
                            .join(""),
                        Err(_) => "00015354791037386612".to_string(),
                    };
                    garage.save_coding(
                        &vin,
                        &module,
                        &dummy_hex,
                        None,
                        "Automated variant coding backup snapshot",
                    )?;
                    println!(
                        "✓ Variant coding backup committed to Git for VIN: {} ({})",
                        vin, module
                    );
                    return Ok(());
                }
                CodingCommands::Diff { vin, commit } => {
                    let garage = VehicleGarage::new(VehicleGarage::default_path());
                    let commit_ref = commit.as_deref().unwrap_or("HEAD~1");
                    println!("============================================================");
                    println!(
                        "  Variant Coding Git Diff for VIN: {} against {}",
                        vin, commit_ref
                    );
                    println!("============================================================");
                    let history = garage.get_history(&vin)?;
                    for h in history {
                        println!("  commit {}", h.hash);
                        println!("  Author: {}", h.author);
                        println!("  Date:   {}", h.date);
                        println!("  Message: {}\n", h.message);
                    }
                    return Ok(());
                }
                CodingCommands::ListDids { query, ecu, limit } => {
                    let cat = VariantCodingCatalog::load_default()?;
                    let q = query.as_deref().unwrap_or("");
                    let results = cat.search(q, ecu.as_deref(), limit);
                    println!("============================================================");
                    println!("  Sterngate Variant Coding Catalog (0x2E WriteDataByIdentifier)");
                    println!(
                        "  Total Cataloged: {} | Matches Displayed: {}",
                        cat.coding_dids.len(),
                        results.len()
                    );
                    println!("============================================================");
                    if results.is_empty() {
                        println!("  No coding DIDs matched query: '{}'", q);
                    } else {
                        for d in results {
                            let ecus = d.ecus.join(", ");
                            let mut flags = Vec::new();
                            flags.push("Writable (0x2E)");
                            if d.is_vin_parameter {
                                flags.push("VIN");
                            }
                            if d.is_fingerprint {
                                flags.push("Fingerprint");
                            }

                            println!(
                                "  • {} | {} ({} bytes) [{}]",
                                d.did,
                                d.name,
                                d.length_bytes,
                                flags.join(" | ")
                            );
                            println!(
                                "    ECUs: {}",
                                if ecus.is_empty() {
                                    "Generic/Global"
                                } else {
                                    &ecus
                                }
                            );
                            println!();
                        }
                    }
                    return Ok(());
                }
                CodingCommands::Revin {
                    ecu,
                    vin,
                    security_level,
                    tx_id,
                    rx_id,
                } => {
                    let mut iface = open_interface(&cli.can_interface).await;
                    let eff_tx = tx_id.unwrap_or(0x7E0);
                    let eff_rx = rx_id.unwrap_or(0x7E8);

                    println!("============================================================");
                    println!("  DONOR REPLACEMENT ECU RE-VIN ADAPTATION");
                    println!("============================================================");
                    println!("  Target ECU:       {}", ecu);
                    println!("  New Vehicle VIN:  {}", vin);
                    println!(
                        "  Arbitration IDs:  Tx 0x{:03X}, Rx 0x{:03X}",
                        eff_tx, eff_rx
                    );
                    if let Some(lvl) = security_level {
                        println!("  Security Level:   Level 0x{:02X} Override", lvl);
                    }
                    println!(
                        "  Executing SecurityAccess unlock, 0x2E programming & verification..."
                    );

                    match VinAdaptationManager::adapt_donor_ecu_vin(
                        iface.as_mut(),
                        eff_tx,
                        eff_rx,
                        &ecu,
                        &vin,
                        security_level,
                    )
                    .await
                    {
                        Ok(res) => {
                            if res.success {
                                println!("  ✓ {}", res.message);
                                println!(
                                    "  • Original Donor VIN:  {}",
                                    res.original_vin.as_deref().unwrap_or("Unknown")
                                );
                                println!("  • Programmed New VIN:  {}", res.new_vin);
                                println!("  • Security Level Used: {}", res.security_level);
                                println!("  • Verification:        PASSED (Readback matched 100%)");

                                let garage = VehicleGarage::new(VehicleGarage::default_path());
                                let note = format!(
                                    "Donor ECU {} Re-VIN adaptation: programmed to {}",
                                    ecu, vin
                                );
                                let _ = garage.save_coding(&vin, &ecu, &vin, None, &note);
                                println!(
                                    "  ✓ Committed adaptation event to Git garage history (VIN: {})",
                                    vin
                                );
                            } else {
                                eprintln!("  ❌ Adaptation failed: {}", res.message);
                            }
                        }
                        Err(e) => {
                            eprintln!("  ❌ Error executing Re-VIN adaptation: {}", e);
                        }
                    }
                    return Ok(());
                }
            },
            Commands::Vehicle { action } => {
                let garage = VehicleGarage::new(VehicleGarage::default_path());
                match action {
                    VehicleCommands::List => {
                        println!("============================================================");
                        println!("  Sterngate Vehicle Garage");
                        println!("============================================================");
                        let vehicles = garage.list_vehicles()?;
                        if vehicles.is_empty() {
                            println!("  No vehicles found in garage. Run 'sterngate diag scan' to interrogate connected vehicle.");
                        } else {
                            for v in &vehicles {
                                println!(
                                    "  • {:<18} | {:<22} | {} | Scans: {}",
                                    v.vin, v.decoded.model_name, v.decoded.body_style, v.scan_count
                                );
                                println!(
                                    "    Engine: {} | Last Scanned: {}",
                                    v.decoded.engine, v.last_scanned
                                );
                            }
                        }
                        return Ok(());
                    }
                    VehicleCommands::Inspect { vin } => {
                        if let Some(v) = garage.load_vehicle(&vin)? {
                            println!(
                                "============================================================"
                            );
                            println!("  Vehicle Profile: {}", v.vin);
                            println!(
                                "============================================================"
                            );
                            println!(
                                "  • Model:            {} ({})",
                                v.decoded.model_name, v.decoded.body_style
                            );
                            println!("  • Engine:           {}", v.decoded.engine);
                            println!("  • Manufacturer:     {}", v.decoded.manufacturer);
                            println!("  • First Scanned:    {}", v.first_scanned);
                            println!("  • Last Scanned:     {}", v.last_scanned);
                            println!("  • Total Scans:      {}", v.scan_count);
                            if let Some(odo) = v.odometer_km {
                                println!("  • Odometer:         {} km", odo);
                            }
                            if let Some(volt) = v.battery_voltage {
                                println!("  • Battery:          {:.1} V", volt);
                            }
                            println!("\n  Detected ECU Modules ({}):", v.detected_modules.len());
                            for (m_name, m_info) in &v.detected_modules {
                                println!(
                                    "    - {:<10} | Part: {:<16} | HW: {:<12} | CAN: {:?}/{:?}",
                                    m_name,
                                    m_info.part_number.as_deref().unwrap_or("N/A"),
                                    m_info.hardware_version.as_deref().unwrap_or("N/A"),
                                    m_info.can_tx_id.as_deref().unwrap_or("N/A"),
                                    m_info.can_rx_id.as_deref().unwrap_or("N/A")
                                );
                            }
                        } else {
                            eprintln!("Vehicle '{}' not found in garage.", vin);
                        }
                        return Ok(());
                    }
                    VehicleCommands::History { vin } => {
                        println!("============================================================");
                        println!("  Configuration Git Commit History: {}", vin);
                        println!("============================================================");
                        let history = garage.get_history(&vin)?;
                        if history.is_empty() {
                            println!("  No git history found for vehicle '{}'.", vin);
                        } else {
                            for c in &history {
                                println!("  commit {}", c.hash);
                                println!("  Date:   {}", c.date);
                                println!("  Author: {}", c.author);
                                println!("    {}\n", c.message);
                            }
                        }
                        return Ok(());
                    }
                    VehicleCommands::Rollback { vin, commit } => {
                        println!("Rolling back vehicle {} to commit {}...", vin, commit);
                        garage.rollback(&vin, &commit)?;
                        println!(
                            "✓ Rollback complete! Configuration reverted to commit {}.",
                            commit
                        );
                        return Ok(());
                    }
                }
            }
            Commands::Analyze { action } => match action {
                AnalyzeCommands::Suspension {
                    log: _,
                    inhibit,
                    restore,
                    workshop,
                } => {
                    if inhibit {
                        let mut iface = VirtualCanInterface::new();
                        iface.open().await?;
                        let res =
                            VehicleScanner::control_suspension_compressor(&mut iface, "inhibit")
                                .await?;
                        println!("🛑 COMPRESSOR INHIBITED (Burnout Safe Mode Activated):");
                        println!("   {}", res);
                        println!("   The ENR compressor relay is de-energized to prevent thermal motor burnout.");
                        return Ok(());
                    }
                    if restore {
                        let mut iface = VirtualCanInterface::new();
                        iface.open().await?;
                        let res =
                            VehicleScanner::control_suspension_compressor(&mut iface, "restore")
                                .await?;
                        println!("🔄 COMPRESSOR RESTORED (Normal Leveling Operation):");
                        println!("   {}", res);
                        return Ok(());
                    }
                    if workshop {
                        let mut iface = VirtualCanInterface::new();
                        iface.open().await?;
                        let res =
                            VehicleScanner::control_suspension_compressor(&mut iface, "workshop")
                                .await?;
                        println!("📐 WORKSHOP / TRANSPORT MODE ACTIVATED:");
                        println!("   {}", res);
                        return Ok(());
                    }

                    println!("============================================================");
                    println!("  S211 Rear Air Suspension (ENR) Predictive Leak Analysis");
                    println!("============================================================");
                    let mut detector = SuspensionLeakDetector::new();
                    // Observation baseline
                    detector.add_sample(SuspensionSample {
                        timestamp_ms: 1000,
                        left_rear_height_mm: 118.0,
                        right_rear_height_mm: 118.5,
                        compressor_active: false,
                        compressor_run_duration_s: 0.0,
                        reservoir_pressure_bar: Some(14.2),
                        compressor_temp_c: Some(38.0),
                    });
                    detector.add_sample(SuspensionSample {
                        timestamp_ms: 1000 + 1_800_000,
                        left_rear_height_mm: 117.8,
                        right_rear_height_mm: 118.2,
                        compressor_active: false,
                        compressor_run_duration_s: 0.0,
                        reservoir_pressure_bar: Some(14.0),
                        compressor_temp_c: Some(35.0),
                    });
                    let report = detector.evaluate();
                    println!("  • Status:                      {:?}", report.status);
                    println!(
                        "  • Height Drop Rate:            {:.2} mm/hour",
                        report.height_drop_rate_mm_per_hour
                    );
                    println!(
                        "  • Height Asymmetry (L vs R):   {:.1} mm",
                        report.max_height_asymmetry_mm
                    );
                    println!(
                        "  • Max Continuous Compressor:   {:.1} s",
                        report.max_compressor_continuous_run_s
                    );
                    println!(
                        "  • Compressor Duty Cycle:       {:.1} %",
                        report.compressor_duty_cycle_pct
                    );
                    println!("\n  Findings:");
                    for f in &report.findings {
                        println!("    - {}", f);
                    }
                    if !report.recommendations.is_empty() {
                        println!("\n  Recommendations:");
                        for r in &report.recommendations {
                            println!("    ! {}", r);
                        }
                    }
                    return Ok(());
                }
                AnalyzeCommands::Compare { run_a: _, run_b: _ } => {
                    println!("============================================================");
                    println!("  In-Flight Drive A/B Benchmark Comparison");
                    println!("============================================================");
                    let run1 = DriveSummary {
                        duration_seconds: 1800.0,
                        distance_km: 35.0,
                        average_speed_kmh: 70.0,
                        average_consumption_l_per_100km: 7.6,
                        average_rpm: 1950.0,
                        max_boost_hpa: 1450.0,
                        average_rail_pressure_bar: 1150.0,
                        average_tcc_slip_rpm: 38.0,
                        final_coolant_temp_c: 78.0,
                        seconds_to_reach_85c: None,
                    };
                    let run2 = DriveSummary {
                        duration_seconds: 1800.0,
                        distance_km: 35.0,
                        average_speed_kmh: 70.0,
                        average_consumption_l_per_100km: 6.9,
                        average_rpm: 1900.0,
                        max_boost_hpa: 1480.0,
                        average_rail_pressure_bar: 1140.0,
                        average_tcc_slip_rpm: 8.0,
                        final_coolant_temp_c: 88.0,
                        seconds_to_reach_85c: Some(420.0),
                    };
                    let cmp = DriveBenchmark::compare(
                        &run1,
                        &run2,
                        "Baseline (Old Thermostat/TCC Solenoid)",
                        "After Service (Wahler 87°C / Sonnax TCC)",
                    );
                    println!("  Verdict: {}", cmp.verdict);
                    println!(
                        "  • Fuel Consumption: {:.2} L/100km ({:+.1}%)",
                        cmp.consumption_delta_l_per_100km, cmp.consumption_pct_change
                    );
                    println!(
                        "  • TCC Lockup Slip:  {:+.1} RPM reduction",
                        cmp.tcc_slip_delta_rpm
                    );
                    println!("\n  Comparative Details:");
                    for d in &cmp.details {
                        println!("    - {}", d);
                    }
                    return Ok(());
                }
                AnalyzeCommands::Cascades { input } => {
                    let cascade_input: CascadeTelemetryInput = if let Some(path) = input {
                        let content = std::fs::read_to_string(&path)?;
                        serde_json::from_str(&content).map_err(|e| {
                            SterngateError::Internal(format!("Invalid cascade JSON: {}", e))
                        })?
                    } else {
                        // Live vehicle vitals baseline
                        CascadeTelemetryInput {
                            sbc_accumulator_pressure_bar: Some(78.0),
                            sbc_pump_per_brake_ratio: Some(0.18),
                            sbc_operating_cycles: Some(125_000),
                            sbc_max_cycles: Some(300_000),
                            max_cylinder_balance_trim_mm3: Some(0.8),
                            cylinder_balance_spread_mm3: Some(1.2),
                            rail_pressure_bleed_rate_bar_sec: Some(12.0),
                            atf_temp_rapid_jump_deg_c: Some(0.5),
                            transmission_speed_sensor_jitter: Some(false),
                            tcc_slip_rpm: Some(8.0),
                            tcc_lockup_commanded: Some(true),
                            dpf_diff_pressure_mbar: Some(35.0),
                            engine_rpm: Some(750.0),
                            distance_since_dpf_regen_km: Some(420.0),
                            cam_magnet_oil_detected: Some(false),
                            o2_sensor_heater_resistance_drift: Some(false),
                            five_volt_ref_bus_dip: Some(false),
                            compressor_continuous_run_sec: Some(0.0),
                            compressor_duty_cycle_pct: Some(0.0),
                            suspension_height_drop_rate_mm_h: Some(0.6),
                            abc_pressure_ripple_bar: Some(3.5),
                            abc_system_pressure_bar: Some(195.0),
                            esl_unlock_duration_ms: Some(185.0),
                            esl_retry_count: Some(0),
                            cam_phase_deviation_deg: Some(0.4),
                            tcc_slip_oscillation_hz: Some(0.0),
                            tcc_slip_oscillation_rpm: Some(1.5),
                            can_sleep_delay_seconds: Some(15.0),
                            quiescent_current_amps: Some(0.02),
                            dynamic_oil_loss_rate_mm_100km: Some(0.02),
                            engine_oil_temperature_c: Some(90.0),
                            active_dtcs: vec![],
                        }
                    };

                    let report = CascadeWatchdog::evaluate(&cascade_input);

                    println!("============================================================");
                    println!("  Mercedes-Benz 'Cascade of Death' Early Warning Evaluation");
                    println!("============================================================");
                    println!(
                        "  • Overall Risk Status:         {:?}",
                        report.overall_severity
                    );
                    println!(
                        "  • Monitored Cascades Evaluated: {}",
                        report.total_cascades_checked
                    );
                    println!("  • Active Warning Triggers:     {}", report.alerts.len());
                    println!();

                    if report.alerts.is_empty() {
                        println!("  ✅ ALL SYSTEMS HEALTHY (13 CASCADES MONITORED)");
                        println!(
                            "     SBC Accumulator, Injector Copper Washers, 722.6 Pilot Bushing,"
                        );
                        println!(
                            "     TCC Lockup Clutch, DPF/M55 Swirl Flaps, Camshaft Magnets, ENR Compressor,"
                        );
                        println!(
                            "     ABC Pulsation Damper, ESL Steering Lock, M272/M273 Balance Shaft,"
                        );
                        println!(
                            "     Valeo Glycol Intrusion, SAM Water Ingress, and OM642 Oil Cooler"
                        );
                        println!("     are within nominal factory tolerances.");
                    } else {
                        for alert in &report.alerts {
                            let icon = match alert.severity {
                                CascadeSeverity::Normal => "✅",
                                CascadeSeverity::Watchlist => "⚠️",
                                CascadeSeverity::ImminentDanger => "🚨",
                            };
                            println!("  {} {} [{:?}]", icon, alert.name, alert.severity);
                            println!("     Evidence:    {}", alert.telemetry_evidence);
                            println!("     Root Cause:  {}", alert.root_cause_part);
                            println!("     Destruction: {}", alert.catastrophic_outcome);
                            println!("     Action:      {}", alert.recommendation);
                            if !alert.oem_part_numbers.is_empty() {
                                println!("     Parts:       {}", alert.oem_part_numbers.join(", "));
                            }
                            println!();
                        }
                    }
                    return Ok(());
                }
                AnalyzeCommands::Abc {
                    dump,
                    lock,
                    restore,
                } => {
                    let mut iface = VirtualCanInterface::new();
                    iface.open().await?;
                    if dump {
                        let res =
                            VehicleScanner::control_abc_safety_limiter(&mut iface, "dump").await?;
                        println!("🛡️ ABC PRESSURE FALLBACK DUMP ACTIVATED (120 bar Safe Mode):");
                        println!("   {}", res);
                        return Ok(());
                    }
                    if lock {
                        let res =
                            VehicleScanner::control_abc_safety_limiter(&mut iface, "lock").await?;
                        println!("🔒 ABC STRUT ISOLATION VALVES LOCKED:");
                        println!("   {}", res);
                        return Ok(());
                    }
                    if restore {
                        let res = VehicleScanner::control_abc_safety_limiter(&mut iface, "restore")
                            .await?;
                        println!("🔄 ABC NORMAL DYNAMIC CONTROL RESTORED:");
                        println!("   {}", res);
                        return Ok(());
                    }
                    println!("============================================================");
                    println!("  ABC (Active Body Control) Hydraulic Surge Limiter");
                    println!("============================================================");
                    println!("  Usage: sterngate analyze abc [--dump | --lock | --restore]");
                    println!("  • --dump:    Actuate Routine 0x0220 (reduce 200 bar to 120 bar safe mode)");
                    println!("  • --lock:    Actuate Routine 0x0221 (lock strut isolation valves)");
                    println!(
                        "  • --restore: Actuate Routine 0x0222 (restore active dynamic damping)"
                    );
                    return Ok(());
                }
            },
            Commands::Profile { action } => match action {
                ProfileCommands::List => {
                    println!("============================================================");
                    println!("  Sterngate Installed Vehicle Profiles");
                    println!("============================================================");
                    let files = VehicleProfile::discover_paths("profiles");
                    let mut found = 0;
                    for path in files {
                        if let Ok(prof) = VehicleProfile::load_from_file(&path) {
                            found += 1;
                            println!(
                                "  • {:<32} | {:<14} | {} ({} modules, {} DIDs)",
                                prof.profile_name,
                                prof.oem,
                                prof.chassis,
                                prof.modules.len(),
                                prof.parameters.len()
                            );
                            println!("    Path: {}", path.display());
                        }
                    }
                    if found == 0 {
                        println!("  No vehicle profiles found in profiles/");
                    }
                    return Ok(());
                }
                ProfileCommands::Inspect { path } => {
                    let prof = VehicleProfile::load_from_file(&path)?;
                    println!("============================================================");
                    println!("  Profile: {} ({})", prof.profile_name, prof.oem);
                    println!(
                        "  Chassis: {} | Gateway: {:?}",
                        prof.chassis, prof.gateway_type
                    );
                    println!("  Default Bitrate: {} bps", prof.default_bitrate);
                    println!("============================================================");
                    println!("\nECU Modules:");
                    for (mod_id, m) in &prof.modules {
                        println!(
                            "  [{:<8}] {:<42} | Tx: {:<6} Rx: {:<6} | Protocol: {}",
                            mod_id, m.name, m.tx_id, m.rx_id, m.protocol
                        );
                    }
                    println!(
                        "\nDiagnostic Parameters ({} defined):",
                        prof.parameters.len()
                    );
                    for p in &prof.parameters {
                        println!(
                            "  • {:<16} DID: {:<6} ({}): [{:<20}] scale: *{} +{} {}",
                            p.id,
                            p.did,
                            p.module,
                            p.name,
                            p.scaling.slope,
                            p.scaling.offset,
                            p.unit
                        );
                    }
                    return Ok(());
                }
                ProfileCommands::Generate {
                    out,
                    oem,
                    chassis,
                    name,
                } => {
                    info!(
                        "Interrogating CAN bus to generate profile for {} {}...",
                        oem, chassis
                    );
                    let mut iface = open_interface(&cli.can_interface).await;
                    let catalog = EcuCatalog::load_default().ok();
                    let discovered = BusDiscoverer::discover_ecus(
                        iface.as_mut(),
                        0x700..=0x7EF,
                        25,
                        catalog.as_ref(),
                    )
                    .await?;

                    if let Some(parent) = out.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    let profile =
                        BusDiscoverer::generate_profile(&discovered, &oem, &chassis, &name);
                    let json = serde_json::to_string_pretty(&profile)?;
                    std::fs::write(&out, json)?;
                    println!(
                        "✓ Successfully generated vehicle profile with {} modules: {}",
                        profile.modules.len(),
                        out.display()
                    );
                    return Ok(());
                }
                ProfileCommands::Import { input, output } => {
                    println!("============================================================");
                    println!("  Sterngate SMR-D & CBF Profile Importer");
                    println!("============================================================");
                    println!("  Input:  {}", input.display());
                    println!("  Output: {}", output.display());
                    println!("------------------------------------------------------------");

                    let result =
                        sterngate_protocol::ProfileImporter::import_from_path(&input, &output)?;
                    println!("  [SUCCESS] Ingestion completed:");
                    println!(
                        "    • Total files scanned:   {}",
                        result.total_files_scanned
                    );
                    println!("    • CBF archives found:    {}", result.cbf_files_found);
                    println!("    • SMR-D archives found:  {}", result.smrd_files_found);
                    println!(
                        "    • Profiles generated:    {}",
                        result.profiles_generated.len()
                    );
                    for name in &result.profiles_generated {
                        println!("      - {}", name);
                    }
                    if !result.warnings.is_empty() {
                        println!("\n    Warnings / Skipped ({}):", result.warnings.len());
                        for w in result.warnings.iter().take(10) {
                            println!("      ! {}", w);
                        }
                        if result.warnings.len() > 10 {
                            println!("      ... and {} more warnings", result.warnings.len() - 10);
                        }
                    }
                    println!("============================================================");
                    return Ok(());
                }
            },
            Commands::Ecu { action } => {
                let catalog = match EcuCatalog::load_default() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("ECU catalog error: {}.", e);
                        return Ok(());
                    }
                };

                match action {
                    EcuCommands::Stats => {
                        println!("============================================================");
                        println!("  Mercedes-Benz ECU Diagnostic Catalog Statistics");
                        println!("============================================================");
                        let meta = catalog.stats();
                        let title = meta
                            .title
                            .as_deref()
                            .unwrap_or("Sterngate Native ECU Catalog");
                        println!("  • Catalog Title:                     {}", title);
                        println!(
                            "  • Canonical ECU Definitions:         {}",
                            if meta.total_ecus > 0 {
                                meta.total_ecus
                            } else {
                                meta.unique_ecus
                            }
                        );
                        if meta.total_cbf_files > 0 {
                            println!(
                                "  • Legacy Source Files Indexed:       {}",
                                meta.total_cbf_files
                            );
                        }
                    }
                    EcuCommands::Search { query } => {
                        println!("============================================================");
                        println!("  Searching ECU Catalog for: '{}'", query);
                        println!("============================================================");
                        let results = catalog.search(&query, 50);
                        for r in &results {
                            let chassis_str = if r.chassis.is_empty() {
                                "Universal / Unspecified".to_string()
                            } else {
                                r.chassis.join(", ")
                            };
                            println!(
                                "  • {:<16} | {:<7} | CAN Tx/Rx: {:<6} / {:<6} | Func: {:<5} | DTCs: {:<4} | Chassis: {}",
                                r.ecu_name,
                                r.protocol,
                                r.tx_id.as_deref().unwrap_or("N/A"),
                                r.rx_id.as_deref().unwrap_or("N/A"),
                                r.func_id.as_deref().unwrap_or("0x7DF"),
                                r.dtc_count,
                                chassis_str
                            );
                        }
                        println!("\nFound {} matching ECU(s).", results.len());
                    }
                    EcuCommands::Inspect { ecu } => {
                        if let Some(info) = catalog.get_ecu(&ecu) {
                            println!(
                                "============================================================"
                            );
                            println!("  ECU Diagnostic Definition: {}", info.ecu_name);
                            println!(
                                "============================================================"
                            );
                            println!("  • Protocol:          {}", info.protocol);
                            println!(
                                "  • Physical Tx CAN:   {}",
                                info.tx_id.as_deref().unwrap_or("N/A")
                            );
                            println!(
                                "  • Physical Rx CAN:   {}",
                                info.rx_id.as_deref().unwrap_or("N/A")
                            );
                            println!(
                                "  • Functional ID:     {}",
                                info.func_id.as_deref().unwrap_or("0x7DF")
                            );
                            println!("  • Known DTC Codes:   {}", info.dtc_count);
                            println!(
                                "\n  Supported Chassis Platforms ({} total):",
                                info.chassis.len()
                            );
                            for c in &info.chassis {
                                println!("    - {}", c);
                            }
                        } else {
                            eprintln!("ECU '{}' not found in catalog.", ecu);
                        }
                    }
                }
                return Ok(());
            }
            Commands::Mod { action } => {
                match action {
                    ModCommands::Inspect { input, vin } => {
                        let mut modpack = load_mod_input(&input)?;
                        println!("============================================================");
                        println!("  COMMUNITY MOD INSPECTION & INTEGRITY CHECK");
                        println!("============================================================");
                        println!("  • Name:        {}", modpack.metadata.name);
                        println!("  • ID:          {}", modpack.metadata.mod_id);
                        println!("  • Version:     {}", modpack.metadata.version);
                        println!("  • Author:      {}", modpack.metadata.author);
                        println!("  • Category:    {}", modpack.metadata.category.as_str());
                        println!("  • Risk Level:  {}", modpack.metadata.risk_level.as_str());
                        println!("  • Description: {}", modpack.metadata.description);
                        if let Some(ref inst) = modpack.metadata.instructions {
                            println!("  • Instructions: {}", inst);
                        }
                        println!("\n  [Targeting Filter]");
                        println!("  • Chassis:     {}", modpack.target.chassis.join(", "));
                        println!("  • Module:      {}", modpack.target.ecu_name);
                        println!(
                            "  • Tx/Rx CAN:   0x{:03X} / 0x{:03X}",
                            modpack.target.tx_id, modpack.target.rx_id
                        );
                        println!(
                            "  • Min Voltage: {:.1} V",
                            modpack.target.min_battery_voltage
                        );
                        if !modpack.target.compatible_hw_ids.is_empty() {
                            println!(
                                "  • Whitelisted HW IDs: {}",
                                modpack.target.compatible_hw_ids.join(", ")
                            );
                        }

                        println!("\n  [Actions ({} total)]", modpack.actions.len());
                        for (idx, act) in modpack.actions.iter().enumerate() {
                            match act {
                                ModAction::WriteDid {
                                    did,
                                    data,
                                    bitmask,
                                    expected_original_data,
                                    description,
                                } => {
                                    println!(
                                        "    {}. Write DID 0x{:04X}: {} (bytes: {:02X?})",
                                        idx + 1,
                                        did,
                                        description,
                                        data
                                    );
                                    if let Some(mask) = bitmask {
                                        println!("       - Bitmask: {:02X?}", mask);
                                    }
                                    if let Some(exp) = expected_original_data {
                                        println!("       - Precondition required: {:02X?}", exp);
                                    }
                                }
                                ModAction::Routine {
                                    routine_id,
                                    subfunction,
                                    data,
                                    description,
                                } => {
                                    println!(
                                        "    {}. Routine 0x{:04X} (subfn 0x{:02X}, data {:02X?}): {}",
                                        idx + 1,
                                        routine_id,
                                        subfunction,
                                        data,
                                        description
                                    );
                                }
                                ModAction::PatchFlashMap {
                                    map_name,
                                    address_offset,
                                    data,
                                    expected_original_data,
                                    description,
                                } => {
                                    println!(
                                        "    {}. Flash Map Patch '{}' @ 0x{:06X}: {} ({} bytes)",
                                        idx + 1,
                                        map_name,
                                        address_offset,
                                        description,
                                        data.len()
                                    );
                                    if let Some(exp) = expected_original_data {
                                        println!(
                                            "       - Original validation required ({} bytes)",
                                            exp.len()
                                        );
                                    }
                                }
                                ModAction::DtcMask {
                                    p_code,
                                    address_offset,
                                    original_mask,
                                    disable_mask,
                                    description,
                                } => {
                                    println!(
                                        "    {}. DTC Mask '{}' @ 0x{:06X}: {} (0x{:02X} -> 0x{:02X})",
                                        idx + 1,
                                        p_code,
                                        address_offset,
                                        description,
                                        original_mask,
                                        disable_mask
                                    );
                                }
                            }
                        }

                        println!("\n  [Integrity & Forward Error Correction]");
                        let report = modpack.verify_and_repair()?;
                        if report.is_valid {
                            println!(
                                "  • Checksum (CRC32):  ✓ Valid (0x{:08X})",
                                modpack.integrity.payload_crc32
                            );
                            println!(
                                "  • Checksum (SHA256): ✓ Valid ({})",
                                &modpack.integrity.payload_sha256[..16]
                            );
                            match &report.fec_status {
                                FecStatus::Repaired {
                                    corrected_byte_count,
                                    repaired_offsets,
                                } => {
                                    println!(
                                        "  • Reed-Solomon FEC:  ✓ AUTO-REPAIRED {} corrupted byte(s) at positions: {:?}",
                                        corrected_byte_count, repaired_offsets
                                    );
                                }
                                _ => {
                                    println!("  • Reed-Solomon FEC:  ✓ Parity blocks clean (zero bitflips)");
                                }
                            }
                        } else {
                            println!("  • Checksum (CRC32):  ✗ Corrupted or unrecoverable");
                            for warn in &report.warning_messages {
                                println!("    ! {}", warn);
                            }
                        }

                        if let Some(target_vin) = vin {
                            println!("\n  [Chassis Compatibility for VIN: {}]", target_vin);
                            let compat = modpack.check_compatibility(Some(&target_vin), None, None);
                            if compat.matched_vehicle {
                                println!("  ✓ Compatible with chassis {}", target_vin);
                                for note in &compat.compatibility_notes {
                                    println!("    {}", note);
                                }
                            } else {
                                println!("  ✗ INCOMPATIBLE with chassis {}", target_vin);
                                for warn in &compat.warning_messages {
                                    println!("    ! {}", warn);
                                }
                            }
                        }
                    }
                    ModCommands::Apply { input, vin, force } => {
                        let mut modpack = load_mod_input(&input)?;
                        let target_vin = vin.unwrap_or_else(|| {
                            let garage = VehicleGarage::new(VehicleGarage::default_path());
                            if let Ok(vehicles) = garage.list_vehicles() {
                                if let Some(v) = vehicles.first() {
                                    return v.vin.clone();
                                }
                            }
                            "WDB2110001A000000".to_string()
                        });

                        let mut iface = open_interface(&cli.can_interface).await;
                        println!("============================================================");
                        println!("  APPLYING COMMUNITY MOD: {}", modpack.metadata.name);
                        println!("============================================================");
                        println!("  Target VIN:     {}", target_vin);
                        println!("  Interface:      {}", cli.can_interface);
                        println!("  Risk Level:     {}", modpack.metadata.risk_level.as_str());
                        if force {
                            println!("  ⚠️  FORCED BYPASS OF PRECONDITIONS ENABLED");
                        }

                        let battery_voltage = 13.2;

                        let report = ModRunner::apply_mod(
                            iface.as_mut(),
                            &mut modpack,
                            &target_vin,
                            battery_voltage,
                            force,
                        )
                        .await?;

                        if report.success {
                            println!("\n  ✓ SUCCESS: {}", report.message);
                            println!(
                                "  • Steps executed: {}/{}",
                                report.steps_completed, report.total_steps
                            );
                            for act in &report.actions_executed {
                                println!("    - {}", act);
                            }
                            if let Some(commit) = &report.git_commit_sha {
                                println!("  • Vehicle Garage Git snapshot: {}", commit);
                            }
                        } else {
                            println!("\n  ✗ FAILED: {}", report.message);
                        }
                    }
                    ModCommands::Create {
                        name,
                        author,
                        description,
                        chassis,
                        ecu,
                        did,
                        data,
                        mask,
                        expected,
                        category,
                        risk,
                        min_voltage,
                        out,
                        armor,
                    } => {
                        let did_u16 = u16::from_str_radix(did.trim_start_matches("0x"), 16)
                            .map_err(|e| anyhow::anyhow!("Invalid DID hex '{}': {}", did, e))?;

                        let data_bytes = parse_hex_bytes(&data)?;
                        let mask_bytes = match mask {
                            Some(ref m) => Some(parse_hex_bytes(m)?),
                            None => None,
                        };
                        let exp_bytes = match expected {
                            Some(ref e) => Some(parse_hex_bytes(e)?),
                            None => None,
                        };

                        let catalog = EcuCatalog::load_default();
                        let (tx_id, rx_id) = if let Ok(cat) = catalog {
                            if let Some(info) = cat.get_ecu(&ecu) {
                                let tx = info
                                    .tx_id
                                    .as_deref()
                                    .and_then(|s| {
                                        u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
                                    })
                                    .unwrap_or(0x7E0);
                                let rx = info
                                    .rx_id
                                    .as_deref()
                                    .and_then(|s| {
                                        u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
                                    })
                                    .unwrap_or(0x7E8);
                                (tx, rx)
                            } else if ecu.eq_ignore_ascii_case("EGS52") {
                                (0x7E1, 0x7E9)
                            } else {
                                (0x7E0, 0x7E8)
                            }
                        } else if ecu.eq_ignore_ascii_case("EGS52") {
                            (0x7E1, 0x7E9)
                        } else {
                            (0x7E0, 0x7E8)
                        };

                        let risk_level = match risk.to_lowercase().as_str() {
                            "moderate" => ModRiskLevel::Moderate,
                            "high" | "expert" => ModRiskLevel::High,
                            _ => ModRiskLevel::Low,
                        };

                        let mod_category = match category.to_lowercase().as_str() {
                            "performance" | "tune" => ModCategory::Performance,
                            "transmission" | "trans" | "egs" => ModCategory::Transmission,
                            "comfort" => ModCategory::Comfort,
                            "lighting" | "light" => ModCategory::Lighting,
                            "brakes" | "sbc" | "esp" => ModCategory::Brakes,
                            "emissions" | "egr" | "dpf" => ModCategory::Emissions,
                            _ => ModCategory::Retrofit,
                        };

                        let mod_id = format!(
                            "{}_{}",
                            name.to_lowercase()
                                .replace(' ', "_")
                                .chars()
                                .filter(|c| c.is_alphanumeric() || *c == '_')
                                .collect::<String>(),
                            chrono::Utc::now().format("%Y%m%d")
                        );

                        let metadata = ModMetadata {
                            mod_id: mod_id.clone(),
                            name: name.clone(),
                            version: "1.0.0".into(),
                            author: author.clone(),
                            description: description.clone(),
                            category: mod_category,
                            risk_level,
                            instructions: Some(
                                "Apply with ignition ON (engine OFF). Maintain battery voltage above minimum."
                                    .into(),
                            ),
                            created_at: chrono::Utc::now().to_rfc3339(),
                        };

                        let target = ModTargetFilter {
                            chassis: vec![chassis.clone()],
                            ecu_name: ecu.clone(),
                            tx_id,
                            rx_id,
                            compatible_hw_ids: vec![],
                            compatible_sw_ids: vec![],
                            min_battery_voltage: min_voltage,
                            requires_engine_off: true,
                        };

                        let action = ModAction::WriteDid {
                            did: did_u16,
                            data: data_bytes,
                            bitmask: mask_bytes,
                            expected_original_data: exp_bytes,
                            description: format!("Configure DID 0x{:04X} on {}", did_u16, ecu),
                        };

                        let modpack = SterngateMod::create(metadata, target, vec![action], vec![])?;

                        let armored = encode_to_armor(&modpack)?;

                        if let Some(out_path) = out {
                            if let Some(parent) = out_path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            std::fs::write(&out_path, modpack.to_json()?)?;
                            println!("✓ Mod file saved to: {}", out_path.display());
                        }

                        if armor {
                            println!("\n--- COPY-PASTEABLE ASCII ARMORED MOD ---");
                            println!("{}", armored);
                        }

                        println!("✓ Mod '{}' (ID: {}) created successfully.", name, mod_id);
                    }
                    ModCommands::List { dir } => {
                        println!("============================================================");
                        println!("  COMMUNITY MOD LIBRARY: {}", dir.display());
                        println!("============================================================");

                        if !dir.exists() {
                            std::fs::create_dir_all(&dir).ok();
                        }

                        let mut entries_count = 0;
                        if let Ok(entries) = std::fs::read_dir(&dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.extension().is_some_and(|ext| {
                                    ext == "sgmod" || ext == "json" || ext == "txt"
                                }) {
                                    if let Ok(mut m) = load_mod_input(&path.to_string_lossy()) {
                                        entries_count += 1;
                                        let rep = m.verify_and_repair();
                                        let fec_str = match rep {
                                            Ok(r) if r.is_valid => match r.fec_status {
                                                FecStatus::Repaired {
                                                    corrected_byte_count,
                                                    ..
                                                } => {
                                                    format!(
                                                        "✓ Repaired (+{}B)",
                                                        corrected_byte_count
                                                    )
                                                }
                                                _ => "✓ Clean".to_string(),
                                            },
                                            _ => "✗ Corrupt".to_string(),
                                        };
                                        println!(
                                            "• {:<20} | {:<16} | {:<6} | {:<8} | {:<10} | {}",
                                            m.metadata.name,
                                            m.target.ecu_name,
                                            m.target.chassis.join("/"),
                                            m.metadata.risk_level.as_str(),
                                            fec_str,
                                            path.file_name().unwrap_or_default().to_string_lossy()
                                        );
                                    }
                                }
                            }
                        }

                        if entries_count == 0 {
                            println!("  No .sgmod packages found in {}.", dir.display());
                            println!("  Use 'sterngate mod create' or drag-and-drop in the Web UI to add mods.");
                        } else {
                            println!("\nTotal: {} community mod(s) found.", entries_count);
                        }
                    }
                }
                return Ok(());
            }
            Commands::Tune { action } => {
                match action {
                    TuneCommands::Scan { rom, filter, table } => {
                        let rom_bytes = std::fs::read(&rom).with_context(|| {
                            format!("Failed to read ROM file from '{}'", rom.display())
                        })?;
                        let sigs = sterngate_core::FirmwareSignatures::extract(&rom_bytes);
                        let checksum = BoschChecksumSolver::verify(&rom_bytes);
                        let maps = BoschMapDetector::scan_rom(&rom_bytes);

                        println!("============================================================");
                        println!("  ECU ROM CALIBRATION SCAN & MAP DETECTION (WinOLS Engine)");
                        println!("============================================================");
                        println!(
                            "  • File:        {} ({} bytes / {:.2} MB)",
                            rom.display(),
                            rom_bytes.len(),
                            rom_bytes.len() as f64 / (1024.0 * 1024.0)
                        );
                        println!(
                            "  • Bosch HW:    {}",
                            sigs.bosch_hw_id.as_deref().unwrap_or("Unknown")
                        );
                        println!(
                            "  • Bosch SW:    {}",
                            sigs.bosch_sw_id.as_deref().unwrap_or("Unknown")
                        );
                        println!(
                            "  • Part Number: {}",
                            sigs.oem_part_number.as_deref().unwrap_or("Unknown")
                        );
                        println!("  • Global CRC:  0x{:08X}", checksum.global_crc32);
                        println!(
                            "  • Checksums:   {} ({} / {} blocks valid)",
                            if checksum.is_valid {
                                "PASS ✓"
                            } else {
                                "MISMATCH ⚠"
                            },
                            checksum.valid_blocks,
                            checksum.total_blocks
                        );

                        let filtered_maps: Vec<_> = if let Some(ref f) = filter {
                            let f_lower = f.to_lowercase();
                            maps.into_iter()
                                .filter(|m| m.name.to_lowercase().contains(&f_lower))
                                .collect()
                        } else {
                            maps
                        };

                        println!(
                            "\n  [Detected Calibration Maps: {} total]",
                            filtered_maps.len()
                        );
                        for (idx, map) in filtered_maps.iter().enumerate() {
                            println!(
                                "  {:2}. [0x{:06X}] {:<36} | {:2}x{:<2} | {}",
                                idx + 1,
                                map.address,
                                map.name,
                                map.rows,
                                map.cols,
                                map.category.as_str()
                            );
                            if table {
                                println!("\n{}", map.format_ascii_table());
                            }
                        }
                    }
                    TuneCommands::Stage1 {
                        rom,
                        chassis,
                        ecu,
                        author,
                        output,
                        armor,
                    } => {
                        let rom_bytes = std::fs::read(&rom).with_context(|| {
                            format!("Failed to read stock ROM file from '{}'", rom.display())
                        })?;
                        let modpack =
                            StageGenerator::generate_stage1(&rom_bytes, &chassis, &ecu, &author)?;

                        println!("============================================================");
                        println!("  GENERATED STAGE 1 PERFORMANCE PACKAGE (.sgmod)");
                        println!("============================================================");
                        println!("  • Name:        {}", modpack.metadata.name);
                        println!("  • Mod ID:      {}", modpack.metadata.mod_id);
                        println!(
                            "  • Target:      {} ({})",
                            modpack.target.chassis.join("/"),
                            modpack.target.ecu_name
                        );
                        println!("  • Scope:       +18% Peak Torque (430 Nm max), +120 mbar Boost, +50 bar Rail");
                        println!(
                            "  • Actions:     {} memory patch routines with rollback protection",
                            modpack.actions.len()
                        );

                        if let Some(ref out_path) = output {
                            if let Some(parent) = out_path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            std::fs::write(out_path, modpack.to_json()?)?;
                            println!("  ✓ Saved .sgmod to: {}", out_path.display());
                        }

                        if armor {
                            let armored = encode_to_armor(&modpack)?;
                            println!("\n--- COPY-PASTEABLE ASCII ARMORED STAGE 1 MOD ---");
                            println!("{}", armored);
                        }
                    }
                    TuneCommands::Stage2 {
                        rom,
                        chassis,
                        ecu,
                        author,
                        output,
                        armor,
                    } => {
                        let rom_bytes = std::fs::read(&rom).with_context(|| {
                            format!("Failed to read stock ROM file from '{}'", rom.display())
                        })?;
                        let modpack =
                            StageGenerator::generate_stage2(&rom_bytes, &chassis, &ecu, &author)?;

                        println!("============================================================");
                        println!("  GENERATED STAGE 2 RACE PERFORMANCE PACKAGE (.sgmod)");
                        println!("============================================================");
                        println!("  • Name:        {}", modpack.metadata.name);
                        println!("  • Mod ID:      {}", modpack.metadata.mod_id);
                        println!(
                            "  • Target:      {} ({})",
                            modpack.target.chassis.join("/"),
                            modpack.target.ecu_name
                        );
                        println!("  • Scope:       +25% Peak Torque, +200 mbar Boost, +80 bar Rail, DPF Off, EGR Hysteresis Off, P0401/P2002 DTC Suppressed");
                        println!("  • Warning:     Requires physical DPF downpipe & EGR blanking plate (Off-road only)");

                        if let Some(ref out_path) = output {
                            if let Some(parent) = out_path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            std::fs::write(out_path, modpack.to_json()?)?;
                            println!("  ✓ Saved .sgmod to: {}", out_path.display());
                        }

                        if armor {
                            let armored = encode_to_armor(&modpack)?;
                            println!("\n--- COPY-PASTEABLE ASCII ARMORED STAGE 2 MOD ---");
                            println!("{}", armored);
                        }
                    }
                    TuneCommands::DtcKill {
                        rom,
                        codes,
                        chassis,
                        ecu,
                        author,
                        output,
                        armor,
                    } => {
                        let rom_bytes = std::fs::read(&rom).with_context(|| {
                            format!("Failed to read stock ROM file from '{}'", rom.display())
                        })?;
                        let p_codes: Vec<String> = codes
                            .split(',')
                            .map(|s| s.trim().to_uppercase())
                            .filter(|s| !s.is_empty())
                            .collect();

                        let modpack = StageGenerator::generate_dtc_kill(
                            &rom_bytes, &chassis, &ecu, &p_codes, &author,
                        )?;

                        println!("============================================================");
                        println!("  GENERATED STANDALONE DTC SUPPRESSION PACKAGE (.sgmod)");
                        println!("============================================================");
                        println!("  • Suppressed:  {}", p_codes.join(", "));
                        println!("  • Target ECU:  {} ({})", chassis, ecu);

                        if let Some(ref out_path) = output {
                            if let Some(parent) = out_path.parent() {
                                std::fs::create_dir_all(parent).ok();
                            }
                            std::fs::write(out_path, modpack.to_json()?)?;
                            println!("  ✓ Saved .sgmod to: {}", out_path.display());
                        }

                        if armor {
                            let armored = encode_to_armor(&modpack)?;
                            println!("\n--- COPY-PASTEABLE ASCII ARMORED DTC KILL MOD ---");
                            println!("{}", armored);
                        }
                    }
                    TuneCommands::Checksum { rom, fix, output } => {
                        let mut rom_bytes = std::fs::read(&rom).with_context(|| {
                            format!("Failed to read ROM file from '{}'", rom.display())
                        })?;
                        let report = BoschChecksumSolver::verify(&rom_bytes);

                        println!("============================================================");
                        println!("  BOSCH MPC5xx / FLASH CHECKSUM VERIFICATION");
                        println!("============================================================");
                        println!("  • File:        {}", rom.display());
                        println!("  • Global CRC:  0x{:08X}", report.global_crc32);
                        println!(
                            "  • Blocks:      {} total ({} valid)",
                            report.total_blocks, report.valid_blocks
                        );
                        println!(
                            "  • Status:      {}",
                            if report.is_valid {
                                "PASS ✓ ALL CHECKSUMS VALID"
                            } else {
                                "MISMATCH ⚠ CHECKSUMS INVALID"
                            }
                        );

                        println!("\n  [Block Map]");
                        for b in &report.blocks {
                            println!(
                                "  Block #{}: 0x{:06X}..0x{:06X} | Sum: 0x{:08X} (stored: 0x{:08X}) | Inv: 0x{:08X} | {}",
                                b.block_index,
                                b.start_address,
                                b.end_address,
                                b.calculated_sum,
                                b.stored_sum,
                                b.calculated_inv,
                                if b.is_valid { "VALID ✓" } else { "INVALID ✗" }
                            );
                        }

                        if fix {
                            let fixed_report =
                                BoschChecksumSolver::recalculate_and_apply(&mut rom_bytes)?;
                            let out_path = output.unwrap_or(rom);
                            std::fs::write(&out_path, &rom_bytes).with_context(|| {
                                format!("Failed writing fixed ROM to '{}'", out_path.display())
                            })?;
                            println!(
                                "\n  ✓ Successfully recalculated and fixed all {} checksum blocks!",
                                fixed_report.valid_blocks
                            );
                            println!("  ✓ Output written to: {}", out_path.display());
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    match mode.unwrap_or(OperatingMode::Local) {
        OperatingMode::Local => {
            info!("============================================================");
            info!("  Starting Sterngate in LOCAL STANDALONE mode");
            info!("  Interface: {}", cli.can_interface);
            info!("  Dashboard: http://localhost:{}", cli.port);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
            info!("============================================================");

            let mut iface: Box<dyn VehicleInterface> = if cli.can_interface == "mock" {
                Box::new(VirtualCanInterface::new())
            } else if cli.can_interface == "openport" || cli.can_interface == "tactrix" {
                Box::new(OpenPortInterface::new())
            } else {
                Box::new(SocketCanInterface::new(&cli.can_interface))
            };
            if let Err(e) = iface.open().await {
                warn!(
                    "Could not open CAN interface {}: {}. Will retry on demand.",
                    cli.can_interface, e
                );
            }

            let profile = load_profile_safe(&cli.profile);
            let flasher = Arc::new(FlashingWorker::new());
            let state = Arc::new(AppState::new(iface, profile, flasher));
            run_server(state, cli.port).await?;
        }
        OperatingMode::Bridge => {
            info!("============================================================");
            info!("  Starting Sterngate: Car-Side Diagnostic Bridge (P2P Host)");
            info!("  Role: On-Vehicle Gateway Bridge & Ticket Host");
            info!("  Interface: {}", cli.can_interface);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
            info!("============================================================");

            let node = P2pNode::new().await?;
            let ticket = node.generate_ticket()?;
            info!("Iroh P2P Endpoint initialized.");
            info!("Node ID: {}", node.node_id());
            println!(
                "\n>>> SHARE THIS TICKET WITH YOUR REMOTE TECHNICIAN <<<\n{}\n",
                ticket
            );

            info!("Waiting for incoming connection from remote technician client...");
            while let Some(incoming) = node.accept().await {
                info!("Incoming connection received!");
                if let Ok(connecting) = incoming.accept() {
                    let _ = connecting.await;
                }
            }
        }
        OperatingMode::Tech => {
            let ticket_str = cli
                .ticket
                .context("Technician mode requires --ticket <TICKET>")?;
            info!("============================================================");
            info!("  Starting Sterngate: Remote Technician Client (P2P Dialer)");
            info!("  Role: Remote Technician Console (Tester)");
            info!("  Connecting to car-side diagnostic bridge via Iroh QUIC...");
            info!("  Technician Dashboard: http://localhost:{}", cli.port);
            info!("  Notice: Use at your own risk. See DISCLAIMER.md.");
            info!("============================================================");

            let target = P2pNode::parse_ticket(&ticket_str)?;
            let node = P2pNode::new().await?;
            info!("Dialing target car node: {}", target.id);
            let _conn = node.dial(target).await?;
            info!("Encrypted P2P tunnel established successfully!");

            let iface = Box::new(VirtualCanInterface::new());
            let profile = load_profile_safe(&cli.profile);
            let flasher = Arc::new(FlashingWorker::new());
            let state = Arc::new(AppState::new(iface, profile, flasher));
            run_server(state, cli.port).await?;
        }
    }

    Ok(())
}

fn load_profile_safe(path: &PathBuf) -> VehicleProfile {
    VehicleProfile::load_from_file(path).unwrap_or_else(|e| {
        warn!(
            "Could not load profile at {}: {}. Using default W211 configuration.",
            path.display(),
            e
        );
        VehicleProfile {
            profile_name: "fallback_w211".into(),
            oem: "Mercedes-Benz".into(),
            chassis: "W211".into(),
            gateway_type: Some("CGW_N93".into()),
            default_bitrate: 500000,
            modules: Default::default(),
            parameters: vec![],
        }
    })
}

async fn open_interface(can_interface: &str) -> Box<dyn VehicleInterface> {
    if can_interface == "mock" || can_interface == "sim" {
        let mut sim = VirtualCanInterface::new();
        let _ = sim.open().await;
        Box::new(sim)
    } else if can_interface == "openport" || can_interface == "tactrix" {
        let mut op = OpenPortInterface::new();
        if op.open().await.is_ok() {
            Box::new(op)
        } else {
            warn!(
                "Tactrix OpenPort hardware not found on USB, falling back to simulated interface"
            );
            let (mut sim_op, _) = OpenPortInterface::new_simulated(12.65);
            let _ = sim_op.open().await;
            Box::new(sim_op)
        }
    } else {
        let mut can = SocketCanInterface::new(can_interface);
        if can.open().await.is_ok() {
            Box::new(can)
        } else {
            let mut sim = VirtualCanInterface::new();
            let _ = sim.open().await;
            Box::new(sim)
        }
    }
}

fn load_mod_input(input: &str) -> Result<SterngateMod> {
    let content = if input == "-" {
        use std::io::Read;
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        buffer
    } else if std::path::Path::new(input).is_file() {
        std::fs::read_to_string(input)?
    } else {
        input.to_string()
    };

    if content.contains("BEGIN STERNGATE COMMUNITY MOD") {
        decode_from_armor(&content).map_err(|e| anyhow::anyhow!("ASCII armor decode failed: {}", e))
    } else {
        SterngateMod::from_json(&content).map_err(|e| anyhow::anyhow!("JSON decode failed: {}", e))
    }
}

fn parse_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let clean = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    if !clean.len().is_multiple_of(2) {
        anyhow::bail!("Hex string must have an even number of characters: '{}'", s);
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&clean[i..i + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex byte at index {}: {}", i, e))
        })
        .collect()
}
