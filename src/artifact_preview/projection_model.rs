//! Bounded UI conversion; immutable measurement input is built on the provider worker.
use super::measurement::MeasurementBlock;
use super::*;
use std::collections::HashMap;

pub(super) struct MarkdownModel {
    blocks: Vec<UiPreparedMarkdownBlock>,
    pub measurements: Arc<Vec<MeasurementBlock>>,
    cache: RefCell<HashMap<usize, ArtifactPreviewBlock>>,
}
impl MarkdownModel {
    pub fn new(
        blocks: Vec<UiPreparedMarkdownBlock>,
        measurements: Arc<Vec<MeasurementBlock>>,
    ) -> Self {
        Self {
            blocks,
            measurements,
            cache: RefCell::new(HashMap::new()),
        }
    }
}
impl Model for MarkdownModel {
    type Data = ArtifactPreviewBlock;
    fn row_count(&self) -> usize {
        self.blocks.len()
    }
    fn row_data(&self, index: usize) -> Option<Self::Data> {
        if let Some(row) = self.cache.borrow().get(&index) {
            return Some(row.clone());
        }
        let row = self.blocks.get(index)?.clone().into_ui_block();
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(index, row.clone());
        Some(row)
    }
    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub(super) struct CodeLinesModel {
    lines: Arc<Vec<UiPreparedCodeLine>>,
    cache: RefCell<HashMap<usize, ArtifactPreviewCodeLine>>,
}
impl CodeLinesModel {
    pub fn new(lines: Arc<Vec<UiPreparedCodeLine>>) -> Self {
        Self {
            lines,
            cache: RefCell::new(HashMap::new()),
        }
    }
}
impl Model for CodeLinesModel {
    type Data = ArtifactPreviewCodeLine;
    fn row_count(&self) -> usize {
        self.lines.len()
    }
    fn row_data(&self, index: usize) -> Option<Self::Data> {
        if let Some(row) = self.cache.borrow().get(&index) {
            return Some(row.clone());
        }
        let row = ArtifactPreviewCodeLine {
            tokens: ModelRc::new(VecModel::from(
                self.lines
                    .get(index)?
                    .tokens
                    .iter()
                    .map(|token| ArtifactPreviewCodeToken {
                        text: token.text.as_str().into(),
                        light_color: slint::Color::from_argb_u8(
                            token.light_color[0],
                            token.light_color[1],
                            token.light_color[2],
                            token.light_color[3],
                        ),
                        dark_color: slint::Color::from_argb_u8(
                            token.dark_color[0],
                            token.dark_color[1],
                            token.dark_color[2],
                            token.dark_color[3],
                        ),
                        bold: token.bold,
                        italic: token.italic,
                    })
                    .collect::<Vec<_>>(),
            )),
        };
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= 128 {
            cache.clear();
        }
        cache.insert(index, row.clone());
        Some(row)
    }
    fn model_tracker(&self) -> &dyn slint::ModelTracker {
        &()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_fence_converts_only_requested_rows_and_bounds_cache() {
        let lines = Arc::new(
            (0..12000)
                .map(|index| UiPreparedCodeLine {
                    tokens: vec![UiPreparedCodeToken {
                        text: format!("line {index} 中文"),
                        light_color: [255, 1, 2, 3],
                        dark_color: [255, 4, 5, 6],
                        bold: false,
                        italic: false,
                    }],
                })
                .collect(),
        );
        let model = CodeLinesModel::new(lines);
        assert!(model.cache.borrow().is_empty());
        assert_eq!(model.row_count(), 12000);
        for index in (0..12000).step_by(19) {
            let row = model.row_data(index).unwrap();
            assert_eq!(
                row.tokens.row_data(0).unwrap().text,
                format!("line {index} 中文")
            );
            assert!(model.cache.borrow().len() <= 128);
        }
        assert!(model.row_data(12000).is_none());
    }
}
