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

    /// Drain background broadcast frames until a diagnostic reply arrives.
    async fn recv_diag(sim: &mut VirtualCanInterface) -> CanFrame {
        loop {
            let f = sim.recv().await.unwrap();
            if f.id == 0x7E8 {
                return f;
            }
        }
    }

    #[tokio::test]
    async fn test_virtual_can_read_memory_by_address_returns_requested_length() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();

        // UDS 0x23 with ALFID 0x24: 8-byte request -> First Frame + one Consecutive Frame.
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let fc = recv_diag(&mut sim).await;
        assert_eq!(fc.data[0], 0x30, "expected Flow Control");

        // The CF carries the 2-byte length: 3 bytes requested.
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x03]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..5], &[0x04, 0x63, 0x00, 0x00, 0x00]);
    }

    #[tokio::test]
    async fn test_virtual_can_read_memory_by_address_refuses_more_than_six_bytes() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let _fc = recv_diag(&mut sim).await;
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x07]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x23, 0x31]);
    }

    #[tokio::test]
    async fn test_virtual_can_failing_services_answer_nrc() {
        // Single-frame service in the failing set
        let mut sim = VirtualCanInterface::with_failing_services(&[0x22]);
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(0x7E0, &[0x03, 0x22, 0xF1, 0x91]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x22, 0x31]);

        // Multi-frame service in the failing set
        let mut sim = VirtualCanInterface::with_failing_services(&[0x23]);
        sim.open().await.unwrap();
        sim.send(CanFrame::new_standard(
            0x7E0,
            &[0x10, 0x08, 0x23, 0x24, 0x00, 0x1C, 0x10, 0x00],
        ))
        .await
        .unwrap();
        let _fc = recv_diag(&mut sim).await;
        sim.send(CanFrame::new_standard(0x7E0, &[0x21, 0x00, 0x03]))
            .await
            .unwrap();
        let resp = recv_diag(&mut sim).await;
        assert_eq!(&resp.data[..4], &[0x03, 0x7F, 0x23, 0x31]);
    }

    #[tokio::test]
    async fn test_virtual_can_cannot_measure_voltage() {
        let mut sim = VirtualCanInterface::new();
        sim.open().await.unwrap();
        assert_eq!(sim.measure_battery_voltage().await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_simulated_openport_is_not_a_measurement() {
        let (mut op, _feed) = OpenPortInterface::new_simulated(12.65);
        op.open().await.unwrap();
        assert_eq!(op.measure_battery_voltage().await.unwrap(), None);
    }
}
