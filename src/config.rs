use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoLockTimeout {
    Never,
    OneMin,
    FiveMin,
    FifteenMin,
    ThirtyMin,
    OneHour,
}

impl AutoLockTimeout {
    pub const ALL: &'static [AutoLockTimeout] = &[
        AutoLockTimeout::Never,
        AutoLockTimeout::OneMin,
        AutoLockTimeout::FiveMin,
        AutoLockTimeout::FifteenMin,
        AutoLockTimeout::ThirtyMin,
        AutoLockTimeout::OneHour,
    ];

    pub fn seconds(self) -> Option<u64> {
        match self {
            AutoLockTimeout::Never => None,
            AutoLockTimeout::OneMin => Some(60),
            AutoLockTimeout::FiveMin => Some(300),
            AutoLockTimeout::FifteenMin => Some(900),
            AutoLockTimeout::ThirtyMin => Some(1800),
            AutoLockTimeout::OneHour => Some(3600),
        }
    }

    pub fn from_seconds(s: Option<u64>) -> Self {
        match s {
            None => AutoLockTimeout::Never,
            Some(n) => AutoLockTimeout::ALL
                .iter()
                .copied()
                .find(|t| t.seconds() == Some(n))
                .unwrap_or(AutoLockTimeout::Never),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AutoLockTimeout::Never => "Never",
            AutoLockTimeout::OneMin => "1 minute",
            AutoLockTimeout::FiveMin => "5 minutes",
            AutoLockTimeout::FifteenMin => "15 minutes",
            AutoLockTimeout::ThirtyMin => "30 minutes",
            AutoLockTimeout::OneHour => "1 hour",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub auto_lock: AutoLockTimeout,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auto_lock: AutoLockTimeout::Never,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("account-manager");
    p.push("config.conf");
    Some(p)
}

impl AppConfig {
    pub fn load() -> Self {
        let path = match config_path() {
            Some(p) => p,
            None => return Self::default(),
        };
        let contents = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        let mut cfg = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = match line.split_once('=') {
                Some(x) => x,
                None => continue,
            };
            if k.trim() == "auto_lock_seconds" {
                let v = v.trim();
                if v == "never" || v.is_empty() {
                    cfg.auto_lock = AutoLockTimeout::Never;
                } else if let Ok(n) = v.parse::<u64>() {
                    cfg.auto_lock = AutoLockTimeout::from_seconds(Some(n));
                }
            }
        }
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or_else(|| "no HOME dir".to_string())?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let value = match self.auto_lock.seconds() {
            None => "never".to_string(),
            Some(n) => n.to_string(),
        };
        let content = format!("auto_lock_seconds={}\n", value);
        fs::write(&path, content).map_err(|e| e.to_string())
    }
}
