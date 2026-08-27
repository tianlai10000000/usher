mod code;
mod markdown;

pub use code::{
    CodeParser, CodeParserLoader, CodeParserRegistry, JavaScriptParserLoader, ParserLoadStrategy,
    PlainTextParserLoader, TsParserLoader,
};
pub use markdown::{
    MarkdownParser, MarkdownParserLoader, parse_markdown, parse_markdown_structure,
    parse_markdown_structure_with_cancellation,
};

#[cfg(test)]
pub(crate) use markdown::parse_markdown_inner;
