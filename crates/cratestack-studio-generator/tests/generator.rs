use std::fs;
use std::path::PathBuf;

use cratestack_studio_generator::{
    StudioGeneratorConfig, StudioGeneratorContext, StudioProfile, generate_package,
};

#[test]
fn generates_vendor_service_studio_scaffold() {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../vaam-backends/services/vendor-service/schema/vendor.cstack");
    let schema =
        cratestack_parser::parse_schema_file(&schema_path).expect("vendor schema should parse");

    let package = generate_package(
        &[StudioGeneratorContext {
            key: "vendor".to_owned(),
            display_name: "vendor-service".to_owned(),
            service_name: "vendor-service".to_owned(),
            schema_path: schema_path.clone(),
            service_url: "http://127.0.0.1:8082".to_owned(),
            schema: &schema,
        }],
        &StudioGeneratorConfig {
            name: "vendor-service-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: None,
        },
    )
    .expect("vendor studio package should generate");

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
    assert!(metadata_json.contains("\"default_context\": \"vendor\""));
    assert!(metadata_json.contains("\"key\": \"vendor\""));
    assert!(metadata_json.contains("\"name\": \"Vendor\""));
    assert!(metadata_json.contains("\"name\": \"VendorMembership\""));
    assert!(metadata_json.contains("\"name\": \"queryVendors\""));
    assert!(readme.contains("vendor-service"));
    assert!(readme.contains("http://127.0.0.1:8082"));
    assert!(web_app.contains("ModelListPage"));
}

#[test]
fn generates_enum_metadata_for_schema_enums() {
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/studio.cstack");
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
    let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/studio.cstack");
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
    let vendor_schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../vaam-backends/services/vendor-service/schema/vendor.cstack");
    let auth_schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../vaam-backends/services/auth-service/schema/auth.cstack");
    let vendor_schema = cratestack_parser::parse_schema_file(&vendor_schema_path)
        .expect("vendor schema should parse");
    let auth_schema =
        cratestack_parser::parse_schema_file(&auth_schema_path).expect("auth schema should parse");

    let package = generate_package(
        &[
            StudioGeneratorContext {
                key: "vendor".to_owned(),
                display_name: "vendor-service".to_owned(),
                service_name: "vendor-service".to_owned(),
                schema_path: vendor_schema_path.clone(),
                service_url: "http://127.0.0.1:8082".to_owned(),
                schema: &vendor_schema,
            },
            StudioGeneratorContext {
                key: "auth".to_owned(),
                display_name: "auth-service".to_owned(),
                service_name: "auth-service".to_owned(),
                schema_path: auth_schema_path.clone(),
                service_url: "http://127.0.0.1:8081".to_owned(),
                schema: &auth_schema,
            },
        ],
        &StudioGeneratorConfig {
            name: "vaam-studio".to_owned(),
            mount_path: "/studio".to_owned(),
            profile: StudioProfile::Dev,
            template_dir: None,
        },
    )
    .expect("multi-context studio package should generate");

    let metadata_json = package_file(&package, "shared/src/metadata.json");
    let readme = package_file(&package, "README.md");

    assert!(metadata_json.contains("\"contexts\":"));
    assert!(metadata_json.contains("\"key\": \"vendor\""));
    assert!(metadata_json.contains("\"key\": \"auth\""));
    assert!(readme.contains("vendor-service"));
    assert!(readme.contains("auth-service"));
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
