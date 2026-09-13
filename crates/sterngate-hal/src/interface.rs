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
}
