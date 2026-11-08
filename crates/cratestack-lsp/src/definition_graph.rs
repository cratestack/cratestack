//! Go-to-definition across the files of a split schema (design section 12).

use tower_lsp_server::ls_types::{Location, Position, Uri};

use crate::definition::definition_span;
use crate::text::range_from_offsets;
use crate::view::Resolved;

/// The declaration of the name at `position`, in whichever file declares it,
/// with a range computed against that file's own text.
pub(crate) fn definition_in(
    uri: &Uri,
    view: &Resolved<'_>,
    position: Position,
) -> Option<Location> {
    let offset = view.global_offset(position)?;
    let span = definition_span(view.global, view.schema, offset)?;
    let (target, text, local) = view.locate(span, uri)?;
    Some(Location {
        uri: target,
        range: range_from_offsets(text, local.start, local.end),
    })
}
