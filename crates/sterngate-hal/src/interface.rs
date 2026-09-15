use async_trait::async_trait;
use sterngate_core::{CanFrame, Result};

#[async_trait]
pub trait VehicleInterface: Send + Sync {
    async fn open(&mut self) -> Result<()>;
    async fn send(&mut self, frame: CanFrame) -> Result<()>;
    async fn recv(&mut self) -> Result<CanFrame>;
    async fn close(&mut self) -> Result<()>;
    fn name(&self) -> &str;
    fn is_connected(&self) -> bool;

    /// Measure the vehicle battery voltage in volts, if this adapter has a
    /// sensor. `Ok(None)` means "cannot measure": callers that gate writes on
    /// voltage must refuse rather than assume. A simulated reading is not a
    /// measurement.
    async fn measure_battery_voltage(&mut self) -> Result<Option<f32>> {
        Ok(None)
    }
}
