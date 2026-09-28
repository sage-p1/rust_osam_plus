use super::{MultiWriteCell, OriginalCell, OriginalNode, RaryCell, UNRESOLVED_RARY_INDEX};
use crate::{Address, BlockCodec, SamError};

/// Variable-length binary encoding used inside a fixed OSAM block.
pub trait ValueCodec<V>: Clone {
    /// Appends an encoded value to `output`.
    fn encode_value(&self, value: &V, output: &mut Vec<u8>) -> Result<(), SamError>;

    /// Decodes one value, advancing `input` past its representation.
    fn decode_value(&self, input: &mut &[u8]) -> Result<V, SamError>;
}

/// Fixed-size, zero-padded envelope for a [`ValueCodec`].
#[derive(Clone, Debug, Default)]
pub struct FixedSizeCodec<C> {
    inner: C,
}

impl<C> FixedSizeCodec<C> {
    /// Wraps a variable-size codec in a fixed-size block envelope.
    pub fn new(inner: C) -> Self {
        Self { inner }
    }

    /// Returns the inner variable-size codec.
    pub fn inner(&self) -> &C {
        &self.inner
    }
}

impl<V, C: ValueCodec<V>, const B: usize> BlockCodec<V, B> for FixedSizeCodec<C> {
    fn encode(&self, value: &V) -> Result<[u8; B], SamError> {
        if B < 4 {
            return Err(SamError::Backend(
                "fixed-size block needs a four-byte length prefix".into(),
            ));
        }
        let mut encoded = Vec::new();
        self.inner.encode_value(value, &mut encoded)?;
        if encoded.len() > B - 4 {
            return Err(SamError::Backend(format!(
                "encoded value requires {} bytes but block payload holds {}",
                encoded.len(),
                B - 4
            )));
        }
        let length =
            u32::try_from(encoded.len()).map_err(|error| SamError::Backend(error.to_string()))?;
        let mut block = [0_u8; B];
        block[..4].copy_from_slice(&length.to_le_bytes());
        block[4..4 + encoded.len()].copy_from_slice(&encoded);
        Ok(block)
    }

    fn decode(&self, bytes: &[u8; B]) -> Result<V, SamError> {
        if B < 4 {
            return Err(SamError::Backend(
                "fixed-size block needs a four-byte length prefix".into(),
            ));
        }
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        if length > B - 4 {
            return Err(SamError::Backend(
                "fixed-size block contains an invalid payload length".into(),
            ));
        }
        let mut input = &bytes[4..4 + length];
        let value = self.inner.decode_value(&mut input)?;
        if !input.is_empty() {
            return Err(SamError::Backend(
                "fixed-size block contains trailing encoded bytes".into(),
            ));
        }
        Ok(value)
    }
}

/// Variable-size little-endian `u64` codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct U64ValueCodec;

impl ValueCodec<u64> for U64ValueCodec {
    fn encode_value(&self, value: &u64, output: &mut Vec<u8>) -> Result<(), SamError> {
        output.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<u64, SamError> {
        Ok(u64::from_le_bytes(take(input, 8)?.try_into().unwrap()))
    }
}

/// UTF-8 string codec with a four-byte length prefix.
#[derive(Clone, Copy, Debug, Default)]
pub struct StringValueCodec;

impl ValueCodec<String> for StringValueCodec {
    fn encode_value(&self, value: &String, output: &mut Vec<u8>) -> Result<(), SamError> {
        put_length(value.len(), output)?;
        output.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<String, SamError> {
        let length = get_length(input)?;
        String::from_utf8(take(input, length)?.to_vec())
            .map_err(|error| SamError::Backend(error.to_string()))
    }
}

/// Binary multi-write cell codec using `C` for root values.
#[derive(Clone, Debug, Default)]
pub struct MultiWriteCellValueCodec<C> {
    payload: C,
}

impl<C> MultiWriteCellValueCodec<C> {
    /// Creates a cell codec around `payload`.
    pub fn new(payload: C) -> Self {
        Self { payload }
    }
}

impl<V, C: ValueCodec<V>> ValueCodec<MultiWriteCell<V>> for MultiWriteCellValueCodec<C> {
    fn encode_value(
        &self,
        value: &MultiWriteCell<V>,
        output: &mut Vec<u8>,
    ) -> Result<(), SamError> {
        match value {
            MultiWriteCell::Root(value) => {
                output.push(0);
                self.payload.encode_value(value, output)
            }
            MultiWriteCell::Inner { parent, sibling } => {
                output.push(1);
                put_address(*parent, output);
                put_address(*sibling, output);
                Ok(())
            }
        }
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<MultiWriteCell<V>, SamError> {
        match get_u8(input)? {
            0 => Ok(MultiWriteCell::Root(self.payload.decode_value(input)?)),
            1 => Ok(MultiWriteCell::Inner {
                parent: get_address(input)?,
                sibling: get_address(input)?,
            }),
            _ => Err(SamError::Backend("invalid multi-write cell tag".into())),
        }
    }
}

/// Original single-write queue/tree cell codec.
#[derive(Clone, Debug, Default)]
pub struct OriginalCellValueCodec<C> {
    payload: C,
}

impl<C> OriginalCellValueCodec<C> {
    /// Creates a cell codec around `payload`.
    pub fn new(payload: C) -> Self {
        Self { payload }
    }
}

impl<V, C: ValueCodec<V>> ValueCodec<OriginalCell<V>> for OriginalCellValueCodec<C> {
    fn encode_value(&self, value: &OriginalCell<V>, output: &mut Vec<u8>) -> Result<(), SamError> {
        match value {
            OriginalCell::Queue { value, next } => {
                output.push(0);
                put_address(*value, output);
                put_address(*next, output);
            }
            OriginalCell::Raw(value) => {
                output.push(2);
                self.payload.encode_value(value, output)?;
            }
            OriginalCell::Node(node) => {
                output.push(1);
                put_option_address(node.left_tail, output);
                put_option_address(node.right_tail, output);
                put_option_address(node.parent_head, output);
                output.push(u8::from(node.root));
                match &node.value {
                    Some(value) => {
                        output.push(1);
                        self.payload.encode_value(value, output)?;
                    }
                    None => output.push(0),
                }
                output.extend_from_slice(
                    &u64::try_from(node.count)
                        .map_err(|error| SamError::Backend(error.to_string()))?
                        .to_le_bytes(),
                );
            }
        }
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<OriginalCell<V>, SamError> {
        match get_u8(input)? {
            0 => Ok(OriginalCell::Queue {
                value: get_address(input)?,
                next: get_address(input)?,
            }),
            1 => {
                let left_tail = get_option_address(input)?;
                let right_tail = get_option_address(input)?;
                let parent_head = get_option_address(input)?;
                let root = match get_u8(input)? {
                    0 => false,
                    1 => true,
                    _ => return Err(SamError::Backend("invalid boolean encoding".into())),
                };
                let value = match get_u8(input)? {
                    0 => None,
                    1 => Some(self.payload.decode_value(input)?),
                    _ => return Err(SamError::Backend("invalid option encoding".into())),
                };
                let count =
                    usize::try_from(u64::from_le_bytes(take(input, 8)?.try_into().unwrap()))
                        .map_err(|error| SamError::Backend(error.to_string()))?;
                Ok(OriginalCell::Node(OriginalNode {
                    left_tail,
                    right_tail,
                    parent_head,
                    root,
                    value,
                    count,
                }))
            }
            2 => Ok(OriginalCell::Raw(self.payload.decode_value(input)?)),
            _ => Err(SamError::Backend("invalid original cell tag".into())),
        }
    }
}

/// R-ary multi-write cell codec using `C` for root values.
#[derive(Clone, Debug, Default)]
pub struct RaryCellValueCodec<C> {
    payload: C,
}

impl<C> RaryCellValueCodec<C> {
    /// Creates a cell codec around `payload`.
    pub fn new(payload: C) -> Self {
        Self { payload }
    }
}

impl<V, C: ValueCodec<V>> ValueCodec<RaryCell<V>> for RaryCellValueCodec<C> {
    fn encode_value(&self, value: &RaryCell<V>, output: &mut Vec<u8>) -> Result<(), SamError> {
        match value {
            RaryCell::Root(value) => {
                output.push(0);
                self.payload.encode_value(value, output)?;
            }
            // Compact node: tag, parent id, group length, one id per slot
            // (0 = empty) = 8b + 10 bytes. The node's own index is omitted:
            // readers recover it from where their address sits in `group`.
            RaryCell::Node { parent, group, .. } => {
                output.push(1);
                put_tree_address(Some(*parent), output)?;
                output.push(u8::try_from(group.len()).map_err(|_| {
                    SamError::Backend("r-ary groups are limited to 255 slots".into())
                })?);
                for member in group.iter() {
                    put_tree_address(*member, output)?;
                }
            }
        }
        Ok(())
    }

    fn decode_value(&self, input: &mut &[u8]) -> Result<RaryCell<V>, SamError> {
        match get_u8(input)? {
            0 => Ok(RaryCell::Root(self.payload.decode_value(input)?)),
            1 => {
                let parent = get_tree_address(input)?
                    .ok_or_else(|| SamError::Backend("r-ary node has no parent".into()))?;
                let length = usize::from(get_u8(input)?);
                let mut group = Vec::with_capacity(length);
                for _ in 0..length {
                    group.push(get_tree_address(input)?);
                }
                Ok(RaryCell::Node {
                    parent,
                    group: group.into(),
                    index: UNRESOLVED_RARY_INDEX,
                })
            }
            _ => Err(SamError::Backend("invalid r-ary cell tag".into())),
        }
    }
}

fn take<'a>(input: &mut &'a [u8], length: usize) -> Result<&'a [u8], SamError> {
    if input.len() < length {
        return Err(SamError::Backend("truncated fixed-size encoding".into()));
    }
    let (head, tail) = input.split_at(length);
    *input = tail;
    Ok(head)
}

fn get_u8(input: &mut &[u8]) -> Result<u8, SamError> {
    Ok(take(input, 1)?[0])
}

fn put_length(length: usize, output: &mut Vec<u8>) -> Result<(), SamError> {
    output.extend_from_slice(
        &u32::try_from(length)
            .map_err(|error| SamError::Backend(error.to_string()))?
            .to_le_bytes(),
    );
    Ok(())
}

fn get_length(input: &mut &[u8]) -> Result<usize, SamError> {
    Ok(u32::from_le_bytes(take(input, 4)?.try_into().unwrap()) as usize)
}

fn put_address(address: Address, output: &mut Vec<u8>) {
    match address {
        Address::Oblivious(identifier) => {
            output.push(0);
            output.extend_from_slice(&identifier.to_le_bytes());
        }
        Address::Plaintext(identifier) => {
            output.push(1);
            output.extend_from_slice(&identifier.to_le_bytes());
        }
    }
}

fn get_address(input: &mut &[u8]) -> Result<Address, SamError> {
    let kind = get_u8(input)?;
    let identifier = u64::from_le_bytes(take(input, 8)?.try_into().unwrap());
    match kind {
        0 => Ok(Address::Oblivious(identifier)),
        1 => Ok(Address::Plaintext(identifier)),
        _ => Err(SamError::Backend("invalid address kind".into())),
    }
}

/// Eight-byte pointer-tree address: pointer trees only hold oblivious
/// addresses, and identifiers start at one, so zero encodes `None`.
fn put_tree_address(address: Option<Address>, output: &mut Vec<u8>) -> Result<(), SamError> {
    let identifier = match address {
        None => 0,
        Some(Address::Oblivious(0)) => {
            return Err(SamError::Backend(
                "oblivious identifier zero is reserved".into(),
            ))
        }
        Some(Address::Oblivious(identifier)) => identifier,
        Some(Address::Plaintext(_)) => {
            return Err(SamError::Backend(
                "pointer-tree cells hold only oblivious addresses".into(),
            ))
        }
    };
    output.extend_from_slice(&identifier.to_le_bytes());
    Ok(())
}

fn get_tree_address(input: &mut &[u8]) -> Result<Option<Address>, SamError> {
    Ok(
        match u64::from_le_bytes(take(input, 8)?.try_into().unwrap()) {
            0 => None,
            identifier => Some(Address::Oblivious(identifier)),
        },
    )
}

fn put_option_address(address: Option<Address>, output: &mut Vec<u8>) {
    match address {
        Some(address) => {
            output.push(1);
            put_address(address, output);
        }
        None => output.push(0),
    }
}

fn get_option_address(input: &mut &[u8]) -> Result<Option<Address>, SamError> {
    match get_u8(input)? {
        0 => Ok(None),
        1 => get_address(input).map(Some),
        _ => Err(SamError::Backend("invalid option-address tag".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pointer::{CachedPointers, MultiWritePointer, MultiWritePointers, RaryPointer},
        AccessPolicy, AccessStrategy, DryRunSam, PathOsamSam, SingleAccessMachine,
    };

    fn round_trip<T: Clone + Eq + std::fmt::Debug, C: ValueCodec<T>, const B: usize>(
        codec: FixedSizeCodec<C>,
        value: T,
    ) {
        let block: [u8; B] = codec.encode(&value).unwrap();
        assert_eq!(codec.decode(&block).unwrap(), value);
    }

    #[test]
    fn all_pointer_cell_shapes_round_trip() {
        let a = Address::Oblivious(7);
        let b = Address::Plaintext(9);
        round_trip::<_, _, 64>(
            FixedSizeCodec::new(MultiWriteCellValueCodec::new(U64ValueCodec)),
            MultiWriteCell::Root(42),
        );
        round_trip::<_, _, 64>(
            FixedSizeCodec::new(MultiWriteCellValueCodec::new(U64ValueCodec)),
            MultiWriteCell::<u64>::Inner {
                parent: a,
                sibling: b,
            },
        );
        // R-ary nodes decode with an unresolved index (see the dedicated test).
        let rary = FixedSizeCodec::new(RaryCellValueCodec::new(U64ValueCodec));
        let node = RaryCell::<u64>::Node {
            parent: a,
            group: vec![Some(a), None, Some(Address::Oblivious(11)), None].into(),
            index: 0,
        };
        let block: [u8; 128] = rary.encode(&node).unwrap();
        let RaryCell::Node { index, .. } = rary.decode(&block).unwrap() else {
            panic!("decoded r-ary node as a root");
        };
        assert_eq!(index, UNRESOLVED_RARY_INDEX);
        round_trip::<_, _, 128>(
            FixedSizeCodec::new(OriginalCellValueCodec::new(U64ValueCodec)),
            OriginalCell::Node(OriginalNode {
                left_tail: Some(a),
                right_tail: None,
                parent_head: Some(b),
                root: true,
                value: Some(42),
                count: 3,
            }),
        );
    }

    #[test]
    fn compact_rary_node_is_eight_bytes_per_slot_plus_ten() {
        let codec = RaryCellValueCodec::new(U64ValueCodec);
        for branching_factor in [2_usize, 4, 6, 64] {
            let mut group = vec![None; branching_factor];
            group[0] = Some(Address::Oblivious(3));
            group[1] = Some(Address::Oblivious(4));
            let mut encoded = Vec::new();
            codec
                .encode_value(
                    &RaryCell::<u64>::Node {
                        parent: Address::Oblivious(9),
                        group: group.into(),
                        index: 1,
                    },
                    &mut encoded,
                )
                .unwrap();
            assert_eq!(encoded.len(), 8 * branching_factor + 10);
        }
        // Six slots now fit a 64-byte block (62 bytes with the envelope).
        let mut group = vec![None; 6];
        group[5] = Some(Address::Oblivious(5));
        let block: Result<[u8; 64], _> =
            FixedSizeCodec::new(codec.clone()).encode(&RaryCell::<u64>::Node {
                parent: Address::Oblivious(2),
                group: group.into(),
                index: 5,
            });
        assert!(block.is_ok());
        let mut encoded = Vec::new();
        let plaintext = RaryCell::<u64>::Node {
            parent: Address::Plaintext(2),
            group: vec![Some(Address::Oblivious(3)), None].into(),
            index: 0,
        };
        assert!(codec.encode_value(&plaintext, &mut encoded).is_err());
    }

    #[test]
    fn oversized_encoding_is_rejected() {
        let codec = FixedSizeCodec::new(StringValueCodec);
        let result: Result<[u8; 16], _> = codec.encode(&"too long for this block".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn encrypted_multiwrite_pointer_snapshot_round_trip() {
        let mut dry = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut original = MultiWritePointer::new(&mut dry, 42_u64).unwrap();
        let mut alias = original.copy(&mut dry).unwrap();
        let codec = FixedSizeCodec::new(MultiWriteCellValueCodec::new(U64ValueCodec));
        let mut encrypted = PathOsamSam::<MultiWriteCell<u64>, _, 64, 4, 1>::from_snapshot(
            dry.snapshot(),
            64,
            40,
            true,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::STANDARD,
            true,
            17,
            codec,
        )
        .unwrap();
        assert_eq!(original.get(&mut encrypted).unwrap(), 42);
        alias.put(&mut encrypted, 99).unwrap();
        assert_eq!(original.get(&mut encrypted).unwrap(), 99);
    }

    #[test]
    fn encrypted_rary_pointer_snapshot_round_trip() {
        let mut dry = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut pointers = RaryPointer::install(&mut dry, 5_u64, 8, 4).unwrap();
        let codec = FixedSizeCodec::new(RaryCellValueCodec::new(U64ValueCodec));
        let mut encrypted = PathOsamSam::<RaryCell<u64>, _, 128, 4, 1>::from_snapshot(
            dry.snapshot(),
            128,
            80,
            true,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::MULTI_WRITE_RARY,
            true,
            23,
            codec,
        )
        .unwrap();
        pointers[3].put(&mut encrypted, 8).unwrap();
        for pointer in &mut pointers {
            assert_eq!(pointer.get(&mut encrypted).unwrap(), 8);
        }
    }

    #[test]
    fn encrypted_original_pointer_snapshot_round_trip() {
        use crate::pointer::OriginalPointer;

        let mut dry = DryRunSam::new(AccessPolicy::SINGLE_WRITE);
        let mut original = OriginalPointer::new(&mut dry, 12_u64).unwrap();
        let mut alias = original.copy(&mut dry).unwrap();
        let codec = FixedSizeCodec::new(OriginalCellValueCodec::new(U64ValueCodec));
        let mut encrypted = PathOsamSam::<OriginalCell<u64>, _, 128, 4, 1>::from_snapshot(
            dry.snapshot(),
            128,
            80,
            true,
            AccessPolicy::SINGLE_WRITE,
            AccessStrategy::STANDARD,
            true,
            29,
            codec,
        )
        .unwrap();
        assert_eq!(original.get(&mut encrypted).unwrap(), 12);
        alias.put(&mut encrypted, 18).unwrap();
        assert_eq!(original.get(&mut encrypted).unwrap(), 18);
    }

    #[test]
    fn recursive_values_use_the_same_fixed_envelope() {
        round_trip::<_, _, 16>(FixedSizeCodec::new(U64ValueCodec), 42_u64);
    }

    #[test]
    fn cached_pointer_runs_after_encrypted_snapshot_handoff() {
        let mut dry = DryRunSam::new(AccessPolicy::MULTI_WRITE);
        let mut cache = CachedPointers::new(MultiWritePointers);
        let mut pointer = cache.new_pointer(&mut dry, 4_u64).unwrap();
        let codec = FixedSizeCodec::new(MultiWriteCellValueCodec::new(U64ValueCodec));
        let mut encrypted = PathOsamSam::<MultiWriteCell<u64>, _, 64, 4, 1>::from_snapshot(
            dry.snapshot(),
            64,
            40,
            true,
            AccessPolicy::MULTI_WRITE,
            AccessStrategy::STANDARD,
            true,
            31,
            codec,
        )
        .unwrap();
        let mut object = cache.get(&mut encrypted, &mut pointer).unwrap().unwrap();
        *cache.value_mut(&object).unwrap() = 10;
        let reads_before_release = encrypted.stats().operations.reads;
        cache.release(&mut encrypted, &mut object).unwrap();
        assert_eq!(encrypted.stats().operations.reads, reads_before_release);

        let mut loaded = cache.get(&mut encrypted, &mut pointer).unwrap().unwrap();
        assert_eq!(*cache.value(&loaded).unwrap(), 10);
        cache.release(&mut encrypted, &mut loaded).unwrap();
    }
}
