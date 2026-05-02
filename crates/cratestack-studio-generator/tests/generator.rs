use std::fs;
use std::path::PathBuf;

use cratestack_studio_generator::{
    StudioGeneratorConfig, StudioGeneratorContext, StudioProfile, generate_package,
};

#[test]
fn generates_inventory_service_studio_scaffold() {
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inventory.cstack");
    let schema =
        cratestack_parser::parse_schema_file(&schema_path).expect("inventory schema should parse");

    let package = generate_package(
        &[StudioGeneratorContext {
            key: "inventory".to_owned(),
            display_name: "inventory-service".to_owned(),
            service_name: "inventory-service".to_owned(),
            schema_path: schema_path.clone(),
            service_url: "http://127.0.0.1:8082".to_owned(),
            schema: &schema,
        }],
        &StudioGeneratorConfig {
            name: "inventory-service-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: None,
        },
    )
    .expect("inventory studio package should generate");

    assert!(
        package
            .files
            .iter()
            .any(|file| file.file_name == "backend/src/main.rs")
    );
    assert!(
        package
            .files
            .iter()
            .any(|file| file.file_name == "web/src/app.rs")
    );
    assert!(
        package
            .files
            .iter()
            .any(|file| file.file_name == "shared/src/lib.rs")
    );

    let shared = package_file(&package, "shared/src/lib.rs");
    let metadata_json = package_file(&package, "shared/src/metadata.json");
    let readme = package_file(&package, "README.md");
    let web_app = package_file(&package, "web/src/app.rs");

    assert!(shared.contains("pub struct StudioMetadata"));
    assert!(shared.contains("pub struct StudioContextMetadata"));
    assert!(shared.contains("include_str!(\"metadata.json\")"));
    assert!(metadata_json.contains("\"default_context\": \"inventory\""));
    assert!(metadata_json.contains("\"key\": \"inventory\""));
    assert!(metadata_json.contains("\"name\": \"Inventory\""));
    assert!(metadata_json.contains("\"name\": \"InventoryMembership\""));
    assert!(metadata_json.contains("\"name\": \"queryInventory\""));
    assert!(readme.contains("inventory-service"));
    assert!(readme.contains("http://127.0.0.1:8082"));
    assert!(web_app.contains("ModelListPage"));
}

#[test]
fn generates_enum_metadata_for_schema_enums() {
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/studio.cstack");
    let schema =
        cratestack_parser::parse_schema_file(&schema_path).expect("fixture schema should parse");

    let package = generate_package(
        &[StudioGeneratorContext {
            key: "role".to_owned(),
            display_name: "role-service".to_owned(),
            service_name: "role-service".to_owned(),
            schema_path: schema_path.clone(),
            service_url: "http://127.0.0.1:8080".to_owned(),
            schema: &schema,
        }],
        &StudioGeneratorConfig {
            name: "role-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: None,
        },
    )
    .expect("enum studio package should generate");

    let shared = package_file(&package, "shared/src/lib.rs");
    let metadata_json = package_file(&package, "shared/src/metadata.json");

    assert!(shared.contains("pub struct StudioEnum"));
    assert!(shared.contains("include_str!(\"metadata.json\")"));
    assert!(metadata_json.contains("\"name\": \"Role\""));
    assert!(metadata_json.contains("\"admin\""));
    assert!(metadata_json.contains("\"member\""));
}

#[test]
fn prefers_template_override_directory_when_provided() {
    let schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/studio.cstack");
    let schema =
        cratestack_parser::parse_schema_file(&schema_path).expect("fixture schema should parse");
    let temp_dir = tempfile::tempdir().expect("temp dir should create");
    let template_path = temp_dir.path().join("root/README.md.j2");
    fs::create_dir_all(
        template_path
            .parent()
            .expect("override template parent should exist"),
    )
    .expect("override dir should be created");
    fs::write(&template_path, "# override {{ app.name }}").expect("override template should write");

    let package = generate_package(
        &[StudioGeneratorContext {
            key: "role".to_owned(),
            display_name: "role-service".to_owned(),
            service_name: "role-service".to_owned(),
            schema_path: schema_path.clone(),
            service_url: "http://127.0.0.1:8080".to_owned(),
            schema: &schema,
        }],
        &StudioGeneratorConfig {
            name: "role-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: Some(temp_dir.path().to_path_buf()),
        },
    )
    .expect("override template should render");

    assert_eq!(
        package_file(&package, "README.md"),
        "# override role-studio"
    );
}

#[test]
fn generates_multi_context_metadata_and_readme() {
    let inventory_schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inventory.cstack");
    let accounts_schema_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/accounts.cstack");
    let inventory_schema = cratestack_parser::parse_schema_file(&inventory_schema_path)
        .expect("inventory schema should parse");
    let accounts_schema = cratestack_parser::parse_schema_file(&accounts_schema_path)
        .expect("accounts schema should parse");

    let package = generate_package(
        &[
            StudioGeneratorContext {
                key: "inventory".to_owned(),
                display_name: "inventory-service".to_owned(),
                service_name: "inventory-service".to_owned(),
                schema_path: inventory_schema_path.clone(),
                service_url: "http://127.0.0.1:8082".to_owned(),
                schema: &inventory_schema,
            },
            StudioGeneratorContext {
                key: "accounts".to_owned(),
                display_name: "accounts-service".to_owned(),
                service_name: "accounts-service".to_owned(),
                schema_path: accounts_schema_path.clone(),
                service_url: "http://127.0.0.1:8081".to_owned(),
                schema: &accounts_schema,
            },
        ],
        &StudioGeneratorConfig {
            name: "sample-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: None,
        },
    )
    .expect("multi-context studio package should generate");

    let metadata_json = package_file(&package, "shared/src/metadata.json");
    let readme = package_file(&package, "README.md");

    assert!(metadata_json.contains("\"contexts\":"));
    assert!(metadata_json.contains("\"key\": \"inventory\""));
    assert!(metadata_json.contains("\"key\": \"accounts\""));
    assert!(readme.contains("inventory-service"));
    assert!(readme.contains("accounts-service"));
}

fn package_file<'a>(
    package: &'a cratestack_studio_generator::GeneratedStudioPackage,
    path: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == path)
        .unwrap_or_else(|| panic!("missing generated file {path}"))
        .contents
        .as_str()
}
