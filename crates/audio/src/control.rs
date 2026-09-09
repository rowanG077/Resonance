//! Fourteen-bit selector arithmetic for musical controls.
use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug)]
pub enum Combine {
    Set,
    Add,
    Multiply,
    Subtract,
}

#[derive(Clone, Copy, Debug)]
pub struct Term {
    /// Signed inputs are centered at zero; unsigned inputs span 0..16383.
    pub value: i16,
    pub signed: bool,
    pub scale: i32,
    pub combine: Combine,
}

pub fn evaluate(terms: &[Term]) -> Result<u16> {
    ensure!(
        terms
            .first()
            .is_some_and(|v| matches!(v.combine, Combine::Set)),
        "selector must begin with Set"
    );
    let mut accum = 0i32;
    let mut signed_accum = false;
    for term in terms {
        ensure!(
            term.signed || (0..=16383).contains(&term.value),
            "invalid unsigned controller"
        );
        ensure!(
            term.signed || term.scale >= 0,
            "negative unsigned selector scales are not supported yet"
        );
        let scaled = (i64::from(term.value) * i64::from(term.scale >> 1)) >> 15;
        let value = if term.signed {
            scaled.clamp(-8192, 8191)
        } else {
            scaled.clamp(0, 16383)
        } as i32;
        match term.combine {
            Combine::Set => {
                accum = value + if term.signed { 8192 } else { 0 };
                signed_accum = term.signed;
            }
            Combine::Add | Combine::Subtract => {
                let value = if matches!(term.combine, Combine::Subtract) {
                    -value
                } else {
                    value
                };
                accum = (accum + value).clamp(0, 16383);
            }
            Combine::Multiply => {
                if term.signed {
                    let product = if signed_accum {
                        ((accum - 8192) * value) >> 13
                    } else {
                        // Original uses a logical shift in this mixed-sign case.
                        ((accum * value) as u32 >> 13) as i32
                    };
                    accum = product.clamp(-8192, 8191) + 8192;
                    signed_accum = true;
                } else if signed_accum {
                    accum = (((accum - 8192) * value) >> 14).clamp(-8192, 8191) + 8192;
                } else {
                    accum = ((accum * value) >> 14).min(16383);
                }
            }
        }
    }
    Ok(accum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_observed_volume_and_zero_scale_auxiliary_selector() {
        let volume = Term {
            value: 127 << 7,
            signed: false,
            scale: 65536,
            combine: Combine::Set,
        };
        assert_eq!(
            evaluate(&[
                volume,
                Term {
                    combine: Combine::Multiply,
                    ..volume
                }
            ])
            .unwrap(),
            16129
        );
        assert_eq!(
            evaluate(&[Term {
                value: -700,
                signed: true,
                scale: 0,
                combine: Combine::Set
            }])
            .unwrap(),
            8192
        );
        assert_eq!(
            evaluate(&[
                Term {
                    value: 4000,
                    signed: true,
                    scale: 65536,
                    combine: Combine::Set
                },
                Term {
                    value: 8192,
                    signed: false,
                    scale: 65536,
                    combine: Combine::Multiply
                }
            ])
            .unwrap(),
            10192
        );
        assert!(
            evaluate(&[Term {
                combine: Combine::Multiply,
                ..volume
            }])
            .is_err()
        );
    }
}
