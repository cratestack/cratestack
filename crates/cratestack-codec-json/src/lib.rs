use cratestack_core::{CoolCodec, CoolError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct JsonCodec;

impl CoolCodec for JsonCodec {
    const CONTENT_TYPE: &'static str = "application/json";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CoolError> {
        serde_json::to_vec(value)
            .map_err(|error| CoolError::Codec(format!("failed to encode JSON body: {error}")))
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CoolError> {
        serde_json::from_slice(bytes)
            .map_err(|error| CoolError::Codec(format!("failed to decode JSON body: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::CoolCodec;

    use super::JsonCodec;

    #[test]
    fn round_trips_value() {
        let codec = JsonCodec;
        let bytes = codec
            .encode(&vec!["cool", "stack"])
            .expect("encode should succeed");
        let value: Vec<String> = codec.decode(&bytes).expect("decode should succeed");

        assert_eq!(value, vec!["cool".to_owned(), "stack".to_owned()]);
    }
}
