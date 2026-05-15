//! Structured principal context — actor / session / tenant facets
//! plus an arbitrary claims bag. Resolves dotted path lookups
//! (`actor.id`, `tenant.slug`) so audit + policy code never reaches
//! into the raw claims map directly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;
use crate::value::Value;

use super::identity::CoolAuthIdentity;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrincipalFacet {
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrincipalContext {
    pub actor: Option<PrincipalFacet>,
    pub session: Option<PrincipalFacet>,
    pub tenant: Option<PrincipalFacet>,
    pub claims: BTreeMap<String, Value>,
}

impl PrincipalContext {
    pub fn from_principal<P: Serialize>(principal: P) -> Result<Self, CoolError> {
        let auth = CoolAuthIdentity::from_principal(principal)?;
        Ok(Self::from_auth_identity(&auth))
    }

    pub fn from_claims(claims: BTreeMap<String, Value>) -> Self {
        Self {
            actor: None,
            session: None,
            tenant: None,
            claims,
        }
    }

    pub fn from_auth_identity(auth: &CoolAuthIdentity) -> Self {
        let mut claims = auth.fields.clone();
        let actor = take_principal_facet(&mut claims, "actor");
        let session = take_principal_facet(&mut claims, "session");
        let tenant = take_principal_facet(&mut claims, "tenant");
        Self {
            actor,
            session,
            tenant,
            claims,
        }
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        if let Some(value) = self
            .claims
            .get(name)
            .or_else(|| lookup_value_path_in_map(&self.claims, name))
        {
            return Some(value);
        }

        let (root, rest) = name.split_once('.')?;
        match root {
            "actor" => lookup_principal_facet_path(self.actor.as_ref(), rest),
            "session" => lookup_principal_facet_path(self.session.as_ref(), rest),
            "tenant" => lookup_principal_facet_path(self.tenant.as_ref(), rest),
            _ => None,
        }
    }

    pub fn as_auth_identity(&self) -> CoolAuthIdentity {
        CoolAuthIdentity {
            fields: self.legacy_fields(),
        }
    }

    pub fn legacy_fields(&self) -> BTreeMap<String, Value> {
        let mut fields = self.claims.clone();
        if let Some(actor) = &self.actor {
            fields.insert("actor".to_owned(), Value::Map(actor.fields.clone()));
        }
        if let Some(session) = &self.session {
            fields.insert("session".to_owned(), Value::Map(session.fields.clone()));
        }
        if let Some(tenant) = &self.tenant {
            fields.insert("tenant".to_owned(), Value::Map(tenant.fields.clone()));
        }
        fields
    }
}

pub(super) fn lookup_value_path_in_map<'a>(
    map: &'a BTreeMap<String, Value>,
    path: &str,
) -> Option<&'a Value> {
    let mut segments = path.split('.');
    let first = segments.next()?;
    let mut current = map.get(first)?;
    for segment in segments {
        current = match current {
            Value::Map(entries) => entries.get(segment)?,
            _ => return None,
        };
    }
    Some(current)
}

fn lookup_principal_facet_path<'a>(
    facet: Option<&'a PrincipalFacet>,
    path: &str,
) -> Option<&'a Value> {
    let facet = facet?;
    facet
        .fields
        .get(path)
        .or_else(|| lookup_value_path_in_map(&facet.fields, path))
}

fn take_principal_facet(claims: &mut BTreeMap<String, Value>, key: &str) -> Option<PrincipalFacet> {
    match claims.remove(key) {
        Some(Value::Map(fields)) => Some(PrincipalFacet { fields }),
        Some(value) => {
            claims.insert(key.to_owned(), value);
            None
        }
        None => None,
    }
}
