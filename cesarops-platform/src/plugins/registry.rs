pub struct PluginInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    pub enabled: bool,
    pub feature_flag: &'static str,
}

pub struct PluginRegistry {
    plugins: Vec<PluginInfo>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        let plugins = vec![
            PluginInfo {
                name: "k9",
                version: "1.0.0",
                description: "Canine assistance module",
                enabled: cfg!(feature = "k9"),
                feature_flag: "k9",
            },
            PluginInfo {
                name: "water",
                version: "1.0.0",
                description: "Aquatic environment module",
                enabled: cfg!(feature = "water"),
                feature_flag: "water",
            },
            PluginInfo {
                name: "drone",
                version: "1.0.0",
                description: "Unmanned aerial vehicle module",
                enabled: cfg!(feature = "drone"),
                feature_flag: "drone",
            },
            PluginInfo {
                name: "maritime",
                version: "1.0.0",
                description: "Sea and ocean navigation module",
                enabled: cfg!(feature = "maritime"),
                feature_flag: "maritime",
            },
            PluginInfo {
                name: "cave",
                version: "1.0.0",
                description: "Subterranean exploration module",
                enabled: cfg!(feature = "cave"),
                feature_flag: "cave",
            },
            PluginInfo {
                name: "alpine",
                version: "1.0.0",
                description: "High altitude mountain module",
                enabled: cfg!(feature = "alpine"),
                feature_flag: "alpine",
            },
        ];

        Self { plugins }
    }

    pub fn list_enabled(&self) -> Vec<&PluginInfo> {
        self.plugins.iter().filter(|p| p.enabled).collect()
    }

    pub fn list_all(&self) -> Vec<&PluginInfo> {
        self.plugins.iter().collect()
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.plugins
            .iter()
            .find(|p| p.name == name)
            .map_or(false, |p| p.enabled)
    }
}
