use crate::Fault;
use symphonia_script::Width;

/// Script-addressed state, shared explicitly by the scheduler. This is the
/// script data region. Native objects use handles.
pub struct Memory(Box<[u8; 65536]>);
impl Default for Memory {
    fn default() -> Self {
        Self(Box::new([0; 65536]))
    }
}
impl Memory {
    /// Copy a byte range without interpreting its values or changing other memory.
    pub fn copy_from(&mut self, source: &Self, range: std::ops::Range<u16>) -> Result<(), Fault> {
        let bytes = usize::from(range.start)..usize::from(range.end);
        let values = source
            .0
            .get(bytes.clone())
            .ok_or(Fault::Memory(range.start))?;
        self.0[bytes].copy_from_slice(values);
        Ok(())
    }

    pub fn read(&self, offset: u16, width: Width) -> Result<i32, Fault> {
        let start = usize::from(offset);
        let b = self
            .0
            .get(start..start + width.bytes())
            .ok_or(Fault::Memory(offset))?;
        Ok(match width {
            Width::S8 => i32::from(b[0] as i8),
            Width::S16 => i32::from(i16::from_be_bytes(b.try_into().unwrap())),
            Width::S32 => i32::from_be_bytes(b.try_into().unwrap()),
        })
    }
    pub fn write(&mut self, offset: u16, width: Width, value: i32) -> Result<(), Fault> {
        let start = usize::from(offset);
        let b = self
            .0
            .get_mut(start..start + width.bytes())
            .ok_or(Fault::Memory(offset))?;
        let bytes = value.to_be_bytes();
        b.copy_from_slice(&bytes[4 - width.bytes()..]);
        Ok(())
    }
}
