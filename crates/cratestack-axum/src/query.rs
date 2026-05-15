//! Query-string parsing for axum-bound handlers: percent-decoded pair
//! extraction and the structured filter expression grammar
//! (`?where=...`) used by macro-generated `list` endpoints.

use cratestack_core::CoolError;
use url::form_urlencoded;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryExpr {
    Predicate { key: String, value: String },
    All(Vec<QueryExpr>),
    Any(Vec<QueryExpr>),
    Not(Box<QueryExpr>),
}

pub fn parse_query_pairs(raw_query: Option<&str>) -> Result<Vec<(String, String)>, CoolError> {
    let Some(raw_query) = raw_query else {
        return Ok(Vec::new());
    };

    let mut pairs = Vec::new();
    for (key, value) in form_urlencoded::parse(raw_query.as_bytes()) {
        pairs.push((key.into_owned(), value.into_owned()));
    }
    Ok(pairs)
}

pub fn parse_filter_expression(input: &str) -> Result<QueryExpr, CoolError> {
    let mut parser = FilterExpressionParser::new(input);
    let expr = parser.parse_expr()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(CoolError::BadRequest(format!(
            "unexpected trailing filter expression content near '{}'",
            parser.remaining(),
        )));
    }
    Ok(expr)
}

pub(crate) struct FilterExpressionParser<'a> {
    input: &'a str,
    cursor: usize,
}

impl<'a> FilterExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, cursor: 0 }
    }

    fn parse_expr(&mut self) -> Result<QueryExpr, CoolError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<QueryExpr, CoolError> {
        let mut nodes = vec![self.parse_and()?];
        loop {
            self.skip_whitespace();
            if !self.consume('|') {
                break;
            }
            nodes.push(self.parse_and()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("single node should exist")
        } else {
            QueryExpr::Any(nodes)
        })
    }

    fn parse_and(&mut self) -> Result<QueryExpr, CoolError> {
        let mut nodes = vec![self.parse_factor()?];
        loop {
            self.skip_whitespace();
            if !self.consume(',') {
                break;
            }
            nodes.push(self.parse_factor()?);
        }
        Ok(if nodes.len() == 1 {
            nodes.pop().expect("single node should exist")
        } else {
            QueryExpr::All(nodes)
        })
    }

    fn parse_factor(&mut self) -> Result<QueryExpr, CoolError> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            self.skip_whitespace();
            if !self.consume('(') {
                return Err(CoolError::BadRequest(
                    "negated filter expression must use not(...)".to_owned(),
                ));
            }
            let expr = self.parse_expr()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(CoolError::BadRequest(
                    "unterminated negated filter expression".to_owned(),
                ));
            }
            return Ok(QueryExpr::Not(Box::new(expr)));
        }
        if self.consume('(') {
            let expr = self.parse_expr()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(CoolError::BadRequest(
                    "unterminated grouped filter expression".to_owned(),
                ));
            }
            return Ok(expr);
        }

        self.parse_predicate()
    }

    fn parse_predicate(&mut self) -> Result<QueryExpr, CoolError> {
        let start = self.cursor;
        while let Some(ch) = self.peek() {
            if matches!(ch, ',' | '|' | ')') {
                break;
            }
            self.cursor += ch.len_utf8();
        }
        let raw = self.input[start..self.cursor].trim();
        let (key, value) = raw.split_once('=').ok_or_else(|| {
            CoolError::BadRequest(format!(
                "invalid grouped filter '{}': expected key=value",
                raw,
            ))
        })?;
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(CoolError::BadRequest(format!(
                "invalid grouped filter '{}': expected non-empty key and value",
                raw,
            )));
        }
        Ok(QueryExpr::Predicate {
            key: key.trim().to_owned(),
            value: value.trim().to_owned(),
        })
    }

    fn consume(&mut self, expected: char) -> bool {
        match self.peek() {
            Some(ch) if ch == expected => {
                self.cursor += ch.len_utf8();
                true
            }
            _ => false,
        }
    }

    fn consume_keyword(&mut self, expected: &str) -> bool {
        let remaining = &self.input[self.cursor..];
        if !remaining.starts_with(expected) {
            return false;
        }
        let boundary = remaining[expected.len()..].chars().next();
        if boundary.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            return false;
        }
        self.cursor += expected.len();
        true
    }

    fn peek(&self) -> Option<char> {
        self.input[self.cursor..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if !ch.is_whitespace() {
                break;
            }
            self.cursor += ch.len_utf8();
        }
    }

    fn remaining(&self) -> &str {
        &self.input[self.cursor..]
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.input.len()
    }
}
