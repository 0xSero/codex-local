use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
struct ConfigToml {
    pub orchestrator_profile: Option<String>,
    #[serde(default)]
    pub agent_profiles: Option<Vec<String>>,
    #[serde(default)]
    pub profiles: HashMap<String, serde_json::Value>,
}

fn main() {
    let config_content = std::fs::read_to_string("/Users/sero/.codex-local/config.toml")
        .expect("Failed to read config file");

    println!("=== Raw TOML content (first 100 chars) ===");
    println!("{}", &config_content[..100.min(config_content.len())]);
    println!("...");

    let config: ConfigToml = toml::from_str(&config_content)
        .expect("Failed to parse TOML");

    println!("\n=== Parsed configuration ===");
    println!("orchestrator_profile: {:?}", config.orchestrator_profile);
    println!("agent_profiles: {:?}", config.agent_profiles);
}