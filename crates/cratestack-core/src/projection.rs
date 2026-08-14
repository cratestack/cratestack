use crate::{CratestackError, Page, SelectionQuery};
use serde_json::Value as JsonValue;

pub trait ProjectionDecoder {
    type Output;

    fn selection_query(&self) -> SelectionQuery;

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CratestackError>;

    fn decode_many(&self, value: JsonValue) -> Result<Vec<Self::Output>, CratestackError> {
        match value {
            JsonValue::Array(values) => values
                .into_iter()
                .map(|value| self.decode_one(value))
                .collect(),
            other => Err(CratestackError::Internal(format!(
                "projected list payload must be an array, got {other:?}"
            ))),
        }
    }

    fn decode_page(&self, value: JsonValue) -> Result<Page<Self::Output>, CratestackError> {
        let page = serde_json::from_value::<Page<JsonValue>>(value).map_err(|error| {
            CratestackError::Codec(format!("failed to decode projected page payload: {error}"))
        })?;
        let items = page
            .items
            .into_iter()
            .map(|value| self.decode_one(value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::new(items, page.page_info).with_total_count(page.total_count))
    }
}

impl ProjectionDecoder for SelectionQuery {
    type Output = JsonValue;

    fn selection_query(&self) -> SelectionQuery {
        self.clone()
    }

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CratestackError> {
        Ok(value)
    }
}
