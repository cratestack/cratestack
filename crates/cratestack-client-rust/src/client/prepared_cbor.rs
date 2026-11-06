//! Validate one complete CBOR item without decoding it into application values.
use minicbor::{Decoder, data::Type};

use super::prepared::bad;
use crate::ClientError;

const MAX_DEPTH: usize = 256;

pub(super) fn validate(bytes: &[u8]) -> Result<(), ClientError> {
    let mut decoder = Decoder::new(bytes);
    item(&mut decoder, 0)?;
    if decoder.position() != bytes.len() {
        return Err(bad("trailing prepared CBOR"));
    }
    Ok(())
}

fn item(decoder: &mut Decoder<'_>, depth: usize) -> Result<(), ClientError> {
    if depth > MAX_DEPTH {
        return Err(bad("prepared CBOR nesting limit exceeded"));
    }
    let kind = decoder.datatype().map_err(invalid)?;
    match kind {
        Type::Array | Type::ArrayIndef => {
            let len = decoder.array().map_err(invalid)?;
            sequence(decoder, len, false, depth)
        }
        Type::Map | Type::MapIndef => {
            let len = decoder.map().map_err(invalid)?;
            sequence(decoder, len, true, depth)
        }
        Type::Bytes | Type::BytesIndef => {
            for chunk in decoder.bytes_iter().map_err(invalid)? {
                chunk.map_err(invalid)?;
            }
            Ok(())
        }
        Type::String | Type::StringIndef => {
            for chunk in decoder.str_iter().map_err(invalid)? {
                chunk.map_err(invalid)?;
            }
            Ok(())
        }
        Type::Tag => {
            decoder.tag().map_err(invalid)?;
            item(decoder, depth + 1)
        }
        Type::Break => Err(bad("unexpected CBOR break")),
        _ => {
            let position = decoder.position();
            let input = decoder.input();
            if input[position] == 0xf8 && input.get(position + 1).is_some_and(|n| *n < 32) {
                return Err(bad("invalid CBOR simple value"));
            }
            decoder.skip().map_err(invalid)
        }
    }
}

fn sequence(
    decoder: &mut Decoder<'_>,
    len: Option<u64>,
    map: bool,
    depth: usize,
) -> Result<(), ClientError> {
    let mut seen = 0_u64;
    loop {
        if len.is_some_and(|n| seen == n) {
            return Ok(());
        }
        if len.is_none() && decoder.datatype().map_err(invalid)? == Type::Break {
            decoder.set_position(decoder.position() + 1);
            return Ok(());
        }
        item(decoder, depth + 1)?;
        if map {
            item(decoder, depth + 1)?;
        }
        seen += 1;
    }
}

fn invalid(error: minicbor::decode::Error) -> ClientError {
    bad(format!("invalid prepared CBOR: {error}"))
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn validates_single_complete_well_formed_items() {
        let bad: &[&[u8]] = &[
            &[],
            &[0, 0],
            &[0x18],
            &[0x1c],
            &[0xff],
            &[0x81, 0xff],
            &[0xbf, 0, 0xff],
            &[0x5f, 0x61, 0x61, 0xff],
            &[0x5f, 0x5f, 0xff, 0xff],
            &[0xf8, 0],
            &[0xf8, 24],
            &[0xf8, 31],
            &[0x61, 0xff],
            &[0x62, 0xc0, 0x80],
            &[0x63, 0xed, 0xa0, 0x80],
            &[0x64, 0xf4, 0x90, 0x80, 0x80],
            &[0x7f, 0x61, 0xc2, 0x61, 0xa2, 0xff],
        ];
        for bytes in bad {
            assert!(validate(bytes).is_err(), "accepted {bytes:02x?}");
        }
        let good: &[&[u8]] = &[
            &[0xa1, 1, 2],
            &[0xa1, 0x81, 1, 2],
            &[0x1b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            &[0x3b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
            &[0xdb, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0],
            &[0x9f, 0x81, 1, 0xbf, 0, 1, 0xff, 0xff],
            &[0x5f, 0x41, 0, 0x42, 1, 2, 0xff],
            &[0x7f, 0x62, 0xc2, 0xa2, 0x61, 0x61, 0xff],
            &[0xf9, 0x7e, 0],
            &[0xfa, 0x7f, 0x80, 0, 0],
            &[0xf8, 32],
            &[0x18, 0],
        ];
        for bytes in good {
            assert!(validate(bytes).is_ok(), "rejected {bytes:02x?}");
        }
        let mut nested = vec![0x81; 256];
        nested.push(0);
        assert!(validate(&nested).is_ok());
        nested.insert(0, 0x81);
        assert!(validate(&nested).is_err());
    }
}
