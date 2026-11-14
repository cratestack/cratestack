//! The completion entry for `@device_auth` (cratestack#1303), with its roles
//! in the detail: they are what the parser refuses anything else than.

use cratestack_core::schema::{DEVICE_AUTH_ATTRIBUTE, DeviceAuthRole};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

pub(crate) fn detail() -> String {
    let roles = DeviceAuthRole::ALL.map(|role| format!("`{}`", role.name()));
    format!(
        "procedure attribute: one leg of a device's enrolment, renewal or revocation — \
         `@device_auth(role: \"beginEnrolment\")`; the roles are {}. The five roles but \
         `sendEnrolmentCode` are required together; `generate-dart --preset fespalier` emits a \
         `DeviceAuthServer` over them. Every leg but `revoke` must be public (allow true, with no deny, authn or \
         authorize); `revoke`, signed by the device, must not be",
        roles.join(", ")
    )
}

pub(crate) fn completion_items() -> impl Iterator<Item = CompletionItem> {
    std::iter::once(CompletionItem {
        label: DEVICE_AUTH_ATTRIBUTE.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(detail()),
        ..CompletionItem::default()
    })
}
