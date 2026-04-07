use tracing::debug;

/// A section in the composed prompt.
#[derive(Debug, Clone)]
pub struct Section {
    pub label: String,
    pub content: String,
}

/// Fluent builder for composing multi-section prompts.
pub struct Composer {
    sections: Vec<Section>,
}

impl Composer {
    pub fn new() -> Self {
        Self {
            sections: Vec::new(),
        }
    }

    /// Appends a free-form section.
    pub fn add_section(mut self, label: &str, content: &str) -> Self {
        if content.is_empty() {
            return self;
        }
        self.sections.push(Section {
            label: label.to_string(),
            content: content.to_string(),
        });
        self
    }

    /// Joins all sections into a single string.
    pub fn build(&self) -> String {
        self.sections
            .iter()
            .map(|s| s.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Returns all sections for inspection.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// Emits a structured debug log of the composition manifest.
    pub fn log_manifest(&self, msg: &str) {
        let manifest: Vec<String> = self
            .sections
            .iter()
            .map(|s| format!("{} ({}B)", s.label, s.content.len()))
            .collect();

        debug!(
            msg,
            sections = manifest.len(),
            manifest = ?manifest,
            total_len = self.build().len(),
        );
    }
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_section_skipped() {
        let result = Composer::new()
            .add_section("Empty", "")
            .add_section("Full", "content")
            .build();

        assert_eq!(result, "content");
    }

    #[test]
    fn sections_accessible() {
        let composer = Composer::new()
            .add_section("A", "a")
            .add_section("B", "b");

        assert_eq!(composer.sections().len(), 2);
        assert_eq!(composer.sections()[0].label, "A");
    }
}
