//! Lossless scalar operands and uninterpreted spans shared by physical asset records.
use anyhow::{Result, ensure};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FloatOperand {
    Value(f32),
    Bits { bits: u32 },
}

impl FloatOperand {
    pub fn from_bits(bits: u32) -> Self {
        let value = f32::from_bits(bits);
        if value.is_finite() {
            Self::Value(value)
        } else {
            Self::Bits { bits }
        }
    }

    pub fn bits(self) -> u32 {
        match self {
            Self::Value(value) => value.to_bits(),
            Self::Bits { bits } => bits,
        }
    }

    pub fn finite(self) -> Result<f32> {
        let value = f32::from_bits(self.bits());
        ensure!(value.is_finite(), "non-finite active float operand");
        Ok(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Storage {
    pub offset: usize,
    pub bytes: Vec<u8>,
}
