use std::{collections::HashSet, env, time::Duration};

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub ping_interval: Option<Duration>,
    pub allowed_origins: Option<HashSet<String>>,
}

impl Config {
    pub fn from_env() -> Self {
        fn read_u64(key: &str, default: u64) -> u64 {
            env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
        }
        let port = env::var("PORT").ok().and_then(|v| v.parse::<u16>().ok()).unwrap_or(8080);
        let ping_secs = read_u64("PING_INTERVAL", 0);
        let allowed_origins = {
            let raw = env::var("ALLOWED_ORIGINS").unwrap_or_default();
            let items: Vec<_> = raw
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
                .collect();
            if items.is_empty() { None } else { Some(items.into_iter().collect()) }
        };
        Self {
            port,
            ping_interval: if ping_secs > 0 { Some(Duration::from_secs(ping_secs)) } else { None },
            allowed_origins,
        }
    }
}
