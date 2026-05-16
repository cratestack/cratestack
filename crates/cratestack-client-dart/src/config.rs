use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DartGeneratorConfig {
    pub library_name: String,
    pub base_path: String,
    pub template_dir: Option<PathBuf>,
}

impl Default for DartGeneratorConfig {
    fn default() -> Self {
        Self {
            library_name: "cratestack_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartFile {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedDartPackage {
    pub files: Vec<GeneratedDartFile>,
}

#[derive(Debug, thiserror::Error)]
pub enum DartGeneratorError {
    #[error("failed to read template '{template_name}' from {path}: {source}")]
    TemplateRead {
        path: String,
        template_name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to register template '{0}': {1}")]
    TemplateRegistration(&'static str, #[source] minijinja::Error),
    #[error("failed to render template '{0}': {1}")]
    TemplateRender(&'static str, #[source] minijinja::Error),
}
