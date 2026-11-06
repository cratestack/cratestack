use super::prepared::{MAX_PAYLOAD, PreparedCall, bad};
use super::route::encode_path_segment;
use reqwest::{
    Method,
    header::{HeaderName, HeaderValue},
};
use std::collections::HashSet;

impl PreparedCall {
    pub(super) fn validate(&self) -> Result<(), crate::ClientError> {
        if self.version != 1 {
            return Err(bad("unsupported prepared-call version"));
        }
        if self.body.len() > MAX_PAYLOAD
            || self.key_tables.len() > MAX_PAYLOAD
            || self.headers.len() > 64
            || self.params.len() > 64
        {
            return Err(bad("prepared-call size limit exceeded"));
        }
        let texts = [
            &self.base_url,
            &self.method,
            &self.path,
            &self.route,
            &self.op,
            &self.content_type,
            &self.plain_type,
            &self.accept,
        ];
        if texts.iter().any(|s| s.len() > 16 * 1024)
            || self.query.as_ref().is_some_and(|q| q.len() > 16 * 1024)
            || self.params.iter().any(|p| p.len() > 16 * 1024)
        {
            return Err(bad("prepared-call text limit exceeded"));
        }
        if !matches!(self.method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
            return Err(bad("prepared calls require a unary mutation method"));
        }
        Method::from_bytes(self.method.as_bytes()).map_err(|e| bad(e.to_string()))?;
        HeaderValue::from_str(&self.content_type).map_err(|e| bad(e.to_string()))?;
        HeaderValue::from_str(&self.accept).map_err(|e| bad(e.to_string()))?;
        match self.content_type.as_str() {
            "application/json" | "application/vnd.cratestack.compact+json" => {
                serde_json::from_slice::<serde_json::Value>(&self.body)
                    .map_err(|error| bad(format!("invalid prepared JSON: {error}")))?;
            }
            "application/cbor" | "application/vnd.cratestack.compact+cbor" => {
                let mut decoder = minicbor::Decoder::new(&self.body);
                decoder
                    .skip()
                    .map_err(|error| bad(format!("invalid prepared CBOR: {error}")))?;
                if decoder.position() != self.body.len() {
                    return Err(bad("trailing prepared CBOR"));
                }
            }
            _ => return Err(bad("invalid prepared content type")),
        }
        if !self.path.starts_with('/')
            || self.path.starts_with("//")
            || self.path.contains(['?', '#'])
        {
            return Err(bad(
                "prepared path must be an absolute service path without query/fragment",
            ));
        }
        let mut params = self.params.iter();
        let expected = if self.route.starts_with('/') {
            let parts = self
                .route
                .split('/')
                .map(|part| {
                    if part.starts_with('{') && part.ends_with('}') {
                        params
                            .next()
                            .map(|p| encode_path_segment(p))
                            .ok_or_else(|| bad("missing route parameter"))
                    } else {
                        Ok(part.to_owned())
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            parts.join("/")
        } else {
            format!("/rpc/{}", self.route)
        };
        if params.next().is_some() || self.path != expected {
            return Err(bad("prepared path does not match its route identity"));
        }
        let expected_op = if self.route.starts_with('/') {
            format!("{} {}", self.method, self.route)
        } else {
            self.route.clone()
        };
        if self.op != expected_op || self.op == "batch" {
            return Err(bad("prepared operation does not match its route"));
        }
        let mut names = HashSet::new();
        let mut has_key = false;
        for header in &self.headers {
            let name = header.name.to_ascii_lowercase();
            if !names.insert(name.clone()) || header.value.len() > 8192 {
                return Err(bad("duplicate or oversized prepared header"));
            }
            HeaderName::from_bytes(name.as_bytes()).map_err(|e| bad(e.to_string()))?;
            HeaderValue::from_str(&header.value).map_err(|e| bad(e.to_string()))?;
            // Persist only application headers whose semantics are part of the
            // mutation. Authentication and arbitrary custom headers are rebuilt
            // by the authorizer on every attempt.
            if !matches!(name.as_str(), "idempotency-key" | "if-match") {
                return Err(bad("header is not permitted in a prepared call"));
            }
            if name == "idempotency-key" {
                has_key = !header.value.trim().is_empty();
            }
        }
        if !has_key {
            return Err(bad("a prepared mutation requires an Idempotency-Key"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeHeader;

    fn call() -> PreparedCall {
        PreparedCall {
            version: 1,
            base_url: "https://example.test/".into(),
            method: "POST".into(),
            path: "/widgets/7".into(),
            route: "/widgets/{id}".into(),
            params: vec!["7".into()],
            query: None,
            headers: vec![RuntimeHeader {
                name: "Idempotency-Key".into(),
                value: "key".into(),
            }],
            body: vec![1],
            content_type: "application/cbor".into(),
            plain_type: "application/cbor".into(),
            accept: "application/cbor".into(),
            op: "POST /widgets/{id}".into(),
            contract: [0; 32],
            key_tables: vec![],
            sealed: false,
        }
    }

    #[test]
    fn refuses_credential_injection_and_duplicate_keys_on_restore() {
        let mut value = call();
        assert!(value.validate().is_ok());
        value.headers.push(RuntimeHeader {
            name: "x-api-key".into(),
            value: "secret".into(),
        });
        assert!(value.validate().is_err());
        value.headers[1].name = "idempotency-key".into();
        assert!(value.validate().is_err());
    }

    #[test]
    fn refuses_route_changes_and_unknown_fields() {
        let mut value = call();
        value.path = "/widgets/other/7".into();
        assert!(value.validate().is_err());
        let mut json = serde_json::to_value(call()).unwrap();
        json["extra"] = serde_json::json!(true);
        assert!(PreparedCall::from_bytes(&serde_json::to_vec(&json).unwrap()).is_err());
    }
}
