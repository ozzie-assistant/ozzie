use ozzie_core::config::GatewayConfig;

use super::super::section::{BuildOutput, CollectResult, ConfigSection, FieldSpec, FieldValue, InputCollector};

const HOST: &str = "host";
const PORT: &str = "port";

pub struct GatewaySection;

#[async_trait::async_trait]
impl ConfigSection for GatewaySection {
    type Output = GatewayConfig;

    fn id(&self) -> &str {
        super::super::section::SectionId::Gateway.as_str()
    }

    fn should_skip(&self, current: Option<&Self::Output>) -> bool {
        current.is_some()
    }

    fn fields(&self, current: Option<&Self::Output>) -> Vec<FieldSpec> {
        let defaults = current.cloned().unwrap_or_default();
        vec![
            FieldSpec::text_default(HOST, &defaults.host),
            FieldSpec::text_default(PORT, &defaults.port.to_string()),
        ]
    }

    fn validate(&self, fragment: &Self::Output) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if fragment.port == 0 {
            errors.push("gateway port must be > 0".to_string());
        }
        if fragment.host.is_empty() {
            errors.push("gateway host must not be empty".to_string());
        }
        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    async fn build(
        &self,
        collector: &mut dyn InputCollector,
        current: Option<&Self::Output>,
    ) -> anyhow::Result<Option<BuildOutput<Self::Output>>> {
        let defaults = current.cloned().unwrap_or_default();

        let fields = vec![
            FieldSpec::text_default(HOST, &defaults.host),
            FieldSpec::text_default(PORT, &defaults.port.to_string()),
        ];

        let values = match collector.collect(self.id(), &fields)? {
            CollectResult::Values(v) => v,
            _ => return Ok(None),
        };

        let host = values
            .get(HOST)
            .and_then(FieldValue::as_text)
            .unwrap_or("127.0.0.1")
            .to_string();

        let port: u16 = values
            .get(PORT)
            .and_then(FieldValue::as_text)
            .and_then(|s| s.parse().ok())
            .unwrap_or(18420);

        Ok(Some(BuildOutput::new(GatewayConfig { host, port })))
    }

    fn apply_field(
        &self,
        current: &Self::Output,
        field_path: &str,
        value: &str,
    ) -> anyhow::Result<BuildOutput<Self::Output>> {
        let mut cfg = current.clone();
        match field_path {
            "host" => cfg.host = value.to_string(),
            "port" => {
                cfg.port = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid port: {value}"))?;
            }
            other => anyhow::bail!("unknown field '{other}' for gateway section"),
        }
        self.validate(&cfg).map_err(|e| anyhow::anyhow!("{}", e.join(", ")))?;
        Ok(BuildOutput::new(cfg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::section::{CollectResult, FieldValues};

    /// Test helper: collector that returns pre-defined values in FIFO order.
    struct MockCollector(Vec<FieldValues>);

    impl InputCollector for MockCollector {
        fn collect(
            &mut self,
            _id: &str,
            _fields: &[FieldSpec],
        ) -> anyhow::Result<CollectResult> {
            if self.0.is_empty() {
                return Ok(CollectResult::Back);
            }
            Ok(CollectResult::Values(self.0.remove(0)))
        }
    }

    #[tokio::test]
    async fn build_from_values() {
        let section = GatewaySection;
        let values = FieldValues::from([
            (HOST.to_string(), FieldValue::Text("0.0.0.0".to_string())),
            (PORT.to_string(), FieldValue::Text("9000".to_string())),
        ]);
        let mut collector = MockCollector(vec![values]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.host, "0.0.0.0");
        assert_eq!(output.config.port, 9000);
    }

    #[tokio::test]
    async fn build_defaults() {
        let section = GatewaySection;
        let mut collector = MockCollector(vec![FieldValues::new()]);

        let output = section.build(&mut collector, None).await.unwrap().unwrap();
        assert_eq!(output.config.host, "127.0.0.1");
        assert_eq!(output.config.port, 18420);
    }

    #[tokio::test]
    async fn build_back_returns_none() {
        let section = GatewaySection;
        let mut collector = MockCollector(vec![]);

        let result = section.build(&mut collector, None).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn validate_zero_port() {
        let section = GatewaySection;
        let config = GatewayConfig { host: "localhost".to_string(), port: 0 };
        assert!(section.validate(&config).is_err());
    }

    #[test]
    fn validate_ok() {
        let section = GatewaySection;
        let config = GatewayConfig { host: "127.0.0.1".to_string(), port: 18420 };
        assert!(section.validate(&config).is_ok());
    }

    #[test]
    fn fields_from_existing() {
        let section = GatewaySection;
        let existing = GatewayConfig { host: "10.0.0.1".to_string(), port: 8080 };
        let fields = section.fields(Some(&existing));
        assert_eq!(fields.len(), 2);
    }
}
