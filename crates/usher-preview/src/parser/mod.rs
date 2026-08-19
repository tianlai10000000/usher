mod code;
mod markdown;

pub use code::{
    CodeParser, CodeParserLoader, CodeParserRegistry, JavaScriptParserLoader, ParserLoadStrategy,
    SyntectParserLoader, TsParserLoader,
};
pub use markdown::{MarkdownParser, MarkdownParserLoader, parse_markdown};

#[cfg(test)]
pub(crate) use markdown::parse_markdown_inner;
