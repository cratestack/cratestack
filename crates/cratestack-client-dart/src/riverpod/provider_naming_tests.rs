use super::*;

#[test]
fn first_call_gets_the_preferred_name() {
    let mut occupied = BTreeSet::new();
    let name = reserve_operation_symbol("widget", false, "tinyRpcClient", &mut occupied);
    assert_eq!(name, "widget");
    assert!(occupied.contains("widget"));
    assert!(occupied.contains("widgetProvider"));
}

#[test]
fn colliding_function_names_escalate_to_the_qualified_prefixed_form() {
    let mut occupied = BTreeSet::new();
    occupied.insert("widgetList".to_owned());

    let name = reserve_operation_symbol("widgetList", false, "tinyRpcClient", &mut occupied);
    assert_eq!(name, "tinyRpcClientWidgetList");
}

#[test]
fn colliding_provider_variable_name_also_escalates_even_when_the_base_is_free() {
    // A class's provider name lower-cases only its first character, so a
    // free-looking base can still collide with another symbol's already
    // reserved `...Provider` variable.
    let mut occupied = BTreeSet::new();
    occupied.insert("widgetCreateControllerProvider".to_owned());

    let name = reserve_operation_symbol(
        "WidgetCreateController",
        true,
        "tinyRpcClient",
        &mut occupied,
    );
    assert_ne!(name, "WidgetCreateController");
    assert!(occupied.contains(&format!("{}Provider", crate::idents::to_camel_case(&name))));
}

#[test]
fn repeated_collisions_fall_back_to_a_numeric_suffix() {
    let mut occupied = BTreeSet::new();
    occupied.insert("widget".to_owned());
    occupied.insert("tinyRpcClientWidget".to_owned());

    let name = reserve_operation_symbol("widget", false, "tinyRpcClient", &mut occupied);
    assert_eq!(name, "tinyRpcClientWidget2");
}

#[test]
fn two_reservations_for_the_same_preferred_name_never_collide_with_each_other() {
    let mut occupied = BTreeSet::new();
    let first = reserve_operation_symbol("widgetList", false, "tinyRpcClient", &mut occupied);
    let second = reserve_operation_symbol("widgetList", false, "tinyRpcClient", &mut occupied);
    assert_ne!(first, second);
}

#[test]
fn seed_includes_the_existing_di_layer_and_model_type_names() {
    let schema = cratestack_parser::parse_schema(
        "model Widget {\n  id Int @id\n  name String\n}\n\nprocedure echoName(name: String): String\n",
    )
    .expect("fixture schema should parse");

    let occupied = seed_occupied_symbols(&schema, "tinyRpcClient", false);
    assert!(occupied.contains("Widget"));
    assert!(occupied.contains("CreateWidgetInput"));
    assert!(occupied.contains("UpdateWidgetInput"));
    assert!(occupied.contains("WidgetApi"));
    assert!(occupied.contains("ProjectedWidget"));
    assert!(occupied.contains("WidgetSelection"));
    assert!(occupied.contains("WidgetIncludeSelection"));
    assert!(occupied.contains("tinyRpcClientWidgetApiProvider"));
    assert!(occupied.contains("tinyRpcClientAdapterProvider"));
    assert!(occupied.contains("tinyRpcClientClientProvider"));
    assert!(occupied.contains("tinyRpcClientProceduresApiProvider"));
    assert!(occupied.contains("ProceduresApi"));
    assert!(occupied.contains("EchoNameArgs"));
    assert!(
        !occupied.contains("tinyRpcClientBasePathProvider"),
        "RPC schemas have no base-path provider"
    );
}

#[test]
fn seed_includes_the_rest_base_path_provider_only_for_rest() {
    let schema = cratestack_parser::parse_schema("model Widget {\n  id Int @id\n}\n")
        .expect("fixture schema should parse");

    let occupied = seed_occupied_symbols(&schema, "tinyRestClient", true);
    assert!(occupied.contains("tinyRestClientBasePathProvider"));
}
