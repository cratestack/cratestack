//! The template-list difference of `--preset fespalier` (cratestack#1204):
//! the Dio transport file is replaced by the fespalier one.
use crate::templates::TemplateSpec;

const DIO_TRANSPORT_OUTPUT: &str = "lib/src/dio_transport.dart";

const FESPALIER_TRANSPORT: TemplateSpec = TemplateSpec {
    template_name: "fespalier-transport.dart.j2",
    output_path: "lib/src/fespalier_transport.dart",
    default_source: include_str!("../templates/fespalier-transport.dart.j2"),
};

/// The fespalier preset has no Dio, so `dio_transport.dart` is skipped and
/// `fespalier_transport.dart` takes its place.
pub(crate) fn swap_dio_for_fespalier(specs: &mut Vec<TemplateSpec>) {
    specs.retain(|spec| spec.output_path != DIO_TRANSPORT_OUTPUT);
    specs.push(FESPALIER_TRANSPORT);
}
