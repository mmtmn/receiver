//! Byte framing, Reed--Solomon erasure coding, and the 6-bit visual bitstream.
//!
//! The geometry is intentionally exact: 205 columns x 204 data rows contain
//! 41,820 six-bit cells, which is exactly 31,365 encoded bytes.  Those bytes
//! are 123 independently recoverable RS(255, 204) codewords.  The final row is
//! reserved for color/shape calibration and carries no file data.

use anyhow::{Result, anyhow, bail};
use rayon::prelude::*;
use reed_solomon::{Decoder, Encoder};

pub const GRID_COLS: usize = 205;
pub const DATA_ROWS: usize = 204;
pub const GRID_ROWS: usize = DATA_ROWS + 1;
pub const DATA_CELLS: usize = GRID_COLS * DATA_ROWS;
pub const CALIBRATION_CELLS: usize = GRID_COLS;
pub const BITS_PER_CELL: usize = 6;
pub const CODED_BYTES: usize = DATA_CELLS * BITS_PER_CELL / 8;

pub const RS_DATA: usize = 204;
pub const RS_PARITY: usize = 51;
pub const RS_TOTAL: usize = RS_DATA + RS_PARITY;
pub const RS_BLOCKS: usize = CODED_BYTES / RS_TOTAL;
pub const DATA_BYTES: usize = RS_BLOCKS * RS_DATA;

const MAGIC: [u8; 8] = *b"OPTLAB01";
const HEADER_BYTES: usize = 20;
pub const PAYLOAD_BYTES: usize = DATA_BYTES - HEADER_BYTES;

const _: () = assert!((DATA_CELLS * BITS_PER_CELL).is_multiple_of(8));
const _: () = assert!(CODED_BYTES.is_multiple_of(RS_TOTAL));

#[derive(Clone, Debug)]
pub struct EncodedFrame {
    pub sequence: u32,
    pub payload: Vec<u8>,
    /// Six-bit values. The first DATA_CELLS are protected data; the last row
    /// contains a repeating sequence of all 64 shape/color combinations.
    pub symbols: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct DecodeDiagnostics {
    pub erased_cells: usize,
    pub erased_bytes: usize,
    pub max_codeword_erasures: usize,
    pub reconstructed_codewords: usize,
    pub corrected_errata: usize,
}

#[derive(Clone, Debug)]
pub struct DecodedFrame {
    pub sequence: u32,
    pub payload: Vec<u8>,
    pub diagnostics: DecodeDiagnostics,
}

pub fn encode_frame(sequence: u32, payload: &[u8]) -> Result<EncodedFrame> {
    if payload.len() > PAYLOAD_BYTES {
        bail!(
            "payload is {} bytes; this profile carries at most {}",
            payload.len(),
            PAYLOAD_BYTES
        );
    }

    let mut framed = vec![0_u8; DATA_BYTES];
    framed[..8].copy_from_slice(&MAGIC);
    framed[8..12].copy_from_slice(&sequence.to_le_bytes());
    framed[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    framed[16..20].copy_from_slice(&crc32(payload).to_le_bytes());
    framed[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(payload);

    let mut codewords = vec![[0_u8; RS_TOTAL]; RS_BLOCKS];
    codewords
        .par_iter_mut()
        .enumerate()
        .for_each(|(block, codeword)| {
            let encoded =
                Encoder::new(RS_PARITY).encode(&framed[block * RS_DATA..(block + 1) * RS_DATA]);
            codeword.copy_from_slice(&encoded);
        });

    // Interleave codewords spatially. Adjacent rendered bytes belong to
    // different RS words, so a local blur/occlusion is spread across words.
    let mut coded = vec![0_u8; CODED_BYTES];
    for shard_index in 0..RS_TOTAL {
        for block in 0..RS_BLOCKS {
            coded[shard_index * RS_BLOCKS + block] = codewords[block][shard_index];
        }
    }

    let mut symbols = bytes_to_symbols(&coded);
    debug_assert_eq!(symbols.len(), DATA_CELLS);
    for index in 0..CALIBRATION_CELLS {
        symbols.push((index % 64) as u8);
    }

    Ok(EncodedFrame {
        sequence,
        payload: payload.to_vec(),
        symbols,
    })
}

/// Decode six-bit cells. `None` means that the visual classifier deliberately
/// erased an ambiguous cell instead of guessing. Reed--Solomon can reconstruct
/// up to 51 erased bytes in every 255-byte codeword.
pub fn decode_frame(symbols: &[Option<u8>]) -> Result<DecodedFrame> {
    if symbols.len() < DATA_CELLS {
        bail!("need {} data cells, got {}", DATA_CELLS, symbols.len());
    }

    let erased_cells = symbols[..DATA_CELLS]
        .iter()
        .filter(|symbol| symbol.is_none())
        .count();
    let (coded, known) = symbols_to_bytes(&symbols[..DATA_CELLS]);
    let erased_bytes = known.iter().filter(|known| !**known).count();
    let mut framed = vec![0_u8; DATA_BYTES];
    let mut diagnostics = DecodeDiagnostics {
        erased_cells,
        erased_bytes,
        ..DecodeDiagnostics::default()
    };

    let recovered: Result<Vec<(Vec<u8>, usize, usize)>> = (0..RS_BLOCKS)
        .into_par_iter()
        .map(|block| {
            let mut codeword = vec![0_u8; RS_TOTAL];
            let mut erasures = Vec::new();
            for (shard_index, byte) in codeword.iter_mut().enumerate() {
                let position = shard_index * RS_BLOCKS + block;
                *byte = coded[position];
                if !known[position] {
                    erasures.push(shard_index as u8);
                }
            }
            let (corrected, errata) = Decoder::new(RS_PARITY)
                .correct_err_count(&codeword, Some(&erasures))
                .map_err(|_| {
                    anyhow!(
                        "RS codeword {block} cannot correct {} known erasures plus unknown errors",
                        erasures.len()
                    )
                })?;
            Ok((corrected.data().to_vec(), erasures.len(), errata))
        })
        .collect();
    for (block, (data, erasures, errata)) in recovered?.into_iter().enumerate() {
        diagnostics.max_codeword_erasures = diagnostics.max_codeword_erasures.max(erasures);
        diagnostics.reconstructed_codewords += usize::from(errata > 0);
        diagnostics.corrected_errata += errata;
        framed[block * RS_DATA..(block + 1) * RS_DATA].copy_from_slice(&data);
    }

    if framed[..8] != MAGIC {
        bail!("frame magic did not survive the visual channel");
    }
    let sequence = u32::from_le_bytes(framed[8..12].try_into()?);
    let payload_len = u32::from_le_bytes(framed[12..16].try_into()?) as usize;
    let expected_crc = u32::from_le_bytes(framed[16..20].try_into()?);
    if payload_len > PAYLOAD_BYTES {
        bail!("decoded payload length {payload_len} exceeds frame capacity");
    }
    let payload = framed[HEADER_BYTES..HEADER_BYTES + payload_len].to_vec();
    if crc32(&payload) != expected_crc {
        bail!("frame CRC failed; at least one confident cell was decoded incorrectly");
    }

    Ok(DecodedFrame {
        sequence,
        payload,
        diagnostics,
    })
}

fn bytes_to_symbols(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 8 / 6);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for &byte in bytes {
        accumulator |= (byte as u32) << bits;
        bits += 8;
        while bits >= 6 {
            out.push((accumulator & 0x3f) as u8);
            accumulator >>= 6;
            bits -= 6;
        }
    }
    debug_assert_eq!(bits, 0);
    out
}

fn symbols_to_bytes(symbols: &[Option<u8>]) -> (Vec<u8>, Vec<bool>) {
    let byte_count = symbols.len() * 6 / 8;
    let mut values = vec![0_u8; byte_count];
    let mut known_masks = vec![0_u8; byte_count];
    for (symbol_index, symbol) in symbols.iter().enumerate() {
        let bit_start = symbol_index * 6;
        for local_bit in 0..6 {
            let absolute = bit_start + local_bit;
            let byte_index = absolute / 8;
            let byte_bit = absolute % 8;
            if let Some(value) = symbol {
                values[byte_index] |= ((value >> local_bit) & 1) << byte_bit;
                known_masks[byte_index] |= 1 << byte_bit;
            }
        }
    }
    let known = known_masks.into_iter().map(|mask| mask == 0xff).collect();
    (values, known)
}

pub fn deterministic_payload(sequence: u32) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ sequence as u64;
    (0..PAYLOAD_BYTES)
        .map(|_| {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            (z ^ (z >> 31)) as u8
        })
        .collect()
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320_u32 & (0_u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_closes_exactly() {
        assert_eq!(DATA_CELLS, 41_820);
        assert_eq!(CODED_BYTES, 31_365);
        assert_eq!(RS_BLOCKS, 123);
        assert_eq!(DATA_BYTES, 25_092);
        assert_eq!(PAYLOAD_BYTES, 25_072);
    }

    #[test]
    fn round_trip_without_erasures() {
        let payload = deterministic_payload(17);
        let encoded = encode_frame(17, &payload).unwrap();
        let symbols: Vec<Option<u8>> = encoded.symbols.into_iter().map(Some).collect();
        let decoded = decode_frame(&symbols).unwrap();
        assert_eq!(decoded.sequence, 17);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn spatially_localized_erasures_are_interleaved() {
        let payload = deterministic_payload(23);
        let encoded = encode_frame(23, &payload).unwrap();
        let mut symbols: Vec<Option<u8>> = encoded.symbols.into_iter().map(Some).collect();
        // Erase a 16 x 16-cell square: 0.6% of the picture, but localized.
        for row in 80..96 {
            for col in 90..106 {
                symbols[row * GRID_COLS + col] = None;
            }
        }
        let decoded = decode_frame(&symbols).unwrap();
        assert_eq!(decoded.payload, payload);
        assert!(decoded.diagnostics.reconstructed_codewords > 0);
    }

    #[test]
    fn corrects_mixed_visual_errors_and_erasures() {
        let payload = deterministic_payload(31);
        let encoded = encode_frame(31, &payload).unwrap();
        let mut symbols: Vec<Option<u8>> = encoded.symbols.into_iter().map(Some).collect();
        // Sparse ambiguity and confident misclassification exercise both sides
        // of the 2*errors + erasures <= parity correction budget.
        for index in (17..DATA_CELLS).step_by(401) {
            symbols[index] = None;
        }
        for index in (101..DATA_CELLS).step_by(997) {
            if let Some(symbol) = &mut symbols[index] {
                *symbol ^= 0b00_0011;
            }
        }
        let decoded = decode_frame(&symbols).unwrap();
        assert_eq!(decoded.payload, payload);
        assert!(decoded.diagnostics.corrected_errata > 0);
    }
}
