use cratestack_core::{CoolCodec, CoolError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct CborCodec;

impl CoolCodec for CborCodec {
    const CONTENT_TYPE: &'static str = "application/cbor";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CoolError> {
        minicbor_serde::to_vec(value)
            .map_err(|error| CoolError::Codec(format!("failed to encode CBOR body: {error}")))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CoolError> {
        minicbor_serde::from_slice(bytes)
            .map_err(|error| CoolError::Codec(format!("failed to decode CBOR body: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::CoolCodec;

    use super::CborCodec;

    #[test]
    fn round_trips_value() {
        let codec = CborCodec;
        let bytes = codec
            .encode(&vec!["cool", "stack"])
            .expect("encode should succeed");
        let value: Vec<String> = codec.decode(&bytes).expect("decode should succeed");

        assert_eq!(value, vec!["cool".to_owned(), "stack".to_owned()]);
    }
}
