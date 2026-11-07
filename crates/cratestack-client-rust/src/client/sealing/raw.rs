#[cfg(feature = "cose")]
use super::RouteRef;
use super::{CratestackClient, HttpClientCodec};

impl<C> CratestackClient<C>
where
    C: HttpClientCodec,
{
    /// This client as it must send a raw bridge request to `path`
    /// ([`execute_raw_transport`](Self::execute_raw_transport)): itself when
    /// it has no envelope, otherwise scoped to the request's route.
    ///
    /// A raw request has no generated code to name its route, so only RPC
    /// paths qualify: `/rpc/{op_id}` binds the op id, `/rpc/batch` binds
    /// `batch`. A raw REST path has no template to bind, and is refused
    /// rather than sealed for a route the server would not recognise.
    pub(crate) fn scoped_for_raw_path(
        &self,
        path: &str,
    ) -> Result<std::borrow::Cow<'_, Self>, crate::error::ClientError> {
        #[cfg(feature = "cose")]
        if self.sealing.envelope.is_some() {
            let op = crate::client::raw_path::rpc_op(path).ok_or_else(|| {
                crate::error::ClientError::BadInput(format!(
                    "a sealed raw request must be an RPC one (/rpc/{{op_id}} or /rpc/batch); \
                         '{path}' has no route template to bind"
                ))
            })?;
            return Ok(std::borrow::Cow::Owned(self.at(RouteRef::rpc(op))));
        }
        let _ = path;
        Ok(std::borrow::Cow::Borrowed(self))
    }
}
