use cratestack_core::{CratestackCodec, CratestackError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct JsonCodec;

impl CratestackCodec for JsonCodec {
    const CONTENT_TYPE: &'static str = "application/json";

    fn encode<T: Serialize + ?Sized>(&self, value: &T) -> Result<Vec<u8>, CratestackError> {
        serde_json::to_vec(value)
            .map_err(|error| CratestackError::Codec(format!("failed to encode JSON body: {error}")))
    }

    /// `serde_json` writes into any `io::Write`, so this appends to `out`
    /// directly instead of the default's encode-then-copy.
    fn encode_into<T: Serialize + ?Sized>(
        &self,
        value: &T,
        out: &mut Vec<u8>,
    ) -> Result<(), CratestackError> {
        let start = out.len();
        serde_json::to_writer(&mut *out, value).map_err(|error| {
            out.truncate(start);
            CratestackError::Codec(format!("failed to encode JSON body: {error}"))
        })
    }

    fn decode<T: for<'de> Deserialize<'de>>(&self, bytes: &[u8]) -> Result<T, CratestackError> {
        serde_json::from_slice(bytes)
            .map_err(|error| CratestackError::Codec(format!("failed to decode JSON body: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use cratestack_core::CratestackCodec;

    use super::JsonCodec;

    #[test]
    fn encode_into_appends_exactly_what_encode_returns() {
        let mut out = b"{prefix}".to_vec();
        let value = serde_json::json!({"a": [1, 2, null], "b": "c"});
        JsonCodec
            .encode_into(&value, &mut out)
            .expect("encode_into");
        assert_eq!(&out[..8], b"{prefix}");
        assert_eq!(
            &out[8..],
            JsonCodec.encode(&value).expect("encode").as_slice()
        );
    }

    #[test]
    fn encode_into_leaves_out_at_its_old_length_on_error() {
        // A map with a non-string key fails after `{` is written.
        let value = std::collections::BTreeMap::from([(vec![1_u8], 1_u8)]);
        let mut out = vec![1, 2, 3];
        assert!(JsonCodec.encode_into(&value, &mut out).is_err());
        assert_eq!(out, [1, 2, 3]);
    }

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
