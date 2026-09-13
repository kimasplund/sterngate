use std::sync::Arc;
use sterngate_core::{TelemetrySnapshot, VehicleProfile};
use sterngate_hal::VehicleInterface;
use sterngate_protocol::FlashingWorker;
use tokio::sync::{broadcast, Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub interface: Arc<Mutex<Box<dyn VehicleInterface>>>,
    pub profile: Arc<RwLock<VehicleProfile>>,
    pub flasher: Arc<FlashingWorker>,
    pub telemetry_tx: broadcast::Sender<TelemetrySnapshot>,
}

impl AppState {
    pub fn new(
        interface: Box<dyn VehicleInterface>,
        profile: VehicleProfile,
        flasher: Arc<FlashingWorker>,
    ) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            interface: Arc::new(Mutex::new(interface)),
            profile: Arc::new(RwLock::new(profile)),
            flasher,
            telemetry_tx: tx,
        }
    }
}
