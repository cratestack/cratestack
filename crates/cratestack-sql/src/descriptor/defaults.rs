#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateDefaultType {
    Bool,
    Int,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateDefault {
    pub column: &'static str,
    pub auth_field: &'static str,
    pub ty: CreateDefaultType,
    pub nullable: bool,
    /// Whether the auth field is required (non-optional) in the auth block.
    /// When true, a missing auth field should cause validation to fail,
    /// even if the model field is nullable. This prevents tenant-isolation
    /// issues where NULL values bypass policy predicates.
    pub auth_field_required: bool,
}
