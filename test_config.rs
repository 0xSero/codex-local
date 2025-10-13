use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
struct TestConfig {
    pub orchestrator_profile: Option<String>,
    #[serde(default)]
    pub agent_profiles: Option<Vec<String>>,
}

fn main() {
    let config_path = PathBuf::from("/Users/sero/.codex-local/config.toml");

    println!("Reading config from: {:?}", config_path);

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        println!("Config file content preview:");
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if i < 10 || i >= lines.len() - 10 {
                println!("  {}: {}", i + 1, line);
            } else if i == 10 {
                println!("  ...");
            }
        }

        match toml::from_str::<TestConfig>(&content) {
            Ok(config) => {
                println!("\nParsed config:");
                println!("  orchestrator_profile: {:?}", config.orchestrator_profile);
                println!("  agent_profiles: {:?}", config.agent_profiles);
            }
            Err(e) => {
                println!("Error parsing config: {}", e);
            }
        }
    } else {
        println!("Could not read config file");
    }
}