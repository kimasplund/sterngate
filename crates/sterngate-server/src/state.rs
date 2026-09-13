use std::sync::Arc;
use sterngate_core::{EcuCatalog, TelemetrySnapshot, VehicleProfile};
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
        }
    }

    pub fn with_catalog(mut self, catalog: Option<EcuCatalog>) -> Self {
        self.catalog = Arc::new(catalog);
        self
    }
}
