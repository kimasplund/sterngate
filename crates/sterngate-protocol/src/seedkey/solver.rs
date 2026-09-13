use crate::seedkey::daimler::DaimlerSeedKey;
use sterngate_core::{Result, SterngateError};

pub trait SeedKeySolver: Send + Sync {
    fn name(&self) -> &str;
    fn compute_key(&self, level: u8, seed: &[u8]) -> Result<Vec<u8>>;
}

pub struct DaimlerSolver;

impl SeedKeySolver for DaimlerSolver {
    fn name(&self) -> &str {
        "Daimler_Standard"
    }

    fn compute_key(&self, level: u8, seed: &[u8]) -> Result<Vec<u8>> {
        match level {
            1 => DaimlerSeedKey::calculate_level1(seed).map(|k| k.to_vec()),
            3 => DaimlerSeedKey::calculate_level3(seed).map(|k| k.to_vec()),
            0x0B => DaimlerSeedKey::calculate_level0b(seed).map(|k| k.to_vec()),
            _ => Err(SterngateError::SecurityAccessDenied(format!(
                "Unsupported security level 0x{:02X} for Daimler",
                level
            ))),
        }
    }
}

pub fn get_solver_for_algorithm(algo: &str) -> Box<dyn SeedKeySolver> {
    match algo.to_lowercase().as_str() {
        "daimler_level1" | "daimler_standard" | "daimler" => Box::new(DaimlerSolver),
        _ => Box::new(DaimlerSolver),
    }
}
