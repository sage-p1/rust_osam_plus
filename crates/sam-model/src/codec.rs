use crate::SamError;

/// Converts model values to and from fixed-size cryptographic blocks.
pub trait BlockCodec<V, const B: usize> {
    /// Encodes one value into exactly `B` bytes.
    fn encode(&self, value: &V) -> Result<[u8; B], SamError>;

    /// Decodes one fixed-size block.
    fn decode(&self, bytes: &[u8; B]) -> Result<V, SamError>;
}

/// Little-endian codec for tests and numeric benchmark records.
#[derive(Clone, Copy, Debug, Default)]
pub struct U64Codec;

impl<const B: usize> BlockCodec<u64, B> for U64Codec {
    fn encode(&self, value: &u64) -> Result<[u8; B], SamError> {
        if B < size_of::<u64>() {
            return Err(SamError::Backend(
                "u64 requires a block of at least eight bytes".to_string(),
            ));
        }
        let mut block = [0_u8; B];
        block[..8].copy_from_slice(&value.to_le_bytes());
        Ok(block)
    }

    fn decode(&self, bytes: &[u8; B]) -> Result<u64, SamError> {
        if B < size_of::<u64>() {
            return Err(SamError::Backend(
                "u64 requires a block of at least eight bytes".to_string(),
            ));
        }
        Ok(u64::from_le_bytes(bytes[..8].try_into().unwrap()))
    }
}
