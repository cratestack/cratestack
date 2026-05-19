use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn generates_fetch_client_and_tanstack_hooks_for_blog_schema() {
    let schema = cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "@example/blog-client".to_owned(),
            base_path: "/cstack".to_owned(),
            template_dir: None,
        },
    )
    .expect("default template should render");

    assert_eq!(package.files.len(), 9);

    let package_json = package_file(&package, "package.json");
    let readme = package_file(&package, "README.md");
    let runtime = package_file(&package, "src/runtime.ts");
    let queries = package_file(&package, "src/queries.ts");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");
    let react_query = package_file(&package, "src/react-query.ts");
    let index = package_file(&package, "src/index.ts");

    assert!(package_json.contains("\"name\": \"@example/blog-client\""));
    assert!(package_json.contains("\"@tanstack/react-query\": \"^5.0.0\""));
    assert!(readme.contains("Generated CrateStack TypeScript client"));
    assert!(readme.contains("client.procedures.publishPost"));
    assert!(runtime.contains("this.basePath = options.basePath ?? \"/cstack\";"));
    assert!(runtime.contains("class CratestackRuntime"));
    assert!(runtime.contains("class CratestackHttpError"));
    assert!(queries.contains("export interface CratestackFetchQuery"));
    assert!(queries.contains("output[`includeFields[${path}]`] = fields.join(\",\");"));
    assert!(models.contains("export interface Post"));
    assert!(models.contains("title?: string;"));
    assert!(models.contains("subtitle?: string | null;"));
    assert!(models.contains("author?: User;"));
    assert!(models.contains("export interface CreatePostInput"));
    assert!(models.contains("export interface UpdatePostInput"));
    assert!(models.contains("title?: string;"));
    assert!(models.contains("export interface GetFeedArgs"));
    assert!(models.contains("limit?: number | null;"));
    assert!(client.contains("export class ExampleBlogClientClient"));
    assert!(client.contains("readonly posts: PostApi;"));
    assert!(client.contains("list(options: CratestackQueryRequestConfig = {}): Promise<Post[]>"));
    assert!(
        client.contains("list(options: CratestackQueryRequestConfig = {}): Promise<Page<Session>>")
    );
    assert!(
        client.contains("return this.runtime.post<Post>(\"/$procs/publishPost\", args, options);")
    );
    assert!(react_query.contains("useQuery"));
    assert!(react_query.contains("useMutation"));
    assert!(react_query.contains("usePostListQuery"));
    assert!(react_query.contains("usePublishPostMutation"));
    assert!(index.contains("export * from \"./react-query\";"));
}

#[test]
fn preserves_enums_and_scalar_mappings() {
    let schema = cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/enums.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "src/models.ts");
    let client = package_file(&package, "src/client.ts");

    assert!(models.contains("export type Role = 'admin' | 'member';"));
    assert!(models.contains("export const RoleValues = ["));
    assert!(models.contains("role?: Role;"));
    assert!(client.contains("resolveUser(args: ResolveUserArgs"));
}

fn package_file<'a>(
    package: &'a cratestack_client_typescript::GeneratedTypeScriptPackage,
    file_name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
