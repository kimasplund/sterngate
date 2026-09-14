pub mod doip;
pub mod interface;
pub mod j2534;
pub mod mock;
pub mod openport;
pub mod openport_codec;
pub mod socketcan;

pub use doip::DoIpInterface;
pub use interface::VehicleInterface;
pub use j2534::J2534Interface;
pub use mock::VirtualCanInterface;
pub use openport::OpenPortInterface;
pub use socketcan::SocketCanInterface;

#[cfg(test)]
mod tests {
    use super::*;
    use sterngate_core::CanFrame;

    #[tokio::test]
    async fn test_virtual_can_lifecycle() {
        let mut sim = VirtualCanInterface::new();
        assert!(!sim.is_connected());
        sim.open().await.unwrap();
        assert!(sim.is_connected());

        // Send DiagnosticSessionControl Extended (0x10 03)
        let req = CanFrame::new_standard(0x7E0, &[0x02, 0x10, 0x03]);
        sim.send(req).await.unwrap();

        // Recv response from EDC16 (0x7E8)
        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E8);
        assert_eq!(resp.data[1], 0x50); // Positive response to 0x10
        assert_eq!(resp.data[2], 0x03);

        sim.close().await.unwrap();
        assert!(!sim.is_connected());
    }

    #[tokio::test]
    async fn test_virtual_can_read_egs_fluid_temp() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // Send ReadDataByIdentifier for Transmission Fluid Temp (0x2001) to EGS52 (0x7E1)
        let req = CanFrame::new_standard(0x7E1, &[0x03, 0x22, 0x20, 0x01]);
        sim.send(req).await.unwrap();

        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E9);
        assert_eq!(resp.data[1], 0x62); // Positive response to 0x22
        assert_eq!(resp.data[2], 0x20);
        assert_eq!(resp.data[3], 0x01);
        assert_eq!(resp.data[4], 120); // 120 - 40 = 80°C (Exact target!)
    }

    #[tokio::test]
    async fn test_virtual_can_routine_control() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // Send RoutineControl (0x31) Start (0x01) Fuel Pump Prime (0xFF01) to EDC16 (0x7E0)
        let req = CanFrame::new_standard(0x7E0, &[0x04, 0x31, 0x01, 0xFF, 0x01]);
        sim.send(req).await.unwrap();

        let resp = sim.recv().await.unwrap();
        assert_eq!(resp.id, 0x7E8);
        assert_eq!(resp.data[1], 0x71); // Positive response to 0x31
        assert_eq!(resp.data[2], 0x01); // sub-function startRoutine
        assert_eq!(resp.data[3], 0xFF); // routine high byte
        assert_eq!(resp.data[4], 0x01); // routine low byte
        assert_eq!(resp.data[5], 0x00); // routine status ok
    }
}
