//! The subscription-procedure attribute `@frames(unsigned)` (cratestack#1378).
//!
//! It marks a `subscription procedure` whose frames are not signed: under a
//! required signed envelope, the open request is sealed and verified like a
//! unary call, and the stream that follows carries plain frames, which a
//! client may treat only as hints (ADR 0006, §6). Without it a subscription
//! keeps its current behaviour under a required envelope.
//!
//! `sealed` (signed frames, #1020) and `chain` (chained frames, #1008) are
//! reserved names, refused here until those exist. One reader for every
//! consumer: `cratestack-parser` refuses an attribute this does not read, the
//! contract digest keeps the attribute's text, and the macros read it
//! through [`Procedure::frames`].

use serde::{Deserialize, Serialize};

use super::{Attribute, Procedure};

/// The attribute's name.
pub const FRAMES_ATTRIBUTE: &str = "@frames";

/// The response header a server sets on the unsigned frames that answer a
/// signed open (`Cratestack-Frames: unsigned`, ADR 0006, cratestack#1378).
/// It is **not authenticated**: a client reads it as a label, never as proof,
/// and refuses a stream that lacks it.
pub const FRAMES_HEADER: &str = "Cratestack-Frames";

/// How the frames of a subscription are authenticated, when the schema says.
/// Absent (`None` from [`Procedure::frames`]) means the subscription takes
/// no part in a signed open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FramesMode {
    /// The open is signed; the frames are not. Integrity of a frame is not
    /// protected: a client must treat it as a hint and re-read through a
    /// sealed call.
    Unsigned,
}

impl FramesMode {
    /// The word written inside `@frames(..)`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unsigned => "unsigned",
        }
    }
}

/// Whether `attribute` is an `@frames` (whatever its argument).
pub fn is_frames_attribute(attribute: &Attribute) -> bool {
    attribute
        .raw
        .trim()
        .strip_prefix(FRAMES_ATTRIBUTE)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

/// Reads the argument of `@frames(..)`: `unsigned`. `sealed` and `chain` are
/// reserved and refused with the issue that will define them.
pub fn parse_frames_argument(argument: &str) -> Result<FramesMode, String> {
    match argument.trim() {
        "unsigned" => Ok(FramesMode::Unsigned),
        "sealed" => Err(
            "`@frames(sealed)` is reserved for signed frames (cratestack#1020) and is not \
             supported yet; `@frames(unsigned)` is the only mode"
                .to_owned(),
        ),
        "chain" => Err(
            "`@frames(chain)` is reserved for chained frames (cratestack#1008) and is not \
             supported yet; `@frames(unsigned)` is the only mode"
                .to_owned(),
        ),
        other => Err(format!(
            "`@frames({other})` is not a frames mode: write `@frames(unsigned)`"
        )),
    }
}

/// Reads a whole `@frames(..)` attribute.
pub fn parse_frames_attribute(raw: &str) -> Result<FramesMode, String> {
    let inner = raw
        .trim()
        .strip_prefix(FRAMES_ATTRIBUTE)
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| "`@frames` takes its mode in parentheses: `@frames(unsigned)`".to_owned())?;
    parse_frames_argument(inner)
}

impl Procedure {
    /// The frames mode the procedure declares: `Some` when it carries a
    /// readable `@frames(..)`, `None` otherwise. The parser refuses any other
    /// spelling and a repeat, so a validated schema has at most one.
    pub fn frames(&self) -> Option<FramesMode> {
        self.attributes
            .iter()
            .find(|attribute| is_frames_attribute(attribute))
            .and_then(|attribute| parse_frames_attribute(&attribute.raw).ok())
    }
}

/// What the generated `SUBSCRIPTION_FRAMES` table holds: per op key, the
/// frames mode of each subscription the schema marked. Keyed like
/// [`crate::OpContracts`]. An op that is not listed is not marked, and a
/// consumer that finds no row must fail closed.
///
/// **Lookup rule.** The key is the route's `contract_key()`, else its `op()`.
/// On RPC the rows are bare op ids (`procedure.<name>`), and
/// [`crate::find_contract`] matches a bare key for any method, so a lookup
/// must not use it: it matches the bare key only when the resolved route is a
/// subscription (`is_subscription()`), so a unary call to the same op id is
/// not an open. On REST the row is method-qualified (`GET /$procs/<name>`)
/// and `is_subscription()` is always false, so a lookup matches
/// `"<METHOD> <key>"` exactly; `HEAD` never matches.
pub type SubscriptionFrames = &'static [(&'static str, FramesMode)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frames_header_name_is_fixed() {
        assert_eq!(FRAMES_HEADER, "Cratestack-Frames");
    }

    #[test]
    fn frames_mode_serialises_as_its_schema_spelling() {
        assert_eq!(
            serde_json::to_string(&FramesMode::Unsigned).unwrap(),
            "\"unsigned\""
        );
        assert_eq!(
            serde_json::from_str::<FramesMode>("\"unsigned\"").unwrap(),
            FramesMode::Unsigned
        );
    }

    #[test]
    fn unsigned_reads_and_the_rest_is_refused() {
        assert_eq!(
            parse_frames_attribute("@frames(unsigned)"),
            Ok(FramesMode::Unsigned)
        );
        assert_eq!(
            parse_frames_attribute("@frames( unsigned )"),
            Ok(FramesMode::Unsigned)
        );
        assert!(
            parse_frames_attribute("@frames(sealed)")
                .unwrap_err()
                .contains("#1020")
        );
        assert!(
            parse_frames_attribute("@frames(chain)")
                .unwrap_err()
                .contains("#1008")
        );
        assert!(
            parse_frames_attribute("@frames(x)")
                .unwrap_err()
                .contains("not a frames mode")
        );
        assert!(parse_frames_attribute("@frames").is_err());
        assert_eq!(FramesMode::Unsigned.as_str(), "unsigned");
    }
}
