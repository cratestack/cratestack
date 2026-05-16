use cratestack_core::{CoolError, Page, SelectionQuery};
use serde_json::Value as JsonValue;

pub trait Projection {
    type Output;

    fn selection_query(&self) -> SelectionQuery;

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CoolError>;

    fn decode_many(&self, value: JsonValue) -> Result<Vec<Self::Output>, CoolError> {
        match value {
            JsonValue::Array(values) => values
                .into_iter()
                .map(|value| self.decode_one(value))
                .collect(),
            other => Err(CoolError::Internal(format!(
                "projected list payload must be an array, got {other:?}"
            ))),
        }
    }

    fn decode_page(&self, value: JsonValue) -> Result<Page<Self::Output>, CoolError> {
        let page = serde_json::from_value::<Page<JsonValue>>(value).map_err(|error| {
            CoolError::Codec(format!("failed to decode projected page payload: {error}"))
        })?;
        let items = page
            .items
            .into_iter()
            .map(|value| self.decode_one(value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Page::new(items, page.page_info).with_total_count(page.total_count))
    }
}

impl Projection for SelectionQuery {
    type Output = JsonValue;

    fn selection_query(&self) -> SelectionQuery {
        self.clone()
    }

    fn decode_one(&self, value: JsonValue) -> Result<Self::Output, CoolError> {
        Ok(value)
    }
}
