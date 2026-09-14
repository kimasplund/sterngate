use std::path::PathBuf;
use std::sync::Arc;
use sterngate_core::{EcuCatalog, FirmwareVault, TelemetrySnapshot, VehicleProfile};
use sterngate_hal::VehicleInterface;
use sterngate_protocol::{FlashingWorker, TransactionGate};
use tokio::sync::{broadcast, Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub interface: Arc<Mutex<Box<dyn VehicleInterface>>>,
    pub profile: Arc<RwLock<VehicleProfile>>,
    pub flasher: Arc<FlashingWorker>,
    pub gate: Arc<TransactionGate>,
    pub telemetry_tx: broadcast::Sender<TelemetrySnapshot>,
    pub recorder: Arc<crate::recorder::FlightRecorder>,
    pub catalog: Arc<Option<EcuCatalog>>,
    pub compressor_guard: Arc<std::sync::Mutex<sterngate_core::CompressorProtectionGuard>>,
    /// Root directory the firmware vault is confined to. Vault scans and
    /// staging may not resolve outside it, so a caller-supplied path can never
    /// reach arbitrary files. Configured via --vault or STERNGATE_VAULT_ROOT.
    pub vault_root: PathBuf,
}

impl AppState {
    pub fn new(
        interface: Box<dyn VehicleInterface>,
        profile: VehicleProfile,
        flasher: Arc<FlashingWorker>,
    ) -> Self {
        let (tx, _) = broadcast::channel(128);
        let catalog = EcuCatalog::load_default().ok();
        Self {
            interface: Arc::new(Mutex::new(interface)),
            profile: Arc::new(RwLock::new(profile)),
            flasher,
            gate: Arc::new(TransactionGate::new()),
            telemetry_tx: tx,
            recorder: Arc::new(crate::recorder::FlightRecorder::default()),
            catalog: Arc::new(catalog),
            compressor_guard: Arc::new(std::sync::Mutex::new(
                sterngate_core::CompressorProtectionGuard::default(),
            )),
            vault_root: FirmwareVault::default_root(),
        }
    }

    pub fn with_catalog(mut self, catalog: Option<EcuCatalog>) -> Self {
        self.catalog = Arc::new(catalog);
        self
    }

    /// Override the firmware vault root. `None` keeps the configured default,
    /// so callers can forward an optional CLI flag directly.
    pub fn with_vault_root(mut self, root: Option<PathBuf>) -> Self {
        if let Some(root) = root {
            self.vault_root = root;
        }
        self
    }
}
