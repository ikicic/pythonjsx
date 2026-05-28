mod chunks;
mod emitter;
pub mod error;
pub(crate) mod html_entities;
pub mod opcodes;
mod settings;
pub mod sourcemap;
mod visitor;

pub use emitter::Emitter;
pub use error::{CompileError, CompileErrorSeverity};
pub use settings::CompilerSettings;
pub use sourcemap::{LineColumnMap, MapResult, SourceMap, SourceMapNode};
pub use visitor::compile;

pub type CompileResult = Result<
    (String, SourceMap, Vec<CompileError>),
    tree_sitter::LanguageError,
>;

/// Compiles .px (Python+JSX) files to .py.
pub struct Compiler {
    pub settings: CompilerSettings,
}

impl Compiler {
    pub fn new(settings: Option<CompilerSettings>) -> Self {
        Self {
            settings: settings.unwrap_or_default(),
        }
    }

    pub fn compile(
        &self,
        source: &str,
    ) -> Result<(String, SourceMap, Vec<CompileError>), tree_sitter::LanguageError> {
        compile(source, &self.settings)
    }
}
